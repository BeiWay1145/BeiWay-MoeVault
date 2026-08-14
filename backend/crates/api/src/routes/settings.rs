//! 设置接口：/api/v1/settings。
//!
//! - GET /api/v1/settings：返回全部设置（含 saucenao_keys 结构化 JSON）
//! - PUT /api/v1/settings：批量更新（增量 diff，白名单 key）
//! - GET /api/v1/settings/saucenao-keys：多 key 列表
//! - POST /api/v1/settings/saucenao-keys：添加 key
//! - DELETE /api/v1/settings/saucenao-keys/{name}：删除 key

use axum::{
    extract::{Path, State},
    routing::{delete, get, put},
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
    "tagger_device",
    "aesthetic_device",
    "dedup_hamming",
    "sidecar_enabled",
    "cn_dict_enabled",
    "recycle_days",
    "library_dir",
    "pagination_enabled",
    "page_size",
    "close_to_tray",
    "waterfall_columns",
    "log_clear_on_start",
    "sidebar_hover_expand",
    "preload_count",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings", get(get_settings).put(update_settings))
        .route("/api/v1/settings/saucenao-keys", get(list_keys).post(add_key))
        .route("/api/v1/settings/saucenao-keys/{name}", delete(delete_key))
        .route("/api/v1/settings/saucenao-keys/{name}/quota", put(set_key_quota))
        .route("/api/v1/devices", get(proxy_devices))
}

/// GET /api/v1/devices：转发到 Python 推理服务 /devices（获取可用推理设备）。
async fn proxy_devices(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let base = state.infer_base_url.clone();
    // 手动 HTTP GET（api crate 无 reqwest 依赖）
    let body = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        // base 形如 http://127.0.0.1:8001
        let base2 = base.trim_end_matches('/');
        let rest = base2
            .strip_prefix("http://")
            .or_else(|| base2.strip_prefix("https://"))
            .unwrap_or(base2);
        let (host_port, _path) = match rest.split_once('/') {
            Some((hp, _)) => (hp, true),
            None => (rest, false),
        };
        let (host, port) = match host_port.split_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(8001)),
            None => (host_port.to_string(), 8001),
        };
        let mut stream = std::net::TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("推理服务不可达: {e}"))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .ok();
        let req = format!("GET /devices HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&buf);
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        Ok::<String, String>(body)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e))?;
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|e| error_response(ErrorKind::Internal, format!("推理服务响应解析失败: {e}")))?;
    Ok(Json(parsed))
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
    // 实时配额：从 sauce_pool 快照合并（pool 未初始化时为 None）
    let live = {
        let slot = state.sauce_pool.read().await;
        match slot.as_ref() {
            Some(pool) => Some(pool.snapshot().await),
            None => None,
        }
    };
    let masked: Vec<Value> = keys
        .iter()
        .map(|k| {
            let lr = live.as_ref().and_then(|l| l.iter().find(|s| s.name == k.name));
            json!({
                "name": k.name,
                "key_masked": format!("{}...{}", &k.key[..2.min(k.key.len())], &k.key[k.key.len().saturating_sub(2)..]),
                "tier": k.tier,
                "has_key": true,
                "short_remaining": lr.map(|s| s.short_remaining).unwrap_or(0),
                "long_remaining": lr.map(|s| s.long_remaining).unwrap_or(95),
                "cooldown_secs": lr.map(|s| s.cooldown_secs()).unwrap_or(0),
                "daily_paused": lr.map(|s| s.daily_paused).unwrap_or(false),
                "total_requests": lr.map(|s| s.total_requests).unwrap_or(0),
            })
        })
        .collect();
    Ok(Json(json!({ "keys": masked, "count": keys.len() })))
}

/// PUT /api/v1/settings/saucenao-keys/{name}/quota：手动修改当日剩余额度。
/// body `{ "long_remaining": number }`；同步更新运行时 pool（若已初始化）。
async fn set_key_quota(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let Some(v) = req.get("long_remaining").and_then(|v| v.as_i64()) else {
        return Err(error_response(ErrorKind::InvalidInput, "缺少 long_remaining"));
    };
    let long = v.clamp(0, 10000);
    let db = state.db.clone();
    let name2 = name.clone();
    tokio::task::spawn_blocking(move || {
        // 同步写回配置（持久化）
        let mut keys = read_keys(&db).map_err(db_error_response)?;
        if let Some(k) = keys.iter_mut().find(|k| k.name == name2) {
            // SauceNaoKey 无配额字段，配额存于 pool 快照；这里只确保 key 存在
            let _ = k;
        } else {
            return Err(error_response(ErrorKind::NotFound, format!("密钥 {name2} 不存在")));
        }
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    // 更新运行时 pool（若已初始化）
    {
        let pool_arc = {
            let slot = state.sauce_pool.read().await;
            slot.clone()
        };
        if let Some(pool) = pool_arc {
            let snap = pool.snapshot().await;
            if let Some(idx) = snap.iter().position(|s| s.name == name) {
                pool.update(idx, None, Some(long)).await;
                // 手动设置后解除当日停用（用户显式改额度 = 允许继续用）
                pool.force_resume(idx).await;
            }
        }
    }
    Ok(Json(json!({ "ok": true, "name": name, "long_remaining": long })))
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
