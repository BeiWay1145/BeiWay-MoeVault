//! 导入接口：POST /api/v1/import、GET /api/v1/import/batches[/:id]。
//!
//! 对应 docs/TECH_DETAILS.md 第 2.2 节。骨架阶段仅支持"移动进库"模式。

use std::path::PathBuf;

use axum::{
    extract::{Path, State},
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
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// 源路径（文件或目录），支持相对/绝对。
    pub paths: Vec<String>,
    /// move（默认）/ copy（暂未实现）。
    pub mode: Option<String>,
}

async fn create_import(
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    if let Some(m) = &req.mode {
        if m != "move" {
            return Err(error_response(
                ErrorKind::InvalidInput,
                format!("导入模式 {m} 暂未支持（M2 仅支持 move）"),
            ));
        }
    }
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
            moevault_ingest::run_import(&db, batch_id, path_bufs, &library, &thumbs)
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
