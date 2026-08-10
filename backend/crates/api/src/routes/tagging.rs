//! 打标接口：/api/v1/tagging/*、/api/v1/images/{id}/tags、/api/v1/images/{id}/retag。

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::ErrorKind;
use moevault_tagger::{ApiKeyPool, InferClient, SauceNaoClient};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tagging/run", post(run_tagging))
        .route("/api/v1/tagging/stats", get(tagging_stats))
        .route("/api/v1/tagging/keys", get(key_status))
        .route("/api/v1/tags", get(list_tags))
        .route("/api/v1/images/{id}/tags", get(image_tags))
        .route("/api/v1/images/{id}/retag", post(retag_image))
}

/// GET /api/v1/tags：标签列表（含关联图数）。
async fn list_tags(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let tags = tokio::task::spawn_blocking(move || db.list_tags(1000))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "items": tags, "total": tags.len() })))
}

#[derive(Debug, Deserialize)]
pub struct RunTaggingRequest {
    /// 指定 image_ids 强制重打；None = 全部未打标 active 图。
    pub force_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize)]
pub struct RetagRequest {
    pub force_sauce: Option<bool>,
}

/// 从 settings 表读取打标配置。返回 (api_keys, min_sim, tag_threshold, model_dir)。
/// 多 key 优先读 saucenao_keys JSON（含名称/等级），回退旧逗号分隔。
#[allow(clippy::type_complexity)]
fn read_tag_config(
    db: &moevault_db::Db,
) -> Result<(Vec<moevault_core::models::SauceNaoKey>, f64, f64, Option<String>), (axum::http::StatusCode, Json<Value>)> {
    // 多 key：优先 saucenao_keys JSON
    let keys: Vec<moevault_core::models::SauceNaoKey> = db
        .get_setting("saucenao_keys")
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?
        .and_then(|json| serde_json::from_str::<Vec<moevault_core::models::SauceNaoKey>>(&json).ok())
        .unwrap_or_default();
    // 若 JSON 为空，回退旧格式
    let keys = if keys.is_empty() {
        let keys_str = db
            .get_setting("saucenao_api_keys")
            .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?
            .or(db
                .get_setting("saucenao_api_key")
                .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?)
            .unwrap_or_default();
        keys_str
            .split([',', ';', '\n', ' '])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .enumerate()
            .map(|(i, k)| moevault_core::models::SauceNaoKey {
                name: format!("Key{i}"),
                key: k,
                tier: "free".to_string(),
            })
            .collect()
    } else {
        keys
    };
    let min_sim = db
        .get_setting("saucenao_min_sim")
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(75.0);
    let tag_threshold = db
        .get_setting("tag_threshold")
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.5);
    let model_dir = db
        .get_setting("tagger_model_dir")
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok((keys, min_sim, tag_threshold, model_dir))
}

/// 获取/初始化全局 SauceNAO key pool（含持久化恢复）。
/// - 首次：尝试从快照文件恢复（key 匹配则恢复配额），否则新建
/// - 设置持久化路径（状态变更自动保存）
/// - 复用已初始化的 pool（配额/冷却跨请求保持）
async fn get_or_init_pool(
    state: &AppState,
    keys_cfg: &[moevault_core::models::SauceNaoKey],
) -> Result<Arc<ApiKeyPool>, (axum::http::StatusCode, Json<Value>)> {
    {
        let slot = state.sauce_pool.read().await;
        if let Some(pool) = slot.as_ref() {
            return Ok(pool.clone());
        }
    }
    // 持久化路径：data_dir/sauce_keys.json
    let persist_path = state.data_dir.join("sauce_keys.json");
    let key_strings: Vec<String> = keys_cfg.iter().map(|k| k.key.clone()).collect();
    let pool = match ApiKeyPool::load_from(&persist_path, &key_strings) {
        Some(p) => {
            tracing::info!("已从快照恢复 SauceNAO key 配额状态");
            p
        }
        None => {
            tracing::info!("无有效快照，新建 SauceNAO key pool");
            ApiKeyPool::from_config(keys_cfg)
        }
    };
    pool.set_persist_path(persist_path);
    let pool = Arc::new(pool);
    let mut slot = state.sauce_pool.write().await;
    *slot = Some(pool.clone());
    Ok(pool)
}

/// 通知推理服务切换打标模型目录（可选）。
async fn sync_tagger_model(state: &AppState, model_dir: &Option<String>) {
    if let Some(dir) = model_dir {
        let st = state.clone();
        let dir = dir.clone();
        tokio::spawn(async move {
            let infer = InferClient::new(st.infer_base_url.clone());
            if let Err(e) = infer.use_tagger_model(&dir).await {
                tracing::warn!(error = %e, "切换打标模型失败");
            }
        });
    }
}

