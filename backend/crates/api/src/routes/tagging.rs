//! 打标接口：/api/v1/tagging/*、/api/v1/images/{id}/tags、/api/v1/images/{id}/retag。

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use moevault_core::ErrorKind;
use moevault_tagger::{ApiKeyPool, InferClient, SauceNaoClient};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tagging/run", post(run_tagging))
        .route("/api/v1/tagging/stats", get(tagging_stats))
        .route("/api/v1/tagging/keys", get(key_status))
        .route("/api/v1/sauce/run", post(run_sauce))
        .route("/api/v1/tags", get(list_tags))
        .route("/api/v1/tags/browse", get(browse_tags))
        .route("/api/v1/tags/{id}/category", put(update_tag_category))
        .route("/api/v1/tags/{id}/name-cn", put(update_tag_name_cn))
        .route("/api/v1/tags/{id}/aliases", get(list_aliases).post(add_alias))
        .route("/api/v1/tags/{id}/aliases/{alias_id}", delete(remove_alias))
        .route("/api/v1/tags/{id}/cover", put(set_tag_cover))
        .route("/api/v1/tags/{id}", delete(delete_tag))
        .route("/api/v1/tags/{id}/blacklist", post(set_tag_blacklist))
        .route("/api/v1/tags/batch-delete", post(batch_delete_tags))
        .route("/api/v1/tags/batch-blacklist", post(batch_blacklist_tags))
        .route("/api/v1/images/{id}/tags", get(image_tags))
        .route("/api/v1/images/{id}/retag", post(retag_image))
}

/// GET /api/v1/tags：标签列表（含关联图数，支持 q 搜索、分类、未设中文别名筛选）。
async fn list_tags(
    State(state): State<AppState>,
    Query(params): Query<ListTagsParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let q = params.q.clone();
    let offset = params.offset.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(500).clamp(1, 1000);
    let category = params.category.clone();
    let no_cn = params.no_cn.as_deref().map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let (tags, total) = {
        let db2 = db.clone();
        let q2 = q.clone();
        let cat2 = category.clone();
        tokio::task::spawn_blocking(move || db2.list_tags_advanced(limit, offset, q2.as_deref(), cat2.as_deref(), no_cn))
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
            .map_err(db_error_response)?
    };
    Ok(Json(json!({ "items": tags, "total": total })))
}

/// GET /api/v1/tags/{id}/aliases：标签的中文别名列表。
async fn list_aliases(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let aliases = tokio::task::spawn_blocking(move || db.list_tag_aliases(id))
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "tag_id": id, "aliases": aliases })))
}

/// POST /api/v1/tags/{id}/aliases：添加中文别名。body `{ "alias": "黑色头发" }`。
#[derive(Debug, Deserialize)]
pub struct AddAliasBody {
    pub alias: String,
}

async fn add_alias(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddAliasBody>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let alias_id = tokio::task::spawn_blocking(move || db.add_tag_alias(id, &body.alias))
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "tag_id": id, "alias_id": alias_id })))
}

/// DELETE /api/v1/tags/{id}/aliases/{alias_id}：删除中文别名。
async fn remove_alias(
    State(state): State<AppState>,
    Path((_id, alias_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.remove_tag_alias(alias_id))
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "alias_id": alias_id })))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListTagsParams {
    /// 关键字（名称或中文名 LIKE）。
    pub q: Option<String>,
    /// 分页：offset/limit（默认 0/500，避免一次性加载全部导致卡死）。
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    /// 分类过滤：artist/copyright/character/general。
    pub category: Option<String>,
    /// 仅未设中文别名（name_cn 为空且无别名）。
    pub no_cn: Option<String>,
}

