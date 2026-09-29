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
use std::time::Duration;

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
    /// 推理设备：None = 不指定（服务端按 auto 处理）/ "cuda:0" / "cpu"。
    device: Option<String>,
}

impl InferClient {
    pub fn new(base_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                // 禁用空闲连接复用：uvicorn 会关闭 keep-alive 连接，reqwest 复用死连接会挂起
                // （打标偶发成功、美学稳定失败与此相关）。每次请求新建连接最稳。
                .pool_max_idle_per_host(0)
                .build()
                .expect("构建推理客户端失败"),
            base_url,
            device: None,
        }
    }

    /// 指定推理设备（链式调用）。device 为 None/空 时表示不干预（服务端 auto）。
    pub fn with_device(mut self, device: Option<String>) -> Self {
        self.device = device.filter(|d| !d.trim().is_empty());
        self
    }

    /// 当前指定的设备（供日志/诊断）。
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// 通用 HTTP 客户端（供 booru 爬取复用）。
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 通知推理服务切换打标模型目录/种类/推理设备（重载模型）。
    /// kind：cl_tagger / wd14 / auto（None=不指定，由推理服务按目录内容自动判定）
    /// device：随客户端配置透传（None 时不发送，服务端沿用当前值）
    pub async fn use_tagger_model(
        &self,
        model_dir: &str,
        model_kind: Option<&str>,
    ) -> Result<(), TaggerError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            model_dir: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            model_kind: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            device: Option<&'a str>,
        }
        let resp = self
            .http
            .post(format!("{}/infer/tagger/config", self.base_url))
            .json(&Req {
                model_dir,
                model_kind,
                device: self.device(),
            })
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
            #[serde(skip_serializing_if = "Option::is_none")]
            device: Option<&'a str>,
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
        // 传绝对路径：推理服务（python）cwd 与后端不同，
        // 相对路径（data\library\...）会导致「文件不存在」404。
        let abs_path = to_absolute_path(path)?;
        let resp = self
            .http
            .post(format!("{}/infer/tags", self.base_url))
            .json(&Req {
                path: &abs_path,
                threshold,
                device: self.device(),
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let body_snippet = resp
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>();
            return Err(TaggerError::Invalid(format!(
                "推理服务 /infer/tags 返回 {status}，响应: {body_snippet}"
            )));
        }
        let body: TagResp = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                return Err(TaggerError::Invalid(format!(
                    "推理服务 /infer/tags 响应解析失败: {e}"
                )));
            }
        };
        Ok(body.tags.into_iter().map(|t| (t.name, t.confidence)).collect())
    }
}

