//! 图库接口：GET /api/v1/images、GET /api/v1/stats。
//!
//! 骨架版实现基础游标分页 + status 过滤；完整筛选（标签/日期/质量/来源）
//! 按 docs/TECH_DETAILS.md 第 2.1 节在 M2+ 补全。

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use moevault_core::models::{Page, Stats, STATUS_ACTIVE};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/images", get(list_images))
        .route("/api/v1/stats", get(stats))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// active / recycled，默认 active。
    pub status: Option<String>,
    /// 每页条数，默认 100，最大 500。
    pub limit: Option<i64>,
    /// 游标：上一页返回的 next_cursor（骨架版为数字 id 字符串）。
    pub cursor: Option<String>,
}

async fn list_images(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let status = params.status.unwrap_or_else(|| STATUS_ACTIVE.to_string());
    if status != "active" && status != "recycled" {
        return Err(error_response(
            moevault_core::ErrorKind::InvalidInput,
            format!("status 仅支持 active/recycled，收到: {status}"),
        ));
    }
    let limit = params.limit.unwrap_or(100);
    let cursor_id = match &params.cursor {
        Some(c) => match c.parse::<i64>() {
            Ok(v) => Some(v),
            Err(_) => {
                return Err(error_response(
                    moevault_core::ErrorKind::InvalidInput,
                    format!("cursor 格式错误: {c}"),
                ))
            }
        },
        None => None,
    };

    let db = state.db.clone();
    let status_for_query = status.clone();
    let (items, next_id) = tokio::task::spawn_blocking(move || {
        db.list_images(&status_for_query, limit, cursor_id)
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    let total = {
        let db = state.db.clone();
        let status2 = status.clone();
        tokio::task::spawn_blocking(move || db.count_images(&status2))
            .await
            .map_err(join_error_response)?
            .map_err(db_error_response)?
    };

    let page = Page {
        items,
        next_cursor: next_id.map(|id| id.to_string()),
        total,
    };
    Ok(Json(json!(page)))
}

async fn stats(
    State(state): State<AppState>,
) -> Result<Json<Stats>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let s = tokio::task::spawn_blocking(move || db.stats())
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(s))
}