/// GET /api/v1/tags/browse：分类浏览标签卡片（category 过滤 + 可配置封面 + 总数）。
#[derive(Debug, Deserialize, Default)]
pub struct BrowseTagsParams {
    /// 分类：artist / copyright / character / general（缺省全部）。
    pub category: Option<String>,
    /// 标签名/中文名关键字。
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

async fn browse_tags(
    State(state): State<AppState>,
    Query(params): Query<BrowseTagsParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let category = params.category.clone();
    let q = params.q.clone();
    let limit = params.limit.unwrap_or(200).clamp(1, 500);
    let offset = params.offset.unwrap_or(0).max(0);
    // 封面规则来自设置（tag_cover_rule，缺省 aesthetic）
    let cover_rule = db
        .get_setting("tag_cover_rule")
        .map_err(db_error_response)?
        .unwrap_or_else(|| "aesthetic".to_string());
    let (items, total) = tokio::task::spawn_blocking(move || {
        db.browse_tags(category.as_deref(), q.as_deref(), limit, offset, &cover_rule)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(db_error_response)?;
    Ok(Json(json!({ "items": items, "total": total })))
}

/// PUT /api/v1/tags/{id}/name-cn：设置/清除中文别名（用于搜索联想与显示）。body `{ "name_cn": "女孩" | null }`。
#[derive(Debug, Deserialize)]
pub struct SetNameCnBody {
    pub name_cn: Option<String>,
}

async fn update_tag_name_cn(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<SetNameCnBody>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let name_cn = body.name_cn.clone();
    tokio::task::spawn_blocking(move || db.set_tag_name_cn(id, name_cn.as_deref()))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "tag_id": id })))
}
#[derive(Debug, Deserialize)]
pub struct SetCoverBody {
    pub image_id: Option<i64>,
}

async fn set_tag_cover(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<SetCoverBody>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_tag_cover(id, body.image_id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "tag_id": id })))
}

/// PUT /api/v1/tags/{id}/category：修改标签分类。
async fn update_tag_category(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let Some(cat) = req.get("category").and_then(|v| v.as_str()) else {
        return Err(error_response(ErrorKind::InvalidInput, "缺少 category"));
    };
    let cat = cat.trim().to_string();
    if !["artist", "copyright", "character", "general"].contains(&cat.as_str()) {
        return Err(error_response(
            ErrorKind::InvalidInput,
            format!("category 仅支持 artist/copyright/character/general，收到: {cat}"),
        ));
    }
    let db = state.db.clone();
    let cat_for_db = cat.clone();
    tokio::task::spawn_blocking(move || {
        db.set_tag_category(id, &cat_for_db).map_err(db_error_response)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "tag_id": id, "category": cat })))
}

/// DELETE /api/v1/tags/{id}：删除标签。
async fn delete_tag(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.delete_tag(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true })))
}

/// POST /api/v1/tags/{id}/blacklist：拉黑/取消拉黑标签。body {blacklisted: bool}
async fn set_tag_blacklist(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let bl = req.get("blacklisted").and_then(|v| v.as_bool()).unwrap_or(true);
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_tag_blacklisted(id, bl))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "blacklisted": bl })))
}

/// POST /api/v1/tags/batch-delete：批量删除标签。body {ids: []}
async fn batch_delete_tags(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let ids: Vec<i64> = req
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "ids 不能为空"));
    }
    let ids_len = ids.len();
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.delete_tags(&ids))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "deleted": ids_len })))
}

/// POST /api/v1/tags/batch-blacklist：批量拉黑标签。body {ids: [], blacklisted: bool}
async fn batch_blacklist_tags(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let ids: Vec<i64> = req
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "ids 不能为空"));
    }
    let bl = req.get("blacklisted").and_then(|v| v.as_bool()).unwrap_or(true);
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_tags_blacklisted(&ids, bl))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "blacklisted": bl })))
}