/// 任务过滤：批量模式下排除无需处理的图片（不浪费配额）。
/// - `tag`：仅排除「已有自动标签」与 GIF。**不受 `no_auto_sauce` 影响**——
///   该标记的语义是"不再自动溯源"，与打标无关（BUG1 修复：此前它同时阻断了打标，
///   导致溯源没命中的图永远无法打标）。
/// - `sauce`：排除 AI 生成、不可溯源（`no_auto_sauce`，force 时忽略）、已溯源
/// - `aesthetic`：排除已有美学分（force 时不过滤）
pub(crate) fn filter_eligible(
    db: &Db,
    kind: &str,
    ids: &[i64],
    force_sauce: bool,
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
            // 打标：AI 图也参与（无 prompt 标签时本地模型打标）。
            // 仅跳过「已有自动标签」与 GIF；**不看 no_auto_sauce**（那是溯源专用标记）。
            "tag" => !has_auto_tags,
            // 溯源：AI 图无需溯源（无来源），跳过
            "sauce" => !is_ai && (!img.no_auto_sauce || force_sauce) && !is_sauced,
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
    exits: Option<&crate::ExitPool>,
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
                filter_eligible(db, "tag", &ids, false)?
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
        let result = tag_one(db, sauce, pool, exits, infer, library_dir, min_sim, tag_threshold, *image_id).await;
        match result {
            Ok(()) => {
                progress.done += 1;
                let _ = db.add_log("info", "tag", &format!("图片 #{image_id} 打标成功"));
            }
            Err(e) => {
                warn!(image_id, error = %e, "打标失败");
                progress.failed += 1;
                let rel = db
                    .get_image_by_id(*image_id)
                    .ok()
                    .flatten()
                    .map(|i| i.rel_path)
                    .unwrap_or_default();
                let _ = db.add_log("error", "tag", &format!("图片 #{image_id}（{rel}）打标失败：{e}"));
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
    exits: Option<&crate::ExitPool>,
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

    // AI 生成图：参与打标——已有非 AI 来源的内容标签才跳过；
    // 否则提取 prompt 标签（source=ai）或本地模型打标，AI 标记不再导致打标被跳过。
    let tags_now = db.image_tags(image_id)?;
    let has_content_tag = tags_now.iter().any(|t| t.source != "ai");
    if img.ai_metadata.is_some() || tags_now.iter().any(|t| t.source == "ai") {
        if has_content_tag {
            info!(image_id, "AI 图已用 prompt 内容标签，无需打标");
            return Ok(());
        }
        // 无内容标签：先尝试提取 prompt 标签（仅当尚无 source=ai 标签时，避免重复）
        if !tags_now.iter().any(|t| t.source == "ai") {
            if let Some(meta) = moevault_ingest::features::read_ai_metadata(&file_path) {
                if !meta.tags.is_empty() {
                    let tag_ids: Vec<(i64, Option<f64>)> = meta
                        .tags
                        .iter()
                        .map(|t| db.upsert_tag(t, "general").map(|tid| (tid, None)))
                        .collect::<Result<_, _>>()?;
                    db.insert_image_tags(image_id, &tag_ids, "ai")?;
                    info!(image_id, tag_count = meta.tags.len(), "AI 图提取 prompt 标签");
                    return Ok(());
                }
            }
        }
        // 无 prompt 内容标签（或仅有 source=ai 标记）：本地模型打标，真正执行
        info!(image_id, "AI 图本地模型打标");
        return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
    }

    // 不可溯源标记：跳过「溯源」环节，但**继续本地模型打标**。
    // BUG1 修复：此前直接返回 Err，导致溯源没命中的图连本地打标也做不了
    // （表现为整批 53 张图提交打标任务后被全部跳过）。
    if img.no_auto_sauce {
        info!(image_id, "图片已标记不可溯源，跳过溯源，改用本地模型打标");
        return apply_local_tags(db, infer, file_path.as_path(), tag_threshold, image_id).await;
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
    //    限流时等待后重试同一张图（全局窗口+冷却由 pool 接管），避免误判为失败
    let mut hit: Option<SauceHit> = None;
    for attempt in 0..=MAX_RATE_RETRIES {
        let (api_key, key_idx) = pool.acquire().await;
        match sauce_one(db, sauce, pool, exits, infer, library_dir, min_sim, image_id, api_key, key_idx).await {
            Ok(h) => {
                hit = h;
                break;
            }
            Err(TaggerError::RateLimited(secs)) => {
                if attempt == MAX_RATE_RETRIES {
                    warn!(image_id, "打标溯源：限流重试次数耗尽，回退本地打标");
                    break;
                }
                let wait = secs.clamp(1, 120) as u64;
                warn!(image_id, retry_secs = wait, attempt = attempt + 1, "打标溯源限流，等待后重试");
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
            Err(e) => return Err(e),
        }
    }
    let hit = match hit {
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
    exits: Option<&crate::ExitPool>,
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

    // 增强2：多出口 IP 轮换 —— 从出口池取一个"有空闲额度"的出口（未配置时退化为直连）。
    // 调度语义（按需求）：当前出口被限流后会被标记冷却，下次自动换到其它可用出口。
    let (http_client, exit_idx) = match exits {
        Some(p) => {
            let (c, i) = p.acquire().await;
            (c, Some((p, i)))
        }
        None => (reqwest::Client::new(), None),
    };

    // SauceNAO 溯源（带 key）；失败也携带配额头，用于更新调度器
    let (result, quota) = match sauce.search_file_with(&http_client, &file_path, &api_key).await {
        Ok(r) => r,
        Err((e, err_quota)) => {
            // 先更新配额（若响应带配额头）
            pool.update(key_idx, err_quota.short_remaining, err_quota.long_remaining).await;
            match e {
                // 限流（-2/3）：整池进入全局冷却，把错误抛给 worker 重试同一张图，
                // 不再当作"该图失败"——否则一次限流会白扔掉整批图。
                TaggerError::RateLimited(secs) => {
                    pool.note_rate_limited(key_idx, secs.max(1) as u64).await;
                    // 该出口同样被限流（SauceNAO 按 IP 计）→ 标记冷却，
                    // 下次 acquire 会自动换到其它有空闲的出口。
                    if let Some((p, i)) = exit_idx {
                        p.note_rate_limited(i, secs.max(1) as u64).await;
                    }
                    return Err(TaggerError::RateLimited(secs));
                }
                // 调用成功但无匹配：正常消耗配额，不冷却（避免"无结果"白等 30s）
                TaggerError::NoSource(msg) => {
                    info!(image_id, "SauceNAO 无匹配结果");
                    db.put_sauce_cache(&img.md5, 0.0, None, None, None)?;
                    db.set_no_auto_sauce(image_id, true)?;
                    let _ = msg;
                    return Ok(None);
                }
                other => {
                    pool.on_failure(key_idx).await;
                    // 出口故障计数：网络/TLS 类失败很可能是**该出口**的问题
                    // （实测个别 Clash 出口在并发下会大量失败）。
                    // 累计到阈值后该出口被停用，调度自动改用其它出口，
                    // 而不是让整批图反复撞在坏出口上。
                    if let Some((p, i)) = exit_idx {
                        p.note_failure(i).await;
                    }
                    warn!(image_id, error = %other, "溯源失败");
                    db.put_sauce_cache(&img.md5, 0.0, None, None, None)?;
                    return Ok(None);
                }
            }
        }
    };
    // 成功：更新配额头；仅当短窗口配额耗尽时才冷却，否则继续用（全局窗口限流兜底）
    pool.update(key_idx, quota.short_remaining, quota.long_remaining).await;
    // 出口请求成功 → 清零其连续失败计数
    if let Some((p, i)) = exit_idx {
        p.note_success(i).await;
    }
    match quota.short_remaining {
        Some(0) => pool.start_cooldown(key_idx, 30).await,
        Some(_) => {}
        None => pool.start_cooldown(key_idx, 5).await, // 无配额头：保守短冷却
    }

    // 有效判定：相似度 ≥ 阈值 且 ext_urls 含 booru 链接
    if result.similarity < min_sim {
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        db.set_no_auto_sauce(image_id, true)?;
        return Ok(None);
    }
    let fetched = booru::fetch_tags(infer.http(), &result.ext_urls).await;
    let Some((source, source_url, tags)) = fetched.ok() else {
        // 命中 booru 但爬取失败（如网络不通）：仍写入 source/source_url（按 ext_urls 域名），
        // 并标记不可溯源避免每次重试同一命中；状态变"已溯源"，用户可手动打开源链接。
        // BUG1 修复：此前只记缓存不写状态，手动溯源后界面毫无变化。
        db.put_sauce_cache(&img.md5, result.similarity, None, None, None)?;
        let fallback_source = booru::extract_booru(&result.ext_urls)
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let fallback_url = result.ext_urls.first().cloned().unwrap_or_default();
        db.set_image_source(image_id, &fallback_source, if fallback_url.is_empty() { None } else { Some(&fallback_url) })?;
        db.set_no_auto_sauce(image_id, true)?;
        return Ok(None);
    };

    Ok(Some(SauceHit {
        source: source.to_string(),
        source_url,
        tags,
        similarity: result.similarity,
    }))
}

/// 单张图遇到 SauceNAO 限流时的最大重试次数（超过则计为失败，避免无限卡住整批）。
const MAX_RATE_RETRIES: u32 = 5;

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
    exits: Option<&crate::ExitPool>,
    infer: &InferClient,
    library_dir: &Path,
    min_sim: f64,
    image_ids: Option<Vec<i64>>,
    job_id: Option<i64>,
    force_sauce: bool,
) -> Result<SauceProgress, TaggerError> {
    let raw_ids = match image_ids {
        Some(ids) => ids,
        None => db.untagged_active_images(10000)?,
    };
    let ids = if raw_ids.len() > 1 {
        // 批量（>1 张）过滤无需处理的图；单张（详情页手动）保留强制语义
        let filtered = filter_eligible(db, "sauce", &raw_ids, force_sauce)?;
        // 记录跳过原因（任务日志可见）
        let skipped = raw_ids.len() - filtered.len();
        if skipped > 0 && job_id.is_some() {
            let _ = db.add_log(
                "warn",
                "sauce",
                &format!("批量溯源过滤：请求 {} 张，跳过 {skipped} 张（AI 生成/不可溯源/已溯源/GIF）", raw_ids.len()),
            );
        }
        filtered
    } else {
        raw_ids
    };
    if ids.is_empty() {
        if let Some(_jid) = job_id {
            let _ = db.add_log("warn", "sauce", "批量溯源：所有图片均被跳过，无任务执行");
        }
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

    // worker 数 = 可用 key 数 × 出口数（至少 1）。
    //
    // 为什么乘出口数：SauceNAO 的「4 次 / 30 秒」是**按出口 IP** 计的，
    // 每个出口有独立额度；若 worker 数只按 key 数算，
    // 多出口就无法并行，实际吞吐仍被单出口的 4 次/30 秒 卡住
    // （用户反馈的"一次还是只有 4 个"正是此因）。
    // 注：出口池的 acquire() 本身会按"有空闲优先"分配，
    // 因此多起的 worker 会自然分散到不同出口，不会同时挤压同一个出口。
    let key_count = pool.len().await.max(1);
    let exit_count = match exits {
        Some(e) if e.enabled() => e.count().await.max(1),
        _ => 1,
    };
    // 上限保护：避免 key 或出口填得多时起过多任务（每个都要上传图片）
    let worker_count = (key_count * exit_count).min(12);
    tracing::info!(
        key_count,
        exit_count,
        worker_count,
        "SauceNAO 溯源并发度（key × 出口）"
    );
    // SauceNaoClient / InferClient 无 Sync 要求但需 'static：Arc 包装
    let sauce = std::sync::Arc::new(sauce.clone());
    let infer = std::sync::Arc::new(infer.clone());
    // 出口池是共享只读状态（内部用 Arc<Mutex>），克隆进各 worker 即可
    let exits_shared: Option<crate::ExitPool> = exits.cloned();
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let queue = queue.clone();
        let progress = progress.clone();
        let db = db.clone();
        let sauce = sauce.clone();
        let pool = pool.clone();
        let exits = exits_shared.clone();
        let infer = infer.clone();
        let library_dir = library_dir.to_path_buf();
        handles.push(tokio::spawn(async move {
            // 限流重试：保存待重试的图片与已重试次数（Some 时优先继续处理该图）
            let mut current: Option<(i64, u32)> = None;
            // 带标签的循环：acquire 被中断时要跳出整个 worker，而不是只跳出内层
            'worker: loop {
                // 中断检查：任务被取消则停止（每轮处理前查一次 DB）
                if let Some(jid) = job_id {
                    if let Ok(Some(job)) = db.get_job(jid) {
                        if job.status == "cancelled" {
                            break;
                        }
                    }
                }
                // 取下一张图（限流重试的图优先）
                let (image_id, attempt) = match current.take() {
                    Some(v) => v,
                    None => {
                        let next = {
                            let mut q = queue.lock().unwrap();
                            q.pop_front()
                        };
                        match next {
                            Some(id) => (id, 0u32),
                            None => break,
                        }
                    }
                };

                // acquire 会等待可用 key（含全局窗口 / 冷却结束后放行）。
                // **中断修复**：acquire 可能阻塞很久（等全局限流冷却 / 出口冷却），
                // 期间不检查中断会让"中断"按钮看起来无效（任务长时间仍是 running）。
                // 这里用 select! 让等待与中断轮询并行，最多每 1 秒检查一次任务状态。
                let (api_key, key_idx) = loop {
                    let cancelled = matches!(
                        job_id,
                        Some(jid) if db
                            .get_job(jid)
                            .ok()
                            .flatten()
                            .map(|j| j.status == "cancelled")
                            .unwrap_or(false)
                    );
                    if cancelled {
                        break 'worker;
                    }
                    match tokio::time::timeout(Duration::from_secs(1), pool.acquire()).await {
                        Ok(v) => break v,
                        Err(_) => continue, // 1 秒超时 → 重新检查中断后继续等
                    }
                };
                match sauce_one(
                    &db,
                    &sauce,
                    &pool,
                    exits.as_ref(),
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
                    Err(TaggerError::RateLimited(secs)) => {
                        // 限流：同一张图等待后重试（全局窗口+冷却已由 pool 接管），
                        // 不丢弃该图，避免一次限流白扔整批。
                        if attempt < MAX_RATE_RETRIES {
                            let wait = secs.clamp(1, 120) as u64;
                            warn!(
                                image_id,
                                retry_secs = wait,
                                attempt = attempt + 1,
                                "溯源限流，等待后重试同一张图"
                            );
                            let _ = db.add_log(
                                "warn",
                                "sauce",
                                &format!(
                                    "SauceNAO 限流：{wait} 秒后重试图片 #{image_id}（第 {} 次）",
                                    attempt + 1
                                ),
                            );
                            current = Some((image_id, attempt + 1));
                            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                            continue;
                        }
                        warn!(image_id, "溯源限流重试次数耗尽，计为失败");
                        let _ = db.add_log(
                            "error",
                            "sauce",
                            &format!("图片 #{image_id} 溯源失败：SauceNAO 限流重试 {MAX_RATE_RETRIES} 次仍未成功"),
                        );
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
                        let rel = db
                            .get_image_by_id(image_id)
                            .ok()
                            .flatten()
                            .map(|i| i.rel_path)
                            .unwrap_or_default();
                        let _ = db.add_log("error", "sauce", &format!("图片 #{image_id}（{rel}）溯源失败：{e}"));
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

/// 把路径转为绝对路径（用于传给推理服务等跨进程消费方）。
/// 相对路径基于后端 cwd 解析；文件不存在时 canonicalize 失败则回退 current_dir join。
pub(crate) fn to_absolute_path(path: &Path) -> Result<String, TaggerError> {
    use std::path::PathBuf;
    let abs: PathBuf = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    // canonicalize 解析 .. / 符号链接，但要求文件存在；失败则用未解析的绝对路径
    Ok(abs
        .canonicalize()
        .unwrap_or(abs)
        .to_string_lossy()
        .into_owned())
}
