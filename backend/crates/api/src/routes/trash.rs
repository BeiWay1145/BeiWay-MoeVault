//! 回收站接口：/api/v1/trash/*。

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, ApiResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/trash", get(list_trash))
        .route("/api/v1/trash/purge-all", post(purge_all))
        .route("/api/v1/trash/audit", get(audit))
        .route("/api/v1/trash/repair", post(repair))
        .route("/api/v1/trash/{image_id}/restore", post(restore))
        .route("/api/v1/trash/{image_id}/purge", post(purge))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

fn parse_cursor(cursor: &Option<String>) -> Result<Option<i64>, (axum::http::StatusCode, Json<Value>)> {
    match cursor {
        Some(c) => c
            .parse::<i64>()
            .map(Some)
            .map_err(|_| error_response(ErrorKind::InvalidInput, format!("cursor 格式错误: {c}"))),
        None => Ok(None),
    }
}

async fn list_trash(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> ApiResult<Value> {
    let limit = params.limit.unwrap_or(100);
    let cursor_id = parse_cursor(&params.cursor)?;
    let db = state.db.clone();
    let (items, next) = tokio::task::spawn_blocking(move || db.list_recycled(limit, cursor_id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    let total = {
        let db = state.db.clone();
        tokio::task::spawn_blocking(move || db.recycle_bin_count())
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
            .map_err(db_error_response)?
    };
    Ok(Json(json!({
        "items": items,
        "next_cursor": next.map(|id| id.to_string()),
        "total": total,
    })))
}

async fn restore(
    State(state): State<AppState>,
    Path(image_id): Path<i64>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    tokio::task::spawn_blocking(move || {
        moevault_dedup::restore_image(&db, image_id, &library, &recycle)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "restored": image_id })))
}

async fn purge(
    State(state): State<AppState>,
    Path(image_id): Path<i64>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    let thumbs = state.thumbs_dir();
    tokio::task::spawn_blocking(move || {
        moevault_dedup::purge_image(&db, image_id, &library, &recycle, &thumbs)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "purged": image_id })))
}

/// POST /api/v1/trash/repair：修复回收站一致性。
///
/// body：`{ "dry_run": true, "remove_orphans": false }`，两项都可省略。
/// - dry_run（默认 true）：只统计，不动任何数据
/// - remove_orphans：是否删除回收站目录里没有任何引用的孤儿文件
///
/// 「半完成态」图片（文件已在回收站但状态还是 active）会被补正成正式回收；
/// 僵尸行（状态 recycled 但文件与记录都丢了）会被清掉，避免回收站页面
/// 永远看不到、恢复又必然失败的幽灵条目。
#[derive(Debug, Deserialize)]
pub struct RepairRequest {
    pub dry_run: Option<bool>,
    pub remove_orphans: Option<bool>,
}

async fn repair(
    State(state): State<AppState>,
    Json(req): Json<RepairRequest>,
) -> ApiResult<Value> {
    let opts = moevault_dedup::RepairOptions {
        dry_run: req.dry_run.unwrap_or(true),
        remove_orphans: req.remove_orphans.unwrap_or(false),
    };
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    let report = tokio::task::spawn_blocking(move || {
        moevault_dedup::repair_recycle(&db, &library, &recycle, opts)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!(report)))
}

/// GET /api/v1/trash/audit：只读体检，返回三类问题与孤儿文件清单。
async fn audit(State(state): State<AppState>) -> ApiResult<Value> {
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    let report = tokio::task::spawn_blocking(move || {
        moevault_dedup::audit_recycle(&db, &library, &recycle)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!(report)))
}
async fn purge_all(
    State(state): State<AppState>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    let thumbs = state.thumbs_dir();
    let n = tokio::task::spawn_blocking(move || {
        moevault_dedup::purge_all(&db, &library, &recycle, &thumbs)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "purged": n })))
}
