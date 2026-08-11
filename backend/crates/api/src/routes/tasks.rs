//! 任务接口：GET /api/v1/tasks（列表）、GET /api/v1/tasks/{id}（详情）。
//!
//! 打标/美学/溯源等耗时操作统一记录到 jobs 表（持久化），
//! 前端轮询本接口获取进度与历史。

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tasks", get(list_tasks).delete(clear_tasks))
        .route("/api/v1/tasks/{id}", get(get_task))
        .route("/api/v1/tasks/{id}/cancel", post(cancel_task))
        .route("/api/v1/tasks/{id}/resume", post(resume_task))
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
    // 各 key 额度消耗（SauceNAO 任务相关）
    let keys_usage: Vec<Value> = {
        let slot = state.sauce_pool.read().await;
        match slot.as_ref() {
            Some(pool) => pool
                .snapshot()
                .await
                .iter()
                .map(|k| {
                    json!({
                        "name": k.name,
                        "tier": k.tier,
                        "long_remaining": k.long_remaining,
                        "short_remaining": k.short_remaining,
                        "total_requests": k.total_requests,
                        "cooldown_secs": k.cooldown_secs(),
                        "daily_paused": k.daily_paused,
                    })
                })
                .collect(),
            None => vec![],
        }
    };
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
            "keys_usage": keys_usage,
        }))),
        None => Err(error_response(ErrorKind::NotFound, format!("任务 {id} 不存在"))),
    }
}

/// DELETE /api/v1/tasks：清空历史任务（保留进行中）。
async fn clear_tasks(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let n = tokio::task::spawn_blocking(move || db.clear_jobs())
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "cleared": n })))
}

/// POST /api/v1/tasks/{id}/cancel：中断任务（置 cancelled，worker 检测后停止）。
async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        // 校验任务存在
        if db.get_job(id).map_err(db_error_response)?.is_none() {
            return Err(error_response(ErrorKind::NotFound, format!("任务 {id} 不存在")));
        }
        db.cancel_job(id).map_err(db_error_response)?;
        db.add_log("warn", "task", &format!("任务 #{id} 已中断（用户操作）"))
            .map_err(db_error_response)?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "cancelled": true, "job_id": id })))
}

/// POST /api/v1/tasks/{id}/resume：继续被中断的任务（重新从 payload 的 image_ids 入队，
/// 已处理的图由 filter_eligible 自动跳过）。
async fn resume_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let job = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.get_job(id)
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;
    let Some(job) = job else {
        return Err(error_response(ErrorKind::NotFound, format!("任务 {id} 不存在")));
    };
    if job.status != "cancelled" {
        return Err(error_response(ErrorKind::InvalidInput, "只有已中断（cancelled）的任务可以继续"));
    }
    // 从 payload 提取 image_ids
    let ids: Vec<i64> = job
        .payload
        .as_deref()
        .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
        .and_then(|v| v.get("image_ids").cloned())
        .and_then(|v| serde_json::from_value::<Vec<i64>>(v).ok())
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "任务负载中没有图片 id，无法继续"));
    }
    // 重新置为 pending（清空计数），重新启动溯源管线
    let ids_count = ids.len();
    tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || {
            db.resume_job(id)?;
            db.add_log("info", "task", &format!("任务 #{id} 已继续（{ids_count} 张重新入队，已处理自动跳过）"))?;
            Ok::<_, moevault_db::DbError>(())
        }
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    // 读取配置并启动管线（与 run_sauce 相同逻辑，但用已存 ids）
    let db_for_config = state.db.clone();
    let (api_keys, min_sim, _, _) = tokio::task::spawn_blocking(move || {
        super::tagging::read_tag_config_public(&db_for_config)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    if api_keys.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "未配置 SauceNAO API key"));
    }
    let pool = {
        let slot = st.sauce_pool.read().await;
        match slot.as_ref() {
            Some(p) => p.clone(),
            None => {
                // 尚未初始化：用配置初始化
                drop(slot);
                super::tagging::init_pool_public(&st, &api_keys).await?
            }
        }
    };
    let sauce = std::sync::Arc::new(moevault_tagger::SauceNaoClient::new(min_sim));
    let infer = moevault_tagger::InferClient::new(st.infer_base_url.clone());
    let library_dir = st.library_dir();
    let ids_len = ids.len();

    tokio::spawn(async move {
        let db = st.db.clone();
        let _ = db.start_job(id, ids_len as i64);
        let result = moevault_tagger::run_sauce_pipeline(
            &db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            Some(ids),
            Some(id),
            false,
        )
        .await;
        let (status, done, failed, error) = match &result {
            Ok(progress) => ("done", progress.done as i64, progress.failed as i64, None),
            Err(e) => ("failed", 0, 0, Some(e.to_string())),
        };
        let final_status =
            if db.get_job(id).ok().flatten().map(|j| j.status) == Some("cancelled".into()) {
                "cancelled"
            } else {
                status
            };
        let _ = db.update_job(id, final_status, done, failed, error.as_deref());
        let event = match result {
            Ok(progress) => json!({
                "type": "task.done",
                "payload": { "job_id": id, "kind": "sauce", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": id, "kind": "sauce", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "ok": true, "resumed": true, "job_id": id, "ids": ids_len })))
}
