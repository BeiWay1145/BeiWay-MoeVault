//! 查重接口：/api/v1/dedup/*。
//!
//! 对应 docs/TECH_DETAILS.md 第 2.3 节。scan 为后台任务 + WS 广播。

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use moevault_dedup::{cluster_scope, full_recluster, incremental_cluster, DEFAULT_HAMMING_THRESHOLD};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/dedup/stats", get(stats))
        .route("/api/v1/dedup/groups", get(list_groups))
        .route("/api/v1/dedup/groups/{id}", get(get_group))
        .route("/api/v1/dedup/scan", post(scan))
        .route("/api/v1/dedup/scan-scope", post(scan_scope))
        .route("/api/v1/dedup/groups/{id}/resolve", post(resolve_group))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    /// true = 全量重建；默认 false = 增量（只处理未分组的 active 图）。
    pub full: Option<bool>,
    /// 汉明距离阈值，默认 8。
    pub threshold: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveRequest {
    /// best_only：保留最优，其余入回收站。
    /// specific：按 keep_ids/recycle_ids 精确指定。
    pub mode: String,
    #[serde(default)]
    pub keep_ids: Vec<i64>,
    #[serde(default)]
    pub recycle_ids: Vec<i64>,
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

async fn stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let s = tokio::task::spawn_blocking(move || db.dedup_stats())
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!(s)))
}

