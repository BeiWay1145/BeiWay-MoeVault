//! 路由模块组织。

pub mod aesthetic;
pub mod dedup;
pub mod health;
pub mod images;
pub mod import;
pub mod logs;
pub mod settings;
pub mod tagging;
pub mod tasks;
pub mod trash;
pub mod ws;

use axum::Json;
use moevault_core::ErrorKind;
use serde_json::{json, Value};

/// 统一错误响应体（docs/TECH_DETAILS.md 第 2 节约定）。
pub fn error_response(kind: ErrorKind, message: impl Into<String>) -> (axum::http::StatusCode, Json<Value>) {
    (
        axum::http::StatusCode::from_u16(kind.status_code()).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({
            "error": { "code": kind.as_str(), "message": message.into() }
        })),
    )
}

/// 把任意 AppError 映射为 HTTP 错误响应。
pub fn app_error_response(e: moevault_core::AppError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(e.kind, e.message)
}

/// 把 DbError 映射为 HTTP 错误响应。
pub fn db_error_response(e: moevault_db::DbError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(moevault_core::ErrorKind::Db, e.to_string())
}

/// 把 tokio JoinError 映射为 HTTP 错误响应（内部错误）。
pub fn join_error_response(e: tokio::task::JoinError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(moevault_core::ErrorKind::Internal, format!("后台任务失败: {e}"))
}
