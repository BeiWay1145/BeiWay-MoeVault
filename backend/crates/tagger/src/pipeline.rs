//! 打标流水线：对未打标的图片执行 溯源 → 爬标签 → 回退本地推理。
//!
//! 流程（docs/PLAN.md 2.3）：
//! 1. 查 sauce_cache（按 md5）：命中则直接用缓存结果
//! 2. SauceNAO 溯源 → 有效判定（相似度≥阈值 且 ext_urls 含 booru 链接）
//! 3. 有效 → 爬取 danbooru/gelbooru 标签，source 记 danbooru/gelbooru
//! 4. 无效 → 回退本地 cl_tagger（推理服务 HTTP），source 记 local
//!
//! 单张失败不中断批次，记 failed 继续。

use std::path::Path;

use moevault_db::Db;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::booru;
use crate::{ApiKeyPool, SauceNaoClient, TaggerError};

/// 打标进度统计。
#[derive(Debug, Clone, Default, Copy)]
pub struct TagProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
}

/// 本地推理服务客户端（HTTP 调用 Python 服务）。
#[derive(Clone)]
pub struct InferClient {
    http: reqwest::Client,
    pub(crate) base_url: String,
}

impl InferClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("构建推理客户端失败"),
            base_url,
        }
    }

    /// 通用 HTTP 客户端（供 booru 爬取复用）。
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 调用 /infer/tags 获取本地标签。
    pub async fn infer_tags(&self, path: &Path, threshold: f64) -> Result<Vec<(String, f64)>, TaggerError> {
        #[derive(Serialize)]
        struct Req<'a> {
            path: &'a str,
            threshold: f64,
        }
        #[derive(Deserialize)]
        struct TagItem {
            name: String,
            confidence: f64,
        }
        #[derive(Deserialize)]
        struct TagResp {
            tags: Vec<TagItem>,
        }
        let resp = self
            .http
            .post(format!("{}/infer/tags", self.base_url))
            .json(&Req {
                path: &path.to_string_lossy(),
                threshold,
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(TaggerError::Invalid(format!(
                "推理服务返回 {}",
                resp.status()
            )));
        }
        let body: TagResp = resp.json().await?;
        Ok(body.tags.into_iter().map(|t| (t.name, t.confidence)).collect())
    }
}

/// 执行打标流水线。
///
/// - `db`：SQLite
/// - `sauce`：SauceNAO 客户端（无状态）
/// - `pool`：多 API key 调度器
/// - `infer`：本地推理客户端
/// - `library_dir`：库目录（图片路径）
/// - `min_sim`：溯源相似度阈值（默认 75）
/// - `tag_threshold`：本地打标置信度阈值（默认 0.5）
/// - `image_ids`：None = 全部未打标 active 图；Some = 指定图（强制重打，跳过不可溯源标记）
#[allow(clippy::too_many_arguments)]
pub async fn run_tag_pipeline(
    db: &Db,
    sauce: &SauceNaoClient,
    pool: &ApiKeyPool,
    infer: &InferClient,
    library_dir: &Path,
    min_sim: f64,
    tag_threshold: f64,
    image_ids: Option<Vec<i64>>,
) -> Result<TagProgress, TaggerError> {
    let is_force = image_ids.is_some();
    let ids = match image_ids {
        Some(ids) => ids,
        None => db.untagged_active_images(10000)?,
    };
    if ids.is_empty() {
        return Ok(TagProgress::default());
    }
    info!(count = ids.len(), "打标流水线：开始");

    // force 模式（指定 ids）：清除不可溯源标记，允许强制重新溯源
    if is_force {
        for id in &ids {
            db.set_no_auto_sauce(*id, false)?;
        }
    }
    let mut progress = TagProgress {
        total: ids.len(),
        ..Default::default()
    };

    for image_id in &ids {
        let result = tag_one(db, sauce, pool, infer, library_dir, min_sim, tag_threshold, *image_id).await;
        match result {
            Ok(()) => progress.done += 1,
            Err(e) => {
                warn!(image_id, error = %e, "打标失败");
                progress.failed += 1;
            }
        }
    }
    info!(done = progress.done, failed = progress.failed, "打标流水线完成");
    Ok(progress)
}

