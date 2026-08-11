//! 应用日志接口（设置页日志追踪器）：GET/POST/DELETE /api/v1/logs。
//!
//! - GET /api/v1/logs：查询日志（倒序分页，before_id 游标）
//! - POST /api/v1/logs：前端上报操作日志（批量动作/导入/设置修改等）
//! - DELETE /api/v1/logs：清空日志

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/logs", get(list_logs).post(add_log).delete(clear_logs))
}

#[derive(Debug, Deserialize, Default)]
pub struct LogQuery {
    pub limit: Option<i64>,
    /// 游标：返回 id 小于此值的更早日志。
    pub before_id: Option<i64>,
}

async fn list_logs(
    State(state): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let logs = tokio::task::spawn_blocking(move || db.list_logs(limit, q.before_id))
        .await
        .map_err(|e| error_response(moevault_core::ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    let items: Vec<Value> = logs
        .iter()
        .map(|l| {
            json!({
                "id": l.id,
                "level": l.level,
                "category": l.category,
                "message": l.message,
                "created_at": l.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "count": items.len() })))
}

#[derive(Debug, Deserialize)]
pub struct AddLogRequest {
    /// info / warn / error（默认 info）。
    pub level: Option<String>,
    /// task / sauce / tag / aesthetic / frontend / import / system。
    pub category: Option<String>,
    pub message: String,
}

async fn add_log(
    State(state): State<AppState>,
    Json(req): Json<AddLogRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let level = req.level.clone().unwrap_or_else(|| "info".to_string());
    let category = req.category.clone().unwrap_or_else(|| "system".to_string());
    let db = state.db.clone();
    let msg = req.message.trim().to_string();
    if msg.is_empty() {
        return Err(error_response(moevault_core::ErrorKind::InvalidInput, "message 不能为空"));
    }
    tokio::task::spawn_blocking(move || db.add_log(&level, &category, &msg))
        .await
        .map_err(|e| error_response(moevault_core::ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true })))
}

async fn clear_logs(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let n = tokio::task::spawn_blocking(move || db.clear_logs())
        .await
        .map_err(|e| error_response(moevault_core::ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "cleared": n })))
}