#[derive(Debug, Deserialize)]
pub struct RunTaggingRequest {
    /// 指定 image_ids 强制重打；None = 全部未打标 active 图。
    pub force_ids: Option<Vec<i64>>,
    /// 溯源时是否忽略不可溯源标记（强制重试）。
    pub force_sauce: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RetagRequest {
    pub force_sauce: Option<bool>,
}

/// 从 settings 表读取打标配置。返回 (api_keys, min_sim, tag_threshold, model_dir, model_kind)。
/// 多 key 优先读 saucenao_keys JSON（含名称/等级），回退旧逗号分隔。
#[allow(clippy::type_complexity)]
pub(crate) fn read_tag_config_public(
    db: &moevault_db::Db,
) -> Result<
    (
        Vec<moevault_core::models::SauceNaoKey>,
        f64,
        f64,
        Option<String>,
        Option<String>,
    ),
    (axum::http::StatusCode, Json<Value>),
> {
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
    let model_kind = db
        .get_setting("tagger_model_kind")
        .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok((keys, min_sim, tag_threshold, model_dir, model_kind))
}

/// 获取/初始化全局 SauceNAO key pool（含持久化恢复）。
/// - 首次：尝试从快照文件恢复（key 匹配则恢复配额），否则新建
/// - 设置持久化路径（状态变更自动保存）
/// - 复用已初始化的 pool（配额/冷却跨请求保持）
pub(crate) async fn init_pool_public(
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

/// 通知推理服务切换打标模型目录/种类（可选）。
/// 空/未设置 = 自动探测模式：跳过切换，推理服务保持其自动探测到的目录/种类。
async fn sync_tagger_model(
    state: &AppState,
    model_dir: &Option<String>,
    model_kind: &Option<String>,
) {
    if let Some(dir) = model_dir {
        if dir.trim().is_empty() {
            return; // 自动探测模式，无需切换
        }
        let st = state.clone();
        let dir = dir.clone();
        let kind = model_kind.clone().filter(|k| !k.trim().is_empty());
        tokio::spawn(async move {
            let infer = InferClient::new(st.infer_base_url.clone());
            if let Err(e) = infer.use_tagger_model(&dir, kind.as_deref()).await {
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
    let (api_keys, min_sim, tag_threshold, model_dir, model_kind) =
        tokio::task::spawn_blocking(move || read_tag_config_public(&db_for_config))
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    let infer_base = state.infer_base_url.clone();

    if api_keys.is_empty() {
        return Err(error_response(
            ErrorKind::InvalidInput,
            "未配置 SauceNAO API key（设置页添加密钥）",
        ));
    }

    // 创建任务记录（持久化）
    let payload = force_ids
        .as_ref()
        .map(|ids| serde_json::json!({ "image_ids": ids }).to_string());
    let job_id = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.create_job("tag", payload.as_deref())
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    // 同步打标模型目录/种类到推理服务
    sync_tagger_model(&state, &model_dir, &model_kind).await;

    let pool = init_pool_public(&state, &api_keys).await?;
    let sauce = Arc::new(SauceNaoClient::new(min_sim));
    let infer = InferClient::new(infer_base);
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let db = st.db.clone();
        let _ = db.start_job(job_id, force_ids.as_ref().map_or(0, |v| v.len() as i64));
        let _ = db.add_log("info", "task", &format!("打标任务 #{job_id} 启动（{} 张）", force_ids.as_ref().map_or(0, |v| v.len())));
        let result = run_tag_pipeline_async(
            &db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            tag_threshold,
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
            "task",
            &format!("打标任务 #{job_id} 完成：成功 {done} 张，失败 {failed} 张{}", error.as_deref().map(|e| format!("，错误：{e}")).unwrap_or_default()),
        );
        let event = match result {
            Ok(progress) => json!({
                "type": "task.done",
                "payload": { "job_id": job_id, "kind": "tag", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": job_id, "kind": "tag", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "job_id": job_id, "kind": "tag" })))
}

/// POST /api/v1/sauce/run：SauceNAO 溯源任务（只溯源+爬标签，失败不本地打标）。
async fn run_sauce(
    State(state): State<AppState>,
    Json(req): Json<RunTaggingRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let st = state.clone();
    let force_ids = req.force_ids;

    // 配置
    let db_for_config = state.db.clone();
    let (api_keys, min_sim, _, _, _) = tokio::task::spawn_blocking(move || {
        read_tag_config_public(&db_for_config)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    if api_keys.is_empty() {
        return Err(error_response(
            ErrorKind::InvalidInput,
            "未配置 SauceNAO API key（设置页添加密钥）",
        ));
    }

    // 创建任务记录
    let payload = force_ids
        .as_ref()
        .map(|ids| serde_json::json!({ "image_ids": ids }).to_string());
    let job_id = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.create_job("sauce", payload.as_deref())
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    let pool = init_pool_public(&state, &api_keys).await?;
    let sauce = Arc::new(SauceNaoClient::new(min_sim));
    let infer = InferClient::new(st.infer_base_url.clone());
    let library_dir = st.library_dir();
    let force_sauce = req.force_sauce.unwrap_or(false);

    tokio::spawn(async move {
        let db = st.db.clone();
        let _ = db.start_job(job_id, force_ids.as_ref().map_or(0, |v| v.len() as i64));
        let _ = db.add_log("info", "task", &format!("溯源任务 #{job_id} 启动（{} 张{}）", force_ids.as_ref().map_or(0, |v| v.len()), if force_sauce { "，强制重试不可溯源图" } else { "" }));
        let result = moevault_tagger::run_sauce_pipeline(
            &db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            force_ids,
            Some(job_id),
            force_sauce,
        )
        .await;
        let (status, done, failed, error) = match &result {
            Ok(progress) => ("done", progress.done as i64, progress.failed as i64, None),
            Err(e) => ("failed", 0, 0, Some(e.to_string())),
        };
        // 若任务已被取消（中断），保持 cancelled 状态（不覆盖为 done）
        let final_status =
            if db.get_job(job_id).ok().flatten().map(|j| j.status) == Some("cancelled".into()) {
                "cancelled"
            } else {
                status
            };
        let _ = db.update_job(job_id, final_status, done, failed, error.as_deref());
        let _ = db.add_log(
            if final_status == "done" { "info" } else if final_status == "cancelled" { "warn" } else { "error" },
            "task",
            &format!("溯源任务 #{job_id} 结束（状态 {}）：成功 {done} 张，失败 {failed} 张{}", final_status, error.as_deref().map(|e| format!("，错误：{e}")).unwrap_or_default()),
        );
        let event = match result {
            Ok(progress) => json!({
                "type": "task.done",
                "payload": { "job_id": job_id, "kind": "sauce", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": job_id, "kind": "sauce", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "job_id": job_id, "kind": "sauce" })))
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
    job_id: Option<i64>,
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
        job_id,
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
    let (api_keys, min_sim, tag_threshold, model_dir, model_kind) =
        tokio::task::spawn_blocking(move || read_tag_config_public(&db_for_config))
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    let infer_base = state.infer_base_url.clone();
    if api_keys.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "未配置 SauceNAO API key"));
    }

    // 创建任务记录
    let payload = serde_json::json!({ "image_ids": [id] }).to_string();
    let job_id = tokio::task::spawn_blocking({
        let db = state.db.clone();
        move || db.create_job("tag", Some(&payload))
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    // 同步打标模型目录/种类到推理服务
    sync_tagger_model(&state, &model_dir, &model_kind).await;

    let pool = init_pool_public(&state, &api_keys).await?;
    let sauce = Arc::new(SauceNaoClient::new(min_sim));
    let infer = InferClient::new(infer_base);
    let library_dir = st.library_dir();

    tokio::spawn(async move {
        let _ = st.db.start_job(job_id, 1);
        let result = run_tag_pipeline_async(
            &st.db,
            &sauce,
            &pool,
            &infer,
            &library_dir,
            min_sim,
            tag_threshold,
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
                "payload": { "job_id": job_id, "kind": "tag", "done": progress.done, "failed": progress.failed, "total": progress.total },
            }),
            Err(e) => json!({
                "type": "task.failed",
                "payload": { "job_id": job_id, "kind": "tag", "error": e.to_string() },
            }),
        };
        st.broadcast(event.to_string());
    });

    Ok(Json(json!({ "started": true, "job_id": job_id, "image_id": id, "kind": "tag" })))
}
