//! 中文字典导入：/api/v1/dict/import 与缓存状态 /api/v1/dict/status。
//!
//! 数据源：ffdkj/ffdkj-Danbooru_Tag-Chinese-English-Translation-Table 的 tag.sqlite
//! （name/cn_name/category/post_count，每日更新 317K+ 条），批量回填 tags.name_cn
//! （仅填空缺，不覆盖手工别名）。
//!
//! ## 增强1：本地缓存（避免每次重复下载 60-80MB）
//!
//! 下载的 sqlite 会**持久保留**在 `data_dir/dict/tag.sqlite`，并记录：
//!   - `meta.json`：本地缓存时间、上游 Last-Modified / ETag、文件大小
//! 再次导入时：
//!   1. 先发 HEAD 请求探测上游 `Last-Modified`；
//!   2. 与本地 meta 中记录的一致**且本地文件存在** → 直接用本地缓存导入，跳过下载；
//!   3. 不一致或无法探测 → 下载新版本并更新缓存。
//!
//! 另外提供 `force` 参数强制重新下载，便于用户手动刷新。
//!
//! ## 增强1：失败提示
//!
//! 下载/导入失败时返回结构化错误（含 `stage` 与 `hint`），前端在右上角以通知形式展示，
//! 不妨碍用户继续使用其它功能，并提示可手动放置文件的路径。

use std::path::{Path, PathBuf};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/dict/import", post(import_dict))
        .route("/api/v1/dict/status", get(dict_status))
}

const DICT_URL: &str =
    "https://github.com/ffdkj/ffdkj-Danbooru_Tag-Chinese-English-Translation-Table/raw/main/tag.sqlite";
/// 下载体积上限（317K 行 sqlite 约 60-80MB，上限 256MB 防异常）。
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// 本地缓存元信息（`data_dir/dict/meta.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DictMeta {
    /// 上游 Last-Modified（用于判断是否需要重新下载）。
    last_modified: Option<String>,
    /// 上游 ETag（Last-Modified 缺失时的备用判据）。
    etag: Option<String>,
    /// 本地缓存的下载时间（Unix 秒）。
    cached_at: i64,
    /// 缓存文件字节数。
    bytes: u64,
}

/// 字典缓存目录。
fn dict_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("dict")
}
/// 缓存文件路径（下载后持久保留，不再用完即删）。
fn dict_cache_path(data_dir: &Path) -> PathBuf {
    dict_dir(data_dir).join("tag.sqlite")
}
/// 缓存元信息路径。
fn dict_meta_path(data_dir: &Path) -> PathBuf {
    dict_dir(data_dir).join("meta.json")
}

