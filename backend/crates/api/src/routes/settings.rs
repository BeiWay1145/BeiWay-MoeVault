//! 设置接口：/api/v1/settings。
//!
//! - GET /api/v1/settings：返回全部设置（含 saucenao_keys 结构化 JSON）
//! - PUT /api/v1/settings：批量更新（增量 diff，白名单 key）
//! - GET /api/v1/settings/saucenao-keys：多 key 列表
//! - POST /api/v1/settings/saucenao-keys：添加 key
//! - DELETE /api/v1/settings/saucenao-keys/{name}：删除 key

use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use moevault_core::models::SauceNaoKey;
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

/// 可写设置白名单（避免任意 key 写入）。
const SETTINGS_WHITELIST: &[&str] = &[
    "saucenao_min_sim",
    "tag_threshold",
    "tagger_model_dir",
    "tagger_model_name",
    "aesthetic_model",
    "dedup_hamming",
    "sidecar_enabled",
    "cn_dict_enabled",
    "recycle_days",
    "library_dir",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings", get(get_settings).put(update_settings))
        .route("/api/v1/settings/saucenao-keys", get(list_keys).post(add_key))
        .route("/api/v1/settings/saucenao-keys/{name}", delete(delete_key))
}

/// 从 settings 表读取 saucenao_keys JSON（回退旧格式）。
fn read_keys(db: &moevault_db::Db) -> Result<Vec<SauceNaoKey>, moevault_db::DbError> {
    if let Some(json) = db.get_setting("saucenao_keys")? {
        if let Ok(keys) = serde_json::from_str::<Vec<SauceNaoKey>>(&json) {
            return Ok(keys);
        }
    }
    // 回退：旧逗号分隔 saucenao_api_keys / 单 key saucenao_api_key
    let legacy = db
        .get_setting("saucenao_api_keys")?
        .or(db.get_setting("saucenao_api_key")?)
        .unwrap_or_default();
    let keys: Vec<SauceNaoKey> = legacy
        .split([',', ';', '\n', ' '])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, k)| SauceNaoKey {
            name: format!("Key{i}"),
            key: k,
            tier: "free".to_string(),
        })
        .collect();
    Ok(keys)
}

fn write_keys(db: &moevault_db::Db, keys: &[SauceNaoKey]) -> Result<(), moevault_db::DbError> {
    let json = serde_json::to_string(keys).unwrap_or_else(|_| "[]".to_string());
    db.put_setting("saucenao_keys", &json)
}

async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let out = tokio::task::spawn_blocking(move || {
        let mut map = serde_json::Map::new();
        for k in SETTINGS_WHITELIST {
            if let Ok(Some(v)) = db.get_setting(k) {
                map.insert((*k).to_string(), Value::String(v));
            }
        }
        // 结构化多 key
        if let Ok(keys) = read_keys(&db) {
            map.insert("saucenao_keys".to_string(), serde_json::to_value(keys).unwrap_or(Value::Null));
        }
        // 缺失项给默认值
        if !map.contains_key("saucenao_min_sim") {
            map.insert("saucenao_min_sim".to_string(), Value::String("75".into()));
        }
        if !map.contains_key("tag_threshold") {
            map.insert("tag_threshold".to_string(), Value::String("0.5".into()));
        }
        serde_json::Value::Object(map)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(flatten)]
    pub fields: std::collections::HashMap<String, Value>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let fields = req.fields;
    tokio::task::spawn_blocking(move || {
        for (k, v) in &fields {
            if !SETTINGS_WHITELIST.contains(&k.as_str()) {
                continue; // 非白名单忽略
            }
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            db.put_setting(k, &val)
                .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
        }
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct AddKeyRequest {
    pub name: Option<String>,
    pub key: String,
    pub tier: Option<String>,
}

async fn list_keys(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let keys = tokio::task::spawn_blocking(move || read_keys(&db))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    // 脱敏返回（仅管理 UI 显示用）
    let masked: Vec<Value> = keys
        .iter()
        .map(|k| {
            json!({
                "name": k.name,
                "key_masked": format!("{}...{}", &k.key[..2.min(k.key.len())], &k.key[k.key.len().saturating_sub(2)..]),
                "tier": k.tier,
                "has_key": true,
            })
        })
        .collect();
    Ok(Json(json!({ "keys": masked, "count": keys.len() })))
}

async fn add_key(
    State(state): State<AppState>,
    Json(req): Json<AddKeyRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let key = req.key.trim().to_string();
    if key.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "API key 不能为空"));
    }
    let tier = req.tier.unwrap_or_else(|| "free".to_string());
    let db = state.db.clone();
    let name = tokio::task::spawn_blocking(move || {
        let mut keys = read_keys(&db).map_err(db_error_response)?;
        // 去重（同名覆盖）
        keys.retain(|k| k.name != req.name.as_deref().unwrap_or(""));
        let name = match &req.name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => {
                // 默认 Key{n}：第一个空位
                let used: std::collections::HashSet<String> =
                    keys.iter().map(|k| k.name.clone()).collect();
                let mut i = 0;
                loop {
                    let candidate = format!("Key{i}");
                    if !used.contains(&candidate) {
                        break candidate;
                    }
                    i += 1;
                }
            }
        };
        keys.push(SauceNaoKey { name: name.clone(), key, tier });
        write_keys(&db, &keys).map_err(db_error_response)?;
        Ok::<String, (axum::http::StatusCode, Json<Value>)>(name)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "name": name })))
}

async fn delete_key(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut keys = read_keys(&db).map_err(db_error_response)?;
        keys.retain(|k| k.name != name);
        write_keys(&db, &keys).map_err(db_error_response)?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true })))
}
