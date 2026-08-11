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

/// 溯源进度统计。
#[derive(Debug, Clone, Default, Copy)]
pub struct SauceProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
}

/// 溯源命中结果。
#[derive(Debug, Clone)]
pub struct SauceHit {
    pub source: String,
    pub source_url: String,
    pub tags: Vec<crate::booru::BooruTag>,
    pub similarity: f64,
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

    /// 通知推理服务切换打标模型目录（重载模型）。
    pub async fn use_tagger_model(&self, model_dir: &str) -> Result<(), TaggerError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            model_dir: &'a str,
        }
        let resp = self
            .http
            .post(format!("{}/infer/tagger/config", self.base_url))
            .json(&Req { model_dir })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(TaggerError::Invalid(format!(
                "推理服务切换模型返回 {}",
                resp.status()
            )));
        }
        Ok(())
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

/// 任务过滤：批量模式下排除无需处理的图片（不浪费配额）。
/// - `tag`：排除 AI 生成、已有自动标签、不可溯源
/// - `sauce`：排除 AI 生成、不可溯源、已溯源（有 source_url 或非 local 来源）
/// - `aesthetic`：排除已有美学分
pub(crate) fn filter_eligible(
    db: &Db,
    kind: &str,
    ids: &[i64],
) -> Result<Vec<i64>, TaggerError> {
    let mut out = Vec::new();
    for id in ids {
        let Some(img) = db.get_image_by_id(*id)? else { continue };
        // GIF 动图：打标/溯源/美学均跳过（帧图无意义，浪费配额/时间）
        if img.format.eq_ignore_ascii_case("gif") {
            tracing::info!(image_id = *id, "批量任务跳过（GIF 动图）");
            continue;
        }
        let tags = db.image_tags(*id).unwrap_or_default();
        let is_ai = img.ai_metadata.is_some()
            || tags.iter().any(|t| t.source == "ai");
        let has_auto_tags = tags
            .iter()
            .any(|t| matches!(t.source.as_str(), "auto_danbooru" | "auto_gelbooru" | "auto_local"));
        let is_sauced = img.source_url.is_some() || (img.source != "local" && !img.source.is_empty());
        let eligible = match kind {
            "tag" => !is_ai && !has_auto_tags && !img.no_auto_sauce,
            "sauce" => !is_ai && !img.no_auto_sauce && !is_sauced,
            "aesthetic" => img.aesthetic_score.is_none(),
            _ => true,
        };
        if eligible {
            out.push(*id);
        } else {
            tracing::info!(image_id = *id, kind, "批量任务跳过（无需处理）");
        }
    }
    Ok(out)
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
    job_id: Option<i64>,
) -> Result<TagProgress, TaggerError> {
    let is_force = image_ids.is_some();
    let ids = match image_ids {
        Some(ids) => {
            // 批量（>1 张）过滤无需处理的图；单张（详情页手动）保留强制语义
            if ids.len() > 1 {
                filter_eligible(db, "tag", &ids)?
            } else {
                ids
            }
        }
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
            Ok(()) => {
                progress.done += 1;
                let _ = db.add_log("info", "tag", &format!("图片 #{image_id} 打标成功"));
            }
            Err(e) => {
                warn!(image_id, error = %e, "打标失败");
                progress.failed += 1;
                let _ = db.add_log("error", "tag", &format!("图片 #{image_id} 打标失败：{e}"));
            }
        }
        // 实时写回 job 进度（任务中心进度条可见推进）
        if let Some(jid) = job_id {
            let _ = db.update_job(jid, "running", progress.done as i64, progress.failed as i64, None);
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

    // AI 生成图：prompt 比打标准确、溯源无意义 → 跳过打标与溯源
    // （已通过 ai-info 写入 source=ai 标签的图）
    let has_ai_tag = db
        .image_tags(image_id)?
        .iter()
        .any(|t| t.source == "ai");
    if img.ai_metadata.is_some() || has_ai_tag {
        if !has_ai_tag {
            // 已标记 AI 但未提取标签：尝试提取（尽力而为）
            if let Some(meta) = moevault_ingest::features::read_ai_metadata(&file_path) {
                if !meta.tags.is_empty() {
                    let tag_ids: Vec<(i64, Option<f64>)> = meta
                        .tags
                        .iter()
                        .map(|t| db.upsert_tag(t, "general").map(|tid| (tid, None)))
                        .collect::<Result<_, _>>()?;
                    db.insert_image_tags(image_id, &tag_ids, "ai")?;
                }
            }
        }
        info!(image_id, "AI 生成图，跳过打标与溯源（prompt 标签已用）");
        return Ok(());
    }

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

    // 2. 溯源（SauceNAO + booru 爬标签）；None = 未命中/失败（回退本地打标）
    let (api_key, key_idx) = pool.acquire().await;
    let hit = match sauce_one(db, sauce, pool, infer, library_dir, min_sim, image_id, api_key, key_idx).await? {
        Some(h) => h,
        None => {
            return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
        }
    };

    // 存标签
    save_tags(db, image_id, &hit.source, Some(&hit.source_url), &hit.tags, hit.similarity)?;
    db.put_sauce_cache(&img.md5, hit.similarity, Some(&hit.source), Some(&hit.source_url), None)?;
    db.set_image_source(image_id, &hit.source, Some(&hit.source_url))?;
    info!(image_id, source = %hit.source, tag_count = hit.tags.len(), "溯源打标成功");
    Ok(())
}

/// 单图 SauceNAO 溯源 + booru 爬标签（key 由调用方传入——并发调度器已 acquire）。
/// 返回 Some(命中) 或 None（未命中/失败，已写缓存与不可溯源标记；调用方决定回退策略）。
/// AI 生成图直接返回 Ok(None)（不消耗配额）。
#[allow(clippy::too_many_arguments)]
async fn sauce_one(
    db: &Db,
    sauce: &SauceNaoClient,
    pool: &ApiKeyPool,
    infer: &InferClient,
    library_dir: &Path,
    min_sim: f64,
    image_id: i64,
    api_key: String,
    key_idx: usize,
) -> Result<Option<SauceHit>, TaggerError> {
    let img = db
        .get_image_by_id(image_id)?
        .ok_or_else(|| TaggerError::Invalid(format!("图片 {image_id} 不存在")))?;
    let file_path = library_dir.join(&img.rel_path);

    // AI 生成图：溯源无意义，直接跳过
    let has_ai_tag = db.image_tags(image_id)?.iter().any(|t| t.source == "ai");
    if img.ai_metadata.is_some() || has_ai_tag {
        info!(image_id, "AI 生成图，跳过溯源");
        return Ok(None);
    }

    // 不可溯源标记：跳过（调用方 force 时由 run_tag_pipeline 预先清除）
    if img.no_auto_sauce {
        info!(image_id, "图片已标记不可溯源，跳过自动溯源");
        return Ok(None);
    }

    // SauceNAO 溯源（带 key）；失败也携带配额头，用于更新调度器
    let (result, quota) = match sauce.search_file(&file_path, &api_key).await {
        Ok(r) => r,
        Err((e, err_quota)) => {
            // 失败：先更新配额（若响应带配额），再标记 key 冷却
            pool.update(key_idx, err_quota.short_remaining, err_quota.long_remaining).await;
            pool.on_failure(key_idx).await;
            let is_no_result = matches!(e, TaggerError::NoSource(_));
            warn!(image_id, error = %e, is_no_result, "溯源失败");
            db.put_sauce_cache(&img.md5, 0.0, None, None, None)?;
            if is_no_result {
                db.set_no_auto_sauce(image_id, true)?;
            }
            return Ok(None);
        }
    };
    // 成功：更新配额头 + 进入短窗口冷却（30s，SauceNAO 免费账号真实限制）
    pool.update(key_idx, quota.short_remaining, quota.long_remaining).await;
    pool.start_cooldown(key_idx, 30).await;

    // 有效判定：相似度 ≥ 阈值 且 ext_urls 含 booru 链接
    if result.similarity < min_sim {
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        db.set_no_auto_sauce(image_id, true)?;
        return Ok(None);
    }
    let fetched = booru::fetch_tags(infer.http(), &result.ext_urls).await;
    let Some((source, source_url, tags)) = fetched.ok() else {
        // 命中 booru 但爬取失败：记缓存，不标记不可溯源（下次可重试）
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        return Ok(None);
    };

    Ok(Some(SauceHit {
        source: source.to_string(),
        source_url,
        tags,
        similarity: result.similarity,
    }))
}

/// 溯源专用管线：只做 SauceNAO 溯源 + booru 爬标签（失败不本地打标）。
/// - `image_ids`：None = 全部未溯源 active 图；Some = 指定图（强制重新溯源）。
/// - 并发调度：按可用 key 数起 worker，每个 worker 从共享队列取图处理，
///   单 key 串行（30s 短窗口冷却），多 key 并行推进——大批量不再被单 key 冷却拖死。
/// - `job_id`：传入时每处理完一张检查 job 状态，cancelled 则停止（供中断）。
#[allow(clippy::too_many_arguments)]
pub async fn run_sauce_pipeline(
    db: &Db,
    sauce: &SauceNaoClient,
    pool: &ApiKeyPool,
    infer: &InferClient,
    library_dir: &Path,
    min_sim: f64,
    image_ids: Option<Vec<i64>>,
    job_id: Option<i64>,
) -> Result<SauceProgress, TaggerError> {
    let ids = match image_ids {
        Some(ids) => {
            // 批量（>1 张）过滤无需处理的图；单张（详情页手动）保留强制语义
            if ids.len() > 1 {
                filter_eligible(db, "sauce", &ids)?
            } else {
                ids
            }
        }
        None => db.untagged_active_images(10000)?,
    };
    if ids.is_empty() {
        return Ok(SauceProgress::default());
    }
    info!(count = ids.len(), "溯源管线：开始");

    // 指定 ids：清除不可溯源标记，允许强制重新溯源
    for id in &ids {
        db.set_no_auto_sauce(*id, false)?;
    }

    // 共享工作队列 + 进度（并发 worker 共同消费）
    let queue: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<i64>>> =
        std::sync::Arc::new(std::sync::Mutex::new(ids.iter().copied().collect()));
    let progress = std::sync::Arc::new(std::sync::Mutex::new(SauceProgress {
        total: ids.len(),
        ..Default::default()
    }));

    // worker 数 = 可用 key 数（至少 1）
    let worker_count = pool.len().await.max(1);
    // SauceNaoClient / InferClient 无 Sync 要求但需 'static：Arc 包装
    let sauce = std::sync::Arc::new(sauce.clone());
    let infer = std::sync::Arc::new(infer.clone());
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let queue = queue.clone();
        let progress = progress.clone();
        let db = db.clone();
        let sauce = sauce.clone();
        let pool = pool.clone();
        let infer = infer.clone();
        let library_dir = library_dir.to_path_buf();
        handles.push(tokio::spawn(async move {
            loop {
                // 中断检查：任务被取消则停止（每轮处理前查一次 DB）
                if let Some(jid) = job_id {
                    if let Ok(Some(job)) = db.get_job(jid) {
                        if job.status == "cancelled" {
                            break;
                        }
                    }
                }
                // 取下一张图
                let image_id = {
                    let mut q = queue.lock().unwrap();
                    q.pop_front()
                };
                let Some(image_id) = image_id else { break };

                // acquire 会等待可用 key（含 30s 冷却结束后放行）
                let (api_key, key_idx) = pool.acquire().await;
                match sauce_one(
                    &db,
                    &sauce,
                    &pool,
                    &infer,
                    &library_dir,
                    min_sim,
                    image_id,
                    api_key,
                    key_idx,
                )                .await
                {
                    Ok(Some(hit)) => {
                        // 写标签 + source
                        save_tags(&db, image_id, &hit.source, Some(&hit.source_url), &hit.tags, hit.similarity)?;
                        if let Ok(Some(img)) = db.get_image_by_id(image_id) {
                            db.put_sauce_cache(&img.md5, hit.similarity, Some(&hit.source), Some(&hit.source_url), None)?;
                        }
                        db.set_image_source(image_id, &hit.source, Some(&hit.source_url))?;
                        let _ = db.add_log("info", "sauce", &format!("图片 #{image_id} 溯源成功（{}）", hit.source));
                        let mut p = progress.lock().unwrap();
                        p.done += 1;
                        let (d, f) = (p.done, p.failed);
                        drop(p);
                        // 实时写回 job 进度（任务中心能看到推进）
                        if let Some(jid) = job_id {
                            let _ = db.update_job(jid, "running", d as i64, f as i64, None);
                        }
                    }
                    Ok(None) => {
                        info!(image_id, "溯源无命中");
                        let _ = db.add_log("warn", "sauce", &format!("图片 #{image_id} 溯源无命中（AI 图/不可溯源/无匹配）"));
                        let mut p = progress.lock().unwrap();
                        p.failed += 1;
                        let (d, f) = (p.done, p.failed);
                        drop(p);
                        if let Some(jid) = job_id {
                            let _ = db.update_job(jid, "running", d as i64, f as i64, None);
                        }
                    }
                    Err(e) => {
                        warn!(image_id, error = %e, "溯源失败");
                        let _ = db.add_log("error", "sauce", &format!("图片 #{image_id} 溯源失败：{e}"));
                        let mut p = progress.lock().unwrap();
                        p.failed += 1;
                        let (d, f) = (p.done, p.failed);
                        drop(p);
                        if let Some(jid) = job_id {
                            let _ = db.update_job(jid, "running", d as i64, f as i64, None);
                        }
                    }
                }
            }
            Ok::<(), TaggerError>(())
        }));
    }
    // 等待所有 worker 完成（任一个出错则停止整体）
    let mut first_err: Option<TaggerError> = None;
    for h in handles {
        if let Err(e) = h.await {
            let msg = if e.is_panic() {
                "溯源 worker panic".to_string()
            } else {
                format!("溯源 worker 失败: {e}")
            };
            if first_err.is_none() {
                first_err = Some(TaggerError::Invalid(msg));
            }
        }
    }
    // 中断：剩余未处理数 = 队列剩余
    let remaining = queue.lock().unwrap().len();
    let progress = progress.lock().unwrap();
    info!(
        done = progress.done,
        failed = progress.failed,
        remaining,
        "溯源管线结束"
    );
    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(*progress)
}

/// 保存标签（tags upsert + image_tags 写入）。
/// image_tags.source 遵守 CHECK 约束（auto_danbooru/auto_gelbooru/auto_local/manual）。
/// category 按 danbooru 分类落库（artist/copyright/character/general）。
fn save_tags(
    db: &Db,
    image_id: i64,
    source: &str,
    source_url: Option<&str>,
    tags: &[crate::booru::BooruTag],
    _similarity: f64,
) -> Result<(), TaggerError> {
    let db_source = format!("auto_{source}");
    let mut tag_ids = Vec::new();
    for t in tags {
        let id = db.upsert_tag(&t.name, &t.category)?;
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