#[allow(clippy::too_many_arguments)]
async fn tag_one(
    db: &Db,
    sauce: &SauceNaoClient,
    pool: &ApiKeyPool,
    infer: &InferClient,
    library_dir: &Path,
    min_sim: f64,
    tag_threshold: f64,
    image_id: i64,
) -> Result<(), TaggerError> {
    let img = db
        .get_image_by_id(image_id)?
        .ok_or_else(|| TaggerError::Invalid(format!("图片 {image_id} 不存在")))?;
    let file_path = library_dir.join(&img.rel_path);

    // 不可溯源标记检查：已标记的图不自动溯源（用户手动 force 时跳过此检查）
    if img.no_auto_sauce {
        info!(image_id, "图片已标记不可溯源，跳过自动溯源（可用手动 retag 强制）");
        return Err(TaggerError::NoSource("图片标记为不可溯源".into()));
    }

    // 1. 溯源缓存命中（仅用于避免重复溯源；标签结果以实际入库为准）
    if db.get_sauce_cache(&img.md5)?.is_some() {
        // 缓存存在说明之前已溯源过（无论成败）——这里仍允许重新尝试爬取，
        // 但为简化：缓存命中且图片已有自动标签则跳过；否则继续溯源流程。
        if db.image_has_auto_tags(image_id)? {
            return Ok(());
        }
    }

    // 2. 从调度器获取可用 key
    let (api_key, key_idx) = pool.acquire().await;

    // 3. SauceNAO 溯源（带 key）
    let (result, quota) = match sauce.search_file(&file_path, &api_key).await {
        Ok(r) => r,
        Err(e) => {
            // 请求失败：标记 key 失败（冷却），溯源失败走本地打标
            pool.on_failure(key_idx).await;
            let is_no_result = matches!(e, TaggerError::NoSource(_));
            warn!(image_id, error = %e, is_no_result, "溯源失败，回退本地打标");
            db.put_sauce_cache(&img.md5, 0.0, None, None, None)?;
            // 无结果（SauceNAO 0 命中）→ 标记不可自动溯源，防止浪费配额；
            // 限流/网络错误等不标记（下次可重试）
            if is_no_result {
                db.set_no_auto_sauce(image_id, true)?;
            }
            return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
        }
    };
    // 成功：更新配额头 + 进入短冷却（保守 5s，防连发撞限流）
    pool.update(key_idx, quota.short_remaining, quota.long_remaining).await;
    pool.start_cooldown(key_idx, 5).await;

    // 4. 有效判定：相似度 ≥ 阈值 且 ext_urls 含 booru 链接
    if result.similarity < min_sim {
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        // 相似度不足（非限流问题）：标记不可自动溯源，防止浪费配额
        db.set_no_auto_sauce(image_id, true)?;
        return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
    }
    let fetched = booru::fetch_tags(infer.http(), &result.ext_urls).await;
    let Some((source, source_url, tags)) = fetched.ok() else {
        // 命中 booru 但爬取失败：记缓存 + 本地回退；不标记不可溯源（下次可重试）
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
    };

    // 5. 存标签
    save_tags(db, image_id, source, Some(&source_url), &tags, result.similarity)?;
    db.put_sauce_cache(&img.md5, result.similarity, Some(source), Some(&source_url), None)?;
    db.set_image_source(image_id, source, Some(&source_url))?;
    info!(image_id, source, tag_count = tags.len(), "溯源打标成功");
    Ok(())
}

/// 保存标签（tags upsert + image_tags 写入）。
/// image_tags.source 遵守 CHECK 约束（auto_danbooru/auto_gelbooru/auto_local/manual）。
fn save_tags(
    db: &Db,
    image_id: i64,
    source: &str,
    source_url: Option<&str>,
    tags: &[String],
    _similarity: f64,
) -> Result<(), TaggerError> {
    let db_source = format!("auto_{source}");
    let mut tag_ids = Vec::new();
    for name in tags {
        let id = db.upsert_tag(name, "general")?;
        tag_ids.push((id, None));
    }
    db.insert_image_tags(image_id, &tag_ids, &db_source)?;
    let _ = source_url;
    Ok(())
}

/// 本地推理打标（回退路径）。
async fn apply_local_tags(
    db: &Db,
    infer: &InferClient,
    path: &Path,
    threshold: f64,
    image_id: i64,
) -> Result<(), TaggerError> {
    let tags = match infer.infer_tags(path, threshold).await {
        Ok(t) => t,
        Err(e) => {
            warn!(image_id, error = %e, "本地推理不可用（推理服务未启动？）");
            return Err(TaggerError::Invalid(format!("本地打标失败: {e}")));
        }
    };
    if tags.is_empty() {
        return Err(TaggerError::NoSource("本地打标无结果".into()));
    }
    let mut tag_ids = Vec::new();
    for (name, conf) in &tags {
        let id = db.upsert_tag(name, "general")?;
        tag_ids.push((id, Some(*conf)));
    }
    db.insert_image_tags(image_id, &tag_ids, "auto_local")?;
    db.set_image_source(image_id, "local", None)?;
    info!(image_id, tag_count = tags.len(), "本地打标成功");
    Ok(())
}