async fn run_tagging(
    State(state): State<AppState>,
    Json(req): Json<RunTaggingRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let force_ids = req.force_ids;

    // 从 settings 读配置（spawn_blocking 包同步 DB 访问）
    let db_for_config = state.db.clone();
    let (api_keys, min_sim, tag_threshold, model_dir) = tokio::task::spawn_blocking(move || {
        read_tag_config(&db_for_config)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    let infer_base = state.infer_base_url.clone();

    if api_keys.is_empty() {
        return Err(error_response(
            ErrorKind::InvalidInput,
            "未配置 SauceNAO API key（设置页添加密钥）",
        ));
    }

    // 同步打标模型目录到推理服务
    sync_tagger_model(&state, &model_dir).await;

    let pool = get_or_init_pool(&state, &api_keys).await?;
    let sauce = Arc::new(SauceNaoClient::new(min_sim));
    let infer = InferClient::new(infer_base);
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let db = st.db.clone();
        let result = run_tag_pipeline_async(
            &db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            tag_threshold,
            force_ids,
        )
        .await;
        let event = match result {
            Ok(progress) => json!({
                "type": "tagging.done",
                "payload": {
                    "done": progress.done,
                    "failed": progress.failed,
                    "total": progress.total,
                },
            }),
            Err(e) => json!({
                "type": "tagging.failed",
                "payload": { "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true })))
}

/// 包装：把同步 DB 操作包进 spawn_blocking，异步网络部分在外。
#[allow(clippy::too_many_arguments)]
async fn run_tag_pipeline_async(
    db: &moevault_db::Db,
    sauce: &Arc<SauceNaoClient>,
    pool: &Arc<ApiKeyPool>,
    infer: &InferClient,
    library_dir: &std::path::Path,
    min_sim: f64,
    tag_threshold: f64,
    force_ids: Option<Vec<i64>>,
) -> Result<moevault_tagger::TagProgress, moevault_tagger::TaggerError> {
    moevault_tagger::run_tag_pipeline(
        db,
        sauce,
        pool,
        infer,
        library_dir,
        min_sim,
        tag_threshold,
        force_ids,
    )
    .await
}

async fn tagging_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let (untagged, total, tagged) = tokio::task::spawn_blocking(move || {
        let untagged = db.untagged_active_images(100000).unwrap_or_default().len();
        let total = db.count_images("active").unwrap_or(0);
        let tagged = total - untagged as i64;
        (untagged, total, tagged)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?;
    Ok(Json(json!({
        "active_images": total,
        "tagged": tagged,
        "untagged": untagged,
    })))
}

/// 查看 SauceNAO key 状态（配额/冷却/预警）。
async fn key_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let slot = state.sauce_pool.read().await;
    let Some(pool) = slot.as_ref() else {
        return Ok(Json(json!({ "keys": [], "message": "尚未初始化（首次打标后可见）" })));
    };
    let snap = pool.snapshot().await;
    let keys: Vec<Value> = snap
        .iter()
        .map(|k| {
            json!({
                "name": k.name,
                "tier": k.tier,
                "key_masked": format!("{}...{}", &k.api_key[..4.min(k.api_key.len())], &k.api_key[k.api_key.len().saturating_sub(4)..]),
                "short_remaining": k.short_remaining,
                "short_limit": k.short_limit,
                "long_remaining": k.long_remaining,
                "cooldown_secs": k.cooldown_secs(),
                "daily_paused": k.daily_paused,
                "available": k.available(),
                "total_requests": k.total_requests,
            })
        })
        .collect();
    Ok(Json(json!({ "keys": keys, "count": keys.len() })))
}

async fn image_tags(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let tags = tokio::task::spawn_blocking(move || db.image_tags(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "image_id": id, "tags": tags })))
}

async fn retag_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(_req): Json<RetagRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    // 配置
    let db_for_config = state.db.clone();
    let (api_keys, min_sim, tag_threshold, model_dir) = tokio::task::spawn_blocking(move || {
        read_tag_config(&db_for_config)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    let infer_base = state.infer_base_url.clone();
    if api_keys.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "未配置 SauceNAO API key"));
    }

    // 同步打标模型目录到推理服务
    sync_tagger_model(&state, &model_dir).await;

    let pool = get_or_init_pool(&state, &api_keys).await?;
    let sauce = Arc::new(SauceNaoClient::new(min_sim));
    let infer = InferClient::new(infer_base);
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let _ = run_tag_pipeline_async(
            &st.db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            tag_threshold,
            Some(vec![id]),
        )
        .await;
    });

    Ok(Json(json!({ "started": true, "image_id": id })))
}
