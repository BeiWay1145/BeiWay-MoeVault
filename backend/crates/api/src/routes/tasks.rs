//! 任务接口：GET /api/v1/tasks（列表）、GET /api/v1/tasks/{id}（详情）。
//!
//! 打标/美学/溯源等耗时操作统一记录到 jobs 表（持久化），
//! 前端轮询本接口获取进度与历史。

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tasks", get(list_tasks))
        .route("/api/v1/tasks/{id}", get(get_task))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListTasksParams {
    /// 返回条数，默认 50，最大 200。
    pub limit: Option<i64>,
}

/// 任务类型 → 中文名。
pub fn task_type_label(ty: &str) -> &'static str {
    match ty {
        "tag" => "打标",
        "aesthetic" => "美学评分",
        "sauce" => "SauceNAO 溯源",
        "ai-detect" => "AI 生成检测",
        "import" => "导入",
        _ => "其他",
    }
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(params): Query<ListTasksParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let db = state.db.clone();
    let jobs = tokio::task::spawn_blocking(move || db.list_jobs(limit))
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    let items: Vec<Value> = jobs
        .iter()
        .map(|j| {
            json!({
                "id": j.id,
                "type": j.ty,
                "type_label": task_type_label(&j.ty),
                "status": j.status,
                "total": j.total,
                "done": j.done,
                "failed": j.failed,
                "error": j.error,
                "created_at": j.created_at,
                "updated_at": j.updated_at,
                "finished_at": j.finished_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": items.len() })))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let job = tokio::task::spawn_blocking(move || db.get_job(id))
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    match job {
        Some(j) => Ok(Json(json!({
            "id": j.id,
            "type": j.ty,
            "type_label": task_type_label(&j.ty),
            "status": j.status,
            "total": j.total,
            "done": j.done,
            "failed": j.failed,
            "error": j.error,
            "payload": j.payload,
            "created_at": j.created_at,
            "updated_at": j.updated_at,
            "finished_at": j.finished_at,
        }))),
        None => Err(error_response(ErrorKind::NotFound, format!("任务 {id} 不存在"))),
    }
}
