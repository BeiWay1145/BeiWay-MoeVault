//! 增强3：溯源原图智能替换。
//!
//! 批量溯源（带 auto_replace 标志）完成后，对每张成功溯源的图：
//! 1. 解析网络图信息（大小）→ 与本地图比较：网络图 ≥ 2× 本地图才继续
//! 2. 下载网络图到临时文件 → 与本地图做严格 pHash 查重（hamming ≤ 4 才算同一张图）
//! 3. 相似 → 替换库内文件（旧文件移入回收站目录，条目保持 active）
//!    不相似 → 写入 replace_pending（查重界面人工确认，防止误换差分图）
//!
//! 单张溯源（详情页）由前端比较大小后弹提示 + 一键替换（走 replace-from-url）。

use std::path::PathBuf;

use axum::{extract::{Path as AxumPath, State}, routing::{delete, get, post}, Json, Router};
use moevault_core::ErrorKind;
use serde_json::{json, Value};

use crate::state::AppState;
use crate::routes::images::{parse_remote_source_info, strip_json_suffix};

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/replace-pending", get(list_pending))
        .route("/api/v1/replace-pending/{id}/confirm", post(confirm_pending))
        .route("/api/v1/replace-pending/{id}", delete(ignore_pending))
}

/// 严格查重阈值：pHash hamming ≤ 4 视为同一张图（差分图通常 ≥ 6）。
pub const STRICT_HAMMING: u32 = 4;
/// 大小差异阈值：网络图 ≥ 2× 本地图（文件字节数）。
pub const SIZE_RATIO: f64 = 2.0;

/// 批量自动替换结果统计。
#[derive(Debug, Default, Clone)]
pub struct AutoReplaceStats {
    pub skipped: i64,
    pub replaced: i64,
    pub pending: i64,
    pub failed: i64,
}

/// 对指定图片批量执行智能替换检查（溯源任务完成后调用）。
pub async fn run_auto_replace_for_images(state: &AppState, ids: Vec<i64>) -> AutoReplaceStats {
    let st = state.clone();
    let runtime = tokio::runtime::Handle::current();
    let handle = tokio::task::spawn_blocking(move || {
        let mut stats = AutoReplaceStats::default();
        for id in ids {
            match check_and_replace_sync(&st, id, &runtime) {
                Ok(outcome) => match outcome {
                    ReplaceOutcome::Replaced => stats.replaced += 1,
                    ReplaceOutcome::Pending { .. } => stats.pending += 1,
                    ReplaceOutcome::Skipped(reason) => {
                        stats.skipped += 1;
                        tracing::info!(image_id = id, reason = %reason, "智能替换：跳过");
                    }
                },
                Err(e) => {
                    stats.failed += 1;
                    tracing::warn!(image_id = id, error = %e, "智能替换：失败");
                    let _ = st.db.add_log("warn", "sauce", &format!("智能替换 图片 #{id} 失败：{e}"));
                }
            }
        }
        stats
    });
    handle.await.unwrap_or_default()
}

#[derive(Debug)]
pub enum ReplaceOutcome {
    Replaced,
    Pending { pending_id: i64 },
    Skipped(String),
}

