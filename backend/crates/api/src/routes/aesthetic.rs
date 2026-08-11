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

    // 创建任务记录
    let payload = force_ids
        .as_ref()
        .map(|ids| serde_json::json!({ "image_ids": ids }).to_string());
    let job_id = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.create_job("aesthetic", payload.as_deref())
    })
    .await
    .map_err(super::join_error_response)?
    .map_err(super::db_error_response)?;

    let infer = Arc::new(InferClient::new(state.infer_base_url.clone()));
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let db = st.db.clone();
        let _ = db.start_job(job_id, force_ids.as_ref().map_or(0, |v| v.len() as i64));
        let _ = db.add_log("info", "aesthetic", &format!("美学任务 #{job_id} 启动（{} 张）", force_ids.as_ref().map_or(0, |v| v.len())));
        let result = moevault_tagger::run_aesthetic_pipeline(
            &db,
            &infer,
            &library_dir,
            force_ids,
            Some(job_id),
        )
        .await;
        let (status, done, failed, error) = match &result {
            Ok(progress) => ("done", progress.done as i64, progress.failed as i64, None),
            Err(e) => ("failed", 0, 0, Some(e.to_string())),
        };
        let _ = db.update_job(job_id, status, done, failed, error.as_deref());
        let _ = db.add_log(
            if status == "done" { "info" } else { "error" },
            "aesthetic",
            &format!("美学任务 #{job_id} 完成：成功 {done} 张，失败 {failed} 张{}", error.as_deref().map(|e| format!("，错误：{e}")).unwrap_or_default()),
        );
        let event = match result {
            Ok(progress) => json!({
                "type": "task.done",
                "payload": { "job_id": job_id, "kind": "aesthetic", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": job_id, "kind": "aesthetic", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "job_id": job_id, "kind": "aesthetic" })))
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

    // 创建任务记录
    let payload = serde_json::json!({ "image_ids": [id] }).to_string();
    let job_id = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.create_job("aesthetic", Some(&payload))
    })
    .await
    .map_err(super::join_error_response)?
    .map_err(super::db_error_response)?;

    let infer = Arc::new(InferClient::new(state.infer_base_url.clone()));
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let _ = st.db.start_job(job_id, 1);
        let _ = st.db.add_log("info", "aesthetic", &format!("美学任务 #{job_id} 启动（图片 {id} 单张）"));
        let result = moevault_tagger::run_aesthetic_pipeline(
            &st.db,
            &infer,
            &library_dir,
            Some(vec![id]),
            Some(job_id),
        )
        .await;
        let (status, done, failed, error) = match &result {
            Ok(progress) => ("done", progress.done as i64, progress.failed as i64, None),
            Err(e) => ("failed", 0, 0, Some(e.to_string())),
        };
        let _ = st.db.update_job(job_id, status, done, failed, error.as_deref());
        let event = match result {
            Ok(progress) => json!({
                "type": "task.done",
                "payload": { "job_id": job_id, "kind": "aesthetic", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": job_id, "kind": "aesthetic", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "job_id": job_id, "image_id": id, "kind": "aesthetic" })))
}
