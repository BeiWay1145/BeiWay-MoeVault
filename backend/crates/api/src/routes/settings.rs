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

use super::{db_error_response, error_response, ApiResult, ApiError};

/// 可写设置白名单（避免任意 key 写入）。
const SETTINGS_WHITELIST: &[&str] = &[
    "saucenao_min_sim",
    "tag_threshold",
    "tagger_model_dir",
    "tagger_model_name",
    "tagger_model_kind",
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
    "tag_cover_rule",
    "tag_show_cn_first",
    "import_default_mode",
    "export_open_explorer_batch",
    "export_open_explorer_manual",
    "export_default_dir",
    "aesthetic_kind",
    // 增强2：SauceNAO 多出口 IP 轮换（Clash 多入站端口）
    "sauce_proxy_enabled",
    "sauce_proxy_ports",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings", get(get_settings).put(update_settings))
        .route("/api/v1/settings/saucenao-keys", get(list_keys).post(add_key))
        .route("/api/v1/settings/saucenao-keys/{name}", delete(delete_key))
        .route("/api/v1/settings/saucenao-keys/{name}/quota", put(set_key_quota))
        .route("/api/v1/settings/sauce-exits", get(list_exits).post(test_exits))
        .route("/api/v1/devices", get(proxy_devices))
        .route("/api/v1/infer/health", get(proxy_infer_health))
}

/// 转发一个 GET 请求到 Python 推理服务并返回响应 body（/health、/devices 用）。
///
/// 这里曾是一段手写裸 TCP 客户端（自己拼 HTTP/1.1 请求行、按 \r\n\r\n 切 body），
/// 理由是「api crate 无 reqwest 依赖」—— 那条注释早就过时了，Cargo.toml 里 reqwest
/// 从多出口功能起就在依赖里。手写实现不处理 chunked、gzip、非 200 状态、
/// 响应头大小写，改用 reqwest 后这些问题自然消失，也能复用连接池。
async fn proxy_python_get(base: &str, path: &str) -> Result<String, String> {
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("推理服务不可达（{url}）: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("读取推理服务响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("推理服务返回 HTTP {status}: {body}"));
    }
    Ok(body)
}

/// GET /api/v1/infer/health：转发到 Python 推理服务 /health（模型加载状态 + 当前模型路径）。
async fn proxy_infer_health(
    State(state): State<AppState>,
) -> ApiResult<Value> {
    proxy_json(&state, "/health").await
}

/// GET /api/v1/devices：转发到 Python 推理服务 /devices（获取可用推理设备）。
async fn proxy_devices(
    State(state): State<AppState>,
) -> ApiResult<Value> {
    proxy_json(&state, "/devices").await
}

/// 通用：代理 Python GET 并解析 JSON（/health、/devices 共用）。
async fn proxy_json(
    state: &AppState,
    path: &str,
) -> ApiResult<Value> {
    let base = state.infer_base_url.clone();
    let path = path.to_string();
    let body = proxy_python_get(&base, &path)
        .await
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
            ..Default::default()
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
) -> ApiResult<Value> {
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
) -> ApiResult<Value> {
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
        Ok::<_, ApiError>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;

    // 增强2：设置保存后重建出口池（端口列表/开关可能已变化）。
    // 重建是廉价操作（只是构造几个绑定代理的 Client），故每次保存都做，
    // 保证运行中的溯源任务立即使用新配置，无需重启。
    let db2 = state.db.clone();
    let (ports, enabled) = tokio::task::spawn_blocking(move || crate::state::exit_config(&db2))
        .await
        .unwrap_or_default();
    let pool = moevault_tagger::ExitPool::new(&ports, enabled);
    {
        let mut slot = state.exit_pool.write().await;
        if enabled && !ports.is_empty() {
            tracing::info!(?ports, "SauceNAO 多出口已启用");
            *slot = Some(std::sync::Arc::new(pool));
        } else {
            *slot = None;
        }
    }

    // 关键：出口窗口是按**出口 IP** 计的（各 4 次/30 秒），
    // 而 keypool 的"全局窗口"原按**单一 IP** 设计（4 次/30 秒）。
    // 启用多出口后若不放大该上限，调度仍被 4 次/30 秒 卡住，
    // 多出口完全发挥不出来（表现为"一次还是只有 4 个"）。
    // 故按出口数等比放宽：N 个出口 → N × 4 次 / 30 秒。
    {
        let slot = state.sauce_pool.read().await;
        if let Some(kp) = slot.as_ref() {
            let n = if enabled && !ports.is_empty() { ports.len().max(1) } else { 1 };
            let limit = moevault_tagger::keypool::DEFAULT_GLOBAL_LIMIT * n;
            kp.set_global_limit(limit).await;
            tracing::info!(limit, exits = n, "已按出口数放宽 SauceNAO 全局窗口上限");
        }
    }

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
) -> ApiResult<Value> {
    let db = state.db.clone();
    let keys = tokio::task::spawn_blocking(move || read_keys(&db))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    // 实时配额：从 sauce_pool 快照合并（pool 未初始化时为 None）。
    //
    // 若 pool 尚未初始化（例如刚启动、还没跑过溯源任务），
    // 则**从持久化快照文件恢复一次**——pool 每次溯源后都会 save()，
    // 这样前端在进程重启后依然能看到真实的剩余额度，
    // 而不是永远显示初始值 95（BUG1：额度计数不减少）。
    let live = {
        let slot = state.sauce_pool.read().await;
        match slot.as_ref() {
            Some(pool) => Some(pool.snapshot().await),
            None => {
                let persist = state.data_dir.join("sauce_keys.json");
                moevault_tagger::keypool::ApiKeyPool::load_snapshot(&persist)
            }
        }
    };
    // 全局 IP 池窗口状态（免费账号 4 次 / 30 秒，跨 key 共享）
    let rate_window = {
        let slot = state.sauce_pool.read().await;
        match slot.as_ref() {
            Some(pool) => {
                let (used, limit, cooling) = pool.global_window().await;
                Some(json!({
                    "used": used,
                    "limit": limit,
                    "cooling_secs": cooling,
                    "window_secs": moevault_tagger::keypool::GLOBAL_WINDOW_SECS,
                }))
            }
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
                // 实时态优先；无实时态（进程重启后 pool 重建）时**回退到持久化配置**，
                // 而不是硬编码默认值 —— 否则前端永远显示 95，
                // 表现为"溯源后额度计数不减少"（BUG1）。
                "short_remaining": lr
                    .map(|s| s.short_remaining)
                    .or(k.short_remaining)
                    .unwrap_or(0),
                "long_remaining": lr
                    .map(|s| s.long_remaining)
                    .or(k.long_remaining)
                    .unwrap_or(95),
                "cooldown_secs": lr.map(|s| s.cooldown_secs()).unwrap_or(0),
                "daily_paused": lr.map(|s| s.daily_paused).unwrap_or(false),
                "total_requests": lr.map(|s| s.total_requests).unwrap_or(0),
            })
        })
        .collect();
    Ok(Json(json!({
        "keys": masked,
        "count": keys.len(),
        "rate_window": rate_window,
    })))
}