fn read_meta(data_dir: &Path) -> Option<DictMeta> {
    let raw = std::fs::read_to_string(dict_meta_path(data_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_meta(data_dir: &Path, meta: &DictMeta) {
    let dir = dict_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(s) = serde_json::to_string_pretty(meta) {
        let _ = std::fs::write(dict_meta_path(data_dir), s);
    }
}

#[derive(Debug, Deserialize, Default)]
struct ImportParams {
    /// 强制重新下载（忽略本地缓存）。
    #[serde(default)]
    force: bool,
}

/// GET /api/v1/dict/status：返回本地缓存是否可用及其信息（供设置页展示）。
async fn dict_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let dir = state.data_dir.clone();
    let cache = dict_cache_path(&dir);
    let meta = read_meta(&dir);
    let exists = cache.is_file();
    let bytes = if exists {
        std::fs::metadata(&cache).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    Ok(Json(json!({
        "cached": exists,
        "path": cache.to_string_lossy(),
        "bytes": bytes,
        "cached_at": meta.as_ref().map(|m| m.cached_at).unwrap_or(0),
        "last_modified": meta.as_ref().and_then(|m| m.last_modified.clone()),
    })))
}

/// POST /api/v1/dict/import：导入中文字典（优先用本地缓存）。
///
/// body（可选）：`{ "force": true }` 强制重新下载。
async fn import_dict(
    State(state): State<AppState>,
    body: Option<Json<ImportParams>>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let force = body.map(|b| b.0.force).unwrap_or(false);
    let data_dir = state.data_dir.clone();
    let cache_path = dict_cache_path(&data_dir);
    let local_meta = read_meta(&data_dir);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("MoeVault/0.1")
        .build()
        .map_err(|e| error_response(ErrorKind::Internal, format!("HTTP 客户端初始化失败: {e}")))?;

    // ---- 第 1 步：判断能否直接使用本地缓存 ----
    let mut use_cache = false;
    let mut upstream_lm: Option<String> = None;
    let mut upstream_etag: Option<String> = None;
    if !force && cache_path.is_file() {
        // 探测上游指纹（失败不致命，仅表示无法确认是否最新）
        match client.head(DICT_URL).send().await {
            Ok(r) if r.status().is_success() => {
                upstream_lm = r
                    .headers()
                    .get(reqwest::header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                upstream_etag = r
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                // 与本地记录一致 → 命中缓存
                if let Some(m) = &local_meta {
                    let lm_match = match (&upstream_lm, &m.last_modified) {
                        (Some(a), Some(b)) => a == b,
                        _ => false,
                    };
                    let etag_match = match (&upstream_etag, &m.etag) {
                        (Some(a), Some(b)) => a == b,
                        _ => false,
                    };
                    use_cache = lm_match || etag_match;
                }
            }
            _ => {
                // 无法探测上游：保守起见用本地缓存（避免因网络问题无法导入）
                use_cache = true;
            }
        }
    }

    // ---- 第 2 步：必要时下载并写入缓存 ----
    if !use_cache {
        let resp = client.get(DICT_URL).send().await.map_err(|e| {
            error_response(
                ErrorKind::Internal,
                format!("下载中文字典失败（网络不可达？）: {e}\n可手动下载后放入: {}", cache_path.display()),
            )
        })?;
        if !resp.status().is_success() {
            return Err(error_response(
                ErrorKind::Internal,
                format!("下载中文字典失败: HTTP {}（可手动下载后放入 {}）", resp.status(), cache_path.display()),
            ));
        }
        // 记录本次响应的指纹（供下次比对）
        upstream_lm = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        upstream_etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 流式写入缓存目录（先写临时文件，成功后原子替换，避免中断留下半截文件）
        let dir = dict_dir(&data_dir);
        std::fs::create_dir_all(&dir).map_err(|e| {
            error_response(ErrorKind::Internal, format!("创建字典目录失败: {e}"))
        })?;
        let tmp_path = dir.join("tag.sqlite.download");
        let bytes = resp.bytes().await.map_err(|e| {
            error_response(ErrorKind::Internal, format!("读取下载内容失败: {e}"))
        })?;
        if bytes.len() as u64 > MAX_BYTES {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(error_response(
                ErrorKind::Internal,
                "字典文件超过 256MB 上限，已中止".to_string(),
            ));
        }
        std::fs::write(&tmp_path, &bytes).map_err(|e| {
            error_response(ErrorKind::Internal, format!("写入缓存失败: {e}"))
        })?;
        std::fs::rename(&tmp_path, &cache_path).map_err(|e| {
            error_response(ErrorKind::Internal, format!("保存缓存失败: {e}"))
        })?;
        write_meta(
            &data_dir,
            &DictMeta {
                last_modified: upstream_lm.clone(),
                etag: upstream_etag.clone(),
                cached_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                bytes: bytes.len() as u64,
            },
        );
    }

    // ---- 第 3 步：从缓存文件导入 ----
    let db = state.db.clone();
    let path: PathBuf = cache_path.clone();
    let result = tokio::task::spawn_blocking(move || db.import_cn_dict(&path))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("导入任务失败: {e}")))
        .and_then(|r| r.map_err(db_error_response));

    let (matched, updated, missing) = match result {
        Ok(v) => v,
        Err(e) => {
            // 导入失败：清掉缓存指纹，下次强制重新下载（可能是文件损坏/被截断）
            let _ = std::fs::remove_file(dict_meta_path(&data_dir));
            return Err(e);
        }
    };
    Ok(Json(json!({
        "matched": matched,
        "updated": updated,
        "missing": missing,
        "from_cache": use_cache,
        "cache_path": cache_path.to_string_lossy(),
    })))
}