async fn list_groups(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let limit = params.limit.unwrap_or(100);
    let cursor_id = parse_cursor(&params.cursor)?;
    let db = state.db.clone();
    let (items, next) = tokio::task::spawn_blocking(move || db.list_dedup_groups(limit, cursor_id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    let total = {
        let db = state.db.clone();
        tokio::task::spawn_blocking(move || db.dedup_stats())
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
            .map_err(db_error_response)?
            .group_count
    };
    Ok(Json(json!({
        "items": items,
        "next_cursor": next.map(|id| id.to_string()),
        "total": total,
    })))
}

async fn get_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let group = tokio::task::spawn_blocking(move || db.get_dedup_group(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    match group {
        Some(g) => Ok(Json(json!(g))),
        None => Err(error_response(ErrorKind::NotFound, format!("查重组 {id} 不存在"))),
    }
}

async fn scan(
    State(state): State<AppState>,
    Json(req): Json<ScanRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let full = req.full.unwrap_or(false);
    let threshold = req.threshold.unwrap_or(DEFAULT_HAMMING_THRESHOLD);
    if threshold > 64 {
        return Err(error_response(
            ErrorKind::InvalidInput,
            format!("threshold 应在 0..=64，收到 {threshold}"),
        ));
    }

    let st = state.clone();
    tokio::spawn(async move {
        let db = st.db.clone();
        let result = tokio::task::spawn_blocking(move || {
            if full {
                full_recluster(&db, threshold)
            } else {
                incremental_cluster(&db, threshold)
            }
        })
        .await;

        let event = match result {
            Ok(Ok(stats)) => {
                let s = st.db.dedup_stats().unwrap_or_default();
                json!({
                    "type": "dedup.updated",
                    "payload": {
                        "groups_created": stats.groups_created,
                        "images_clustered": stats.images_clustered,
                        "redundant_marked": stats.redundant_marked,
                        "group_count": s.group_count,
                        "redundant_count": s.redundant_count,
                    },
                })
            }
            Ok(Err(e)) => json!({
                "type": "dedup.failed",
                "payload": { "error": e.to_string() },
            }),
            Err(e) => json!({
                "type": "dedup.failed",
                "payload": { "error": format!("后台任务失败: {e}") },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "full": full, "threshold": threshold })))
}

/// POST /api/v1/dedup/scan-scope：对指定 ids 范围聚类（主目录按筛选集/选中图查重）。
/// 同步执行（范围小），返回统计。
#[derive(Debug, Deserialize)]
pub struct ScanScopeRequest {
    pub image_ids: Vec<i64>,
    pub threshold: Option<u32>,
}

async fn scan_scope(
    State(state): State<AppState>,
    Json(req): Json<ScanScopeRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    if req.image_ids.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "image_ids 不能为空"));
    }
    let threshold = req.threshold.unwrap_or(DEFAULT_HAMMING_THRESHOLD);
    if threshold > 64 {
        return Err(error_response(
            ErrorKind::InvalidInput,
            format!("threshold 应在 0..=64，收到 {threshold}"),
        ));
    }
    let db = state.db.clone();
    let db_for_stats = db.clone();
    let ids = req.image_ids.clone();
    let stats = tokio::task::spawn_blocking(move || cluster_scope(&db, &ids, threshold))
        .await
        .map_err(join_error_response)?
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    let s = db_for_stats.dedup_stats().unwrap_or_default();
    // 广播更新
    state.broadcast(
        json!({
            "type": "dedup.updated",
            "payload": {
                "groups_created": stats.groups_created,
                "images_clustered": stats.images_clustered,
                "redundant_marked": stats.redundant_marked,
                "group_count": s.group_count,
                "redundant_count": s.redundant_count,
            },
        })
        .to_string(),
    );
    Ok(Json(json!({
        "groups_created": stats.groups_created,
        "images_clustered": stats.images_clustered,
        "redundant_marked": stats.redundant_marked,
    })))
}

async fn resolve_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let library_dir = state.library_dir();
    let recycle_dir = state.recycle_dir();

    let db = state.db.clone();
    let detail = tokio::task::spawn_blocking(move || db.get_dedup_group(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    let Some(detail) = detail else {
        return Err(error_response(ErrorKind::NotFound, format!("查重组 {id} 不存在")));
    };

    // 确定要回收的成员
    let recycle_ids: Vec<i64> = match req.mode.as_str() {
        "best_only" => {
            let best = detail
                .members
                .iter()
                .find(|m| m.is_best)
                .or_else(|| {
                    detail
                        .members
                        .iter()
                        .max_by(|a, b| a.clarity_score.partial_cmp(&b.clarity_score).unwrap_or(std::cmp::Ordering::Equal))
                })
                .map(|m| m.image_id);
            detail
                .members
                .iter()
                .filter(|m| Some(m.image_id) != best)
                .map(|m| m.image_id)
                .collect()
        }
        "specific" => {
            for k in &req.keep_ids {
                if !detail.members.iter().any(|m| &m.image_id == k) {
                    return Err(error_response(
                        ErrorKind::InvalidInput,
                        format!("keep_ids 含非本组成员: {k}"),
                    ));
                }
            }
            for r in &req.recycle_ids {
                if !detail.members.iter().any(|m| &m.image_id == r) {
                    return Err(error_response(
                        ErrorKind::InvalidInput,
                        format!("recycle_ids 含非本组成员: {r}"),
                    ));
                }
            }
            req.recycle_ids.clone()
        }
        other => {
            return Err(error_response(
                ErrorKind::InvalidInput,
                format!("mode 仅支持 best_only/specific，收到 {other}"),
            ))
        }
    };

    if recycle_ids.is_empty() {
        return Ok(Json(json!({ "recycled": 0 })));
    }

    // 逐个入回收站（spawn_blocking 内完成，因含文件 IO）
    let db2 = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut recycled = 0;
        for rid in &recycle_ids {
            match moevault_dedup::recycle_image(&db2, *rid, "duplicate", &library_dir, &recycle_dir) {
                Ok(()) => recycled += 1,
                Err(e) => tracing::warn!(image_id = rid, error = %e, "resolve：单张回收失败，继续"),
            }
        }
        // 标记组已处理（若全部 active 成员已回收或保留）
        let _ = db2.set_group_state(id, "resolved");
        Ok::<usize, moevault_dedup::DedupError>(recycled)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;

    Ok(Json(json!({ "recycled": result })))
}