/// PUT /api/v1/settings/saucenao-keys/{name}/quota：手动修改当日剩余额度。
/// body `{ "long_remaining": number }`；同步更新运行时 pool（若已初始化）。
async fn set_key_quota(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> ApiResult<Value> {
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
        Ok::<_, ApiError>(())
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
) -> ApiResult<Value> {
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
        keys.push(SauceNaoKey { name: name.clone(), key, tier, ..Default::default() });
        write_keys(&db, &keys).map_err(db_error_response)?;
        Ok::<String, ApiError>(name)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "name": name })))
}

async fn delete_key(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut keys = read_keys(&db).map_err(db_error_response)?;
        keys.retain(|k| k.name != name);
        write_keys(&db, &keys).map_err(db_error_response)?;
        Ok::<_, ApiError>(())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true })))
}

// ---------- 增强2：SauceNAO 多出口 IP 轮换（Clash 多入站端口）----------

/// GET /api/v1/settings/sauce-exits：返回出口配置与各出口的实时状态。
async fn list_exits(
    State(state): State<AppState>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    let (ports, enabled) = tokio::task::spawn_blocking(move || crate::state::exit_config(&db))
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?;

    let live = {
        let slot = state.exit_pool.read().await;
        match slot.as_ref() {
            Some(pool) => Some(pool.snapshot().await),
            None => None,
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let exits: Vec<Value> = match live {
        Some(states) => states
            .iter()
            .map(|s| {
                json!({
                    "port": s.port,
                    "exit_ip": s.exit_ip,
                    "total_requests": s.total_requests,
                    "disabled": s.disabled,
                    "cooldown_secs": s.cooldown_until.saturating_sub(now),
                    "consecutive_failures": s.consecutive_failures,
                })
            })
            .collect(),
        None => ports
            .iter()
            .map(|p| {
                json!({
                    "port": p,
                    "exit_ip": Value::Null,
                    "total_requests": 0,
                    "disabled": false,
                    "cooldown_secs": 0,
                    "consecutive_failures": 0,
                })
            })
            .collect(),
    };

    Ok(Json(json!({ "enabled": enabled, "ports": ports, "exits": exits })))
}

/// POST /api/v1/settings/sauce-exits：测试所有出口（探测各自出口 IP 与延迟）。
///
/// 若多个端口返回**相同出口 IP**，说明 Clash 配置未生效（各端口走了同一节点），
/// 此时轮换无意义 —— 前端据此给出明确警告。
async fn test_exits(
    State(state): State<AppState>,
) -> ApiResult<Value> {
    let db = state.db.clone();
    let (ports, _) = tokio::task::spawn_blocking(move || crate::state::exit_config(&db))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?;
    if ports.is_empty() {
        return Err(error_response(
            ErrorKind::InvalidInput,
            "尚未配置出口端口，请先在设置中填写 Clash 的入站端口号",
        ));
    }
    let pool = moevault_tagger::ExitPool::new(&ports, true);
    let results = pool.probe_all().await;
    let items: Vec<Value> = results
        .iter()
        .map(|(port, ip, note)| json!({ "port": port, "exit_ip": ip, "note": note }))
        .collect();
    let mut ips: Vec<String> = results.iter().filter_map(|(_, ip, _)| ip.clone()).collect();
    ips.sort();
    let before = ips.len();
    ips.dedup();
    let duplicate_ips = ips.len() < before;
    Ok(Json(json!({
        "exits": items,
        "duplicate_ips": duplicate_ips,
        "hint": if duplicate_ips {
            "检测到多个出口使用相同 IP —— Clash 的 listeners 可能未生效（各端口走了同一节点），轮换无意义"
        } else { "" },
    })))
}