/// 单张图的完整检查+替换流程（阻塞；调用方放入 spawn_blocking 并传入 runtime handle）。
pub fn check_and_replace_sync(st: &AppState, image_id: i64, runtime: &tokio::runtime::Handle) -> Result<ReplaceOutcome, String> {
    let db = &st.db;
    let img = db
        .get_image_by_id(image_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("图片 {image_id} 不存在"))?;
    // 条件：active + 已溯源（有 source_url）
    if img.status != moevault_core::models::STATUS_ACTIVE {
        return Ok(ReplaceOutcome::Skipped(format!("状态 {} 非 active", img.status)));
    }
    let Some(source_url) = img.source_url.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(ReplaceOutcome::Skipped("未溯源（无 source_url）".into()));
    };

    // 1. 网络图信息（大小/直链）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("MoeVault/0.1")
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {e}"))?;
    let page_url = strip_json_suffix(source_url);
    // parse_remote_source_info 是 async —— 在阻塞线程里用 handle.block_on
    let info = runtime.block_on(parse_remote_source_info(&client, &page_url));
    let net_size = info.get("size_bytes").and_then(|v| v.as_i64()).unwrap_or(0);
    let file_url = info
        .get("file_url")
        .and_then(|v| v.as_str())
        .map(String::from);
    let Some(file_url) = file_url.filter(|u| !u.is_empty()) else {
        return Ok(ReplaceOutcome::Skipped("无法解析网络图直链".into()));
    };
    if net_size < (SIZE_RATIO * img.size_bytes as f64) as i64 {
        return Ok(ReplaceOutcome::Skipped(format!(
            "大小差异不足（网络 {net_size} B vs 本地 {} B）",
            img.size_bytes
        )));
    }

    // 2. 下载网络图到临时文件
    let tmp_dir = st.data_dir.join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let ext = file_url
        .rsplit('.')
        .next()
        .map(|e| e.split('?').next().unwrap_or("png").to_lowercase())
        .unwrap_or_else(|| "png".into());
    let tmp_path: PathBuf = tmp_dir.join(format!("replace_{}_{}.{}", image_id, chrono_now_secs(), ext));
    let bytes = runtime.block_on(async {
        let resp = client.get(&file_url).send().await.map_err(|e| format!("下载网络图失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("下载网络图失败: HTTP {}", resp.status()));
        }
        resp.bytes().await.map_err(|e| format!("读取网络图失败: {e}")).map(|b| b.to_vec())
    })?;
    std::fs::write(&tmp_path, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;

    // 3. 严格 pHash 查重
    let local_path = st.library_dir().join(&img.rel_path);
    let (net_ph, local_ph) = match (image::open(&tmp_path), image::open(&local_path)) {
        (Ok(a), Ok(b)) => (
            moevault_ingest::phash::phash(&a),
            moevault_ingest::phash::phash(&b),
        ),
        (Err(e), _) => return Err(format!("临时图解码失败: {e}")),
        (_, Err(e)) => return Err(format!("本地图解码失败: {e}")),
    };
    let distance = moevault_ingest::phash::hamming(net_ph, local_ph);
    tracing::info!(image_id, distance, net_size, local = img.size_bytes, "智能替换：pHash 比对");

    if distance <= STRICT_HAMMING {
        // 相似 → 替换（旧文件 → 回收站目录）
        do_replace_from_bytes(st, image_id, &bytes)?;
        let _ = std::fs::remove_file(&tmp_path);
        let _ = db.add_log("info", "sauce", &format!("智能替换：图片 #{image_id} 已替换为网络原图（pHash 距离 {distance}）"));
        Ok(ReplaceOutcome::Replaced)
    } else {
        // 不相似 → 待人工确认
        let pending_id = db
            .add_replace_pending(
                image_id,
                &tmp_path.to_string_lossy(),
                net_size,
                img.size_bytes,
                info.get("width").and_then(|v| v.as_i64()),
                info.get("height").and_then(|v| v.as_i64()),
                Some(source_url),
            )
            .map_err(|e| e.to_string())?;
        let _ = db.add_log(
            "warn",
            "sauce",
            &format!("智能替换：图片 #{image_id} 网络图与本地图差异较大（pHash 距离 {distance}），已加入待确认列表"),
        );
        Ok(ReplaceOutcome::Pending { pending_id })
    }
}

/// 用字节内容替换库内图片（写新 md5 路径 + 旧文件移回收站 + 缩略图 + 记录更新）。
/// 与 replace-from-url 相同语义（增强3：旧文件移入回收站目录）。
pub fn do_replace_from_bytes(
    st: &AppState,
    image_id: i64,
    bytes: &[u8],
) -> Result<Value, String> {
    let db = &st.db;
    let img = db
        .get_image_by_id(image_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("图片 {image_id} 不存在"))?;
    let old_path = st.library_dir().join(&img.rel_path);
    let format_guess =
        image::guess_format(bytes).map_err(|e| format!("下载内容不是有效图片: {e}"))?;
    let ext = match format_guess {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::Bmp => "bmp",
        _ => return Err("不支持的图片格式".into()),
    };
    let decoded = image::load_from_memory(bytes).map_err(|e| format!("图片解码失败: {e}"))?;
    let (w, h) = (decoded.width(), decoded.height());
    use md5::Digest;
    let digest = md5::Md5::digest(bytes);
    let md5_hex = format!("{digest:x}");
    let prefix = &md5_hex[..md5_hex.len().min(2)];
    let new_rel = format!("{prefix}/{md5_hex}.{ext}");
    let new_path = st.library_dir().join(&new_rel);
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&new_path, bytes).map_err(|e| format!("写入新文件失败: {e}"))?;
    // 旧文件 → 回收站目录
    if new_path != old_path {
        let recycle_dst = st.recycle_dir().join(&img.rel_path);
        if let Some(parent) = recycle_dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(&old_path, &recycle_dst).is_err() {
            let _ = std::fs::copy(&old_path, &recycle_dst).and_then(|_| std::fs::remove_file(&old_path));
        }
    }
    // 缩略图
    let thumb_rel = format!("{prefix}/{md5_hex}.webp");
    let thumb_path = st.thumbs_dir().join(&thumb_rel);
    moevault_ingest::importer::generate_thumbnail(&new_path, &thumb_path);
    // 记录
    db.replace_image_file(image_id, &md5_hex, &new_rel, w, h, ext, bytes.len() as i64)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "ok": true,
        "md5": md5_hex,
        "rel_path": new_rel,
        "width": w,
        "height": h,
        "size_bytes": bytes.len(),
        "thumb_rel": thumb_rel,
    }))
}

fn chrono_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------- 待确认替换路由 ----------

/// GET /api/v1/replace-pending：待确认替换列表。
async fn list_pending(State(state): State<AppState>) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let items = tokio::task::spawn_blocking(move || db.list_replace_pending())
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "items": items, "count": items.len() })))
}

/// POST /api/v1/replace-pending/{id}/confirm：人工确认替换。
async fn confirm_pending(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let pending = st
            .db
            .get_replace_pending(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("记录 {id} 不存在")))?;
        let bytes = std::fs::read(&pending.temp_path)
            .map_err(|e| error_response(ErrorKind::Internal, format!("读取临时文件失败: {e}")))?;
        let result = do_replace_from_bytes(&st, pending.image_id, &bytes)
            .map_err(|e| error_response(ErrorKind::Internal, e))?;
        let _ = st.db.remove_replace_pending(id);
        let _ = std::fs::remove_file(&pending.temp_path);
        let _ = st.db.add_log("info", "sauce", &format!("人工确认：图片 #{} 已替换为网络原图", pending.image_id));
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(result)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(result))
}

/// DELETE /api/v1/replace-pending/{id}：忽略（删除记录 + 临时文件）。
async fn ignore_pending(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let pending = tokio::task::spawn_blocking(move || db.get_replace_pending(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    if let Some(p) = pending {
        let _ = std::fs::remove_file(&p.temp_path);
        let db2 = state.db.clone();
        let _ = tokio::task::spawn_blocking(move || db2.remove_replace_pending(id)).await;
    }
    Ok(Json(json!({ "ok": true })))
}
