//! 美学评分接口：/api/v1/aesthetic/*。

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use moevault_tagger::InferClient;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::error_response;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/aesthetic/run", post(run_aesthetic))
        .route("/api/v1/aesthetic/stats", get(aesthetic_stats))
        .route("/api/v1/images/{id}/rescore", post(rescore_image))
}

#[derive(Debug, Deserialize)]
pub struct RunAestheticRequest {
    /// 指定 image_ids 强制重评分；None = 全部未评分 active 图。
    pub force_ids: Option<Vec<i64>>,
}

async fn run_aesthetic(
    State(state): State<AppState>,
    Json(req): Json<RunAestheticRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let force_ids = req.force_ids;
    let infer = Arc::new(InferClient::new(state.infer_base_url.clone()));
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let db = st.db.clone();
        let result = moevault_tagger::run_aesthetic_pipeline(
            &db,
            &infer,
            &library_dir,
            force_ids,
        )
        .await;
        let event = match result {
            Ok(progress) => json!({
                "type": "aesthetic.done",
                "payload": {
                    "done": progress.done,
                    "failed": progress.failed,
                    "total": progress.total,
                },
            }),
            Err(e) => json!({
                "type": "aesthetic.failed",
                "payload": { "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true })))
}

async fn aesthetic_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let (unscored, total, scored) = tokio::task::spawn_blocking(move || {
        let unscored = db.unscored_active_images(100000).unwrap_or_default().len();
        let total = db.count_images("active").unwrap_or(0);
        let scored = total - unscored as i64;
        (unscored, total, scored)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?;
    Ok(Json(json!({
        "active_images": total,
        "scored": scored,
        "unscored": unscored,
    })))
}

async fn rescore_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(_req): Json<RunAestheticRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let infer = Arc::new(InferClient::new(state.infer_base_url.clone()));
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let _ = moevault_tagger::run_aesthetic_pipeline(
            &st.db,
            &infer,
            &library_dir,
            Some(vec![id]),
        )
        .await;
    });

    Ok(Json(json!({ "started": true, "image_id": id })))
}
