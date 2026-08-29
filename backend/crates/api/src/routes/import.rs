//! 导入接口：POST /api/v1/import、GET /api/v1/import/batches[/:id]。
//!
//! 对应 docs/TECH_DETAILS.md 第 2.2 节。支持 move（移动进库）/ copy（复制进库）两种模式。

use std::path::PathBuf;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/import", post(create_import))
        .route("/api/v1/import/batches", get(list_batches))
        .route("/api/v1/import/batches/{id}", get(get_batch))
        // 主目录（按天 → 来源分组）板块
        .route("/api/v1/imports/tree", get(import_tree))
        .route("/api/v1/imports/dir", get(import_dir_images))
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// 源路径（文件或目录），支持相对/绝对。
    pub paths: Vec<String>,
    /// move（默认，移动进库）/ copy（复制进库，保留源文件）。
    pub mode: Option<String>,
}

async fn create_import(
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // 解析导入模式：move（默认）/ copy。
    // move = 移动进库（源文件移入库目录）；copy = 复制进库（保留源文件）。
    let mode = match req.mode.as_deref() {
        None | Some("move") => moevault_ingest::ImportMode::Move,
        Some("copy") => moevault_ingest::ImportMode::Copy,
        Some(m) => {
            return Err(error_response(
                ErrorKind::InvalidInput,
                format!("导入模式 {m} 无效（仅支持 move / copy）"),
            ))
        }
    };
    // 过滤空白/空字符串路径
    let paths: Vec<String> = req
        .paths
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if paths.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "paths 不能为空"));
    }
    // 校验路径存在（相对路径按当前工作目录解析）
    for p in &paths {
        let path = PathBuf::from(p);
        let exists = if path.is_absolute() {
            path.exists()
        } else {
            std::env::current_dir().map(|c| c.join(&path).exists()).unwrap_or(false)
        };
        if !exists {
            return Err(error_response(
                ErrorKind::InvalidInput,
                format!("路径不存在: {p}"),
            ));
        }
    }

    // 创建批次
    let source_summary = paths.join("; ");
    let db = state.db.clone();
    let batch_id = tokio::task::spawn_blocking(move || db.create_import_batch(&source_summary))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;

    // 后台执行导入（spawn_blocking 包真实 IO）
    let st = state.clone();
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    tokio::spawn(async move {
        let db = st.db.clone();
        let library = st.library_dir();
        let thumbs = st.thumbs_dir();
        let result = tokio::task::spawn_blocking(move || {
            moevault_ingest::run_import(&db, batch_id, path_bufs, &library, &thumbs, mode)
        })
        .await;

        let event = match result {
            Ok(Ok(progress)) => json!({
                "type": "batch.done",
                "payload": {
                    "batch_id": batch_id,
                    "done": progress.done,
                    "failed": progress.failed,
                    "duplicate": progress.duplicate,
                },
            }),
            Ok(Err(e)) => json!({
                "type": "batch.failed",
                "payload": { "batch_id": batch_id, "error": e.to_string() },
            }),
            Err(e) => json!({
                "type": "batch.failed",
                "payload": { "batch_id": batch_id, "error": format!("后台任务失败: {e}") },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "batch_id": batch_id })))
}

async fn list_batches(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let batches = tokio::task::spawn_blocking(move || db.list_import_batches(100))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "items": batches })))
}

async fn get_batch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let batch = tokio::task::spawn_blocking(move || db.get_import_batch(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    match batch {
        Some(b) => Ok(Json(json!(b))),
        None => Err(error_response(ErrorKind::NotFound, format!("批次 {id} 不存在"))),
    }
}

/// 主目录树：按天 → 来源分组。支持状态筛选（sauce/tag/ai），空组隐藏。
#[derive(Debug, Deserialize, Default)]
pub struct TreeQuery {
    pub sauce: Option<String>,
    pub tag: Option<String>,
    pub ai: Option<String>,
}

async fn import_tree(
    State(state): State<AppState>,
    Query(q): Query<TreeQuery>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let sauce = q.sauce.clone();
    let tag = q.tag.clone();
    let ai = q.ai.clone();
    let days = tokio::task::spawn_blocking(move || {
        db.import_tree(sauce.as_deref(), tag.as_deref(), ai.as_deref())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(db_error_response)?;
    // 组装 JSON：日期 → 来源组
    let items: Vec<Value> = days
        .into_iter()
        .map(|(day, dirs)| {
            let dir_list: Vec<Value> = dirs
                .into_iter()
                .map(|(dir, cnt)| {
                    json!({
                        "name": dir.clone().unwrap_or_else(|| "未知来源".to_string()),
                        "source_dir": dir,
                        "count": cnt,
                    })
                })
                .collect();
            json!({ "date": day, "dirs": dir_list })
        })
        .collect();
    Ok(Json(json!({ "days": items })))
}

/// 某来源组内图片（游标分页）。
#[derive(Debug, Deserialize, Default)]
pub struct DirImagesQuery {
    pub date: String,
    pub source_dir: Option<String>,
    pub sauce: Option<String>,
    pub tag: Option<String>,
    pub ai: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

async fn import_dir_images(
    State(state): State<AppState>,
    Query(q): Query<DirImagesQuery>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let limit = q.limit.unwrap_or(60).clamp(1, 200);
    let cursor = q.cursor.as_deref().and_then(|c| c.parse::<i64>().ok());
    let sauce = q.sauce.clone();
    let tag = q.tag.clone();
    let ai = q.ai.clone();
    let source_dir = q.source_dir.clone();
    let (items, next) = tokio::task::spawn_blocking(move || {
        db.import_dir_images(
            &q.date,
            source_dir.as_deref(),
            sauce.as_deref(),
            tag.as_deref(),
            ai.as_deref(),
            limit,
            cursor,
        )
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(db_error_response)?;
    Ok(Json(json!({
        "items": items,
        "next_cursor": next.map(|id| id.to_string()),
    })))
}
