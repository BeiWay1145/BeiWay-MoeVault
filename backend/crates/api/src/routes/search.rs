//! 搜索联想接口：/api/v1/search/suggest。
//!
//! danbooru 风格搜索式筛选的联想：输入前缀 →
//! - 标签（名称/中文名前缀匹配，按词频倒序）
//! - 状态（AI 生成 / 非 AI / 溯源状态 / 冗余候选 / 来源），带各自图片数量
//! 前端图库工具栏搜索框消费。

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use moevault_core::models::ImageFilter;
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/search/suggest", get(search_suggest))
}

#[derive(Debug, Deserialize, Default)]
pub struct SuggestParams {
    /// 输入前缀（标签名/中文名；状态关键字：ai/溯源/sauce/冗余/redundant/来源/danbooru…）。
    pub q: Option<String>,
    /// 标签联想条数（默认 8，最大 20）。
    pub limit: Option<i64>,
}

/// 联想状态项：key 供前端映射为筛选条件。
struct StatusSuggest {
    key: &'static str,
    label: &'static str,
    filter: ImageFilter,
}

async fn search_suggest(
    State(state): State<AppState>,
    Query(params): Query<SuggestParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let q = params.q.clone().unwrap_or_default();
    let q_trim = q.trim().to_string();
    let limit = params.limit.unwrap_or(8).clamp(1, 20);

    // ---- 标签联想：前缀匹配 + 词频倒序 ----
    let db_for_tag = db.clone();
    let q_for_tag = q_trim.clone();
    let tags = tokio::task::spawn_blocking(move || db_for_tag.suggest_tags(&q_for_tag, limit))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;

    // ---- 状态联想：关键字匹配 + 计数 ----
    let lower = q_trim.to_lowercase();
    let mut status_defs: Vec<StatusSuggest> = Vec::new();
    // AI 相关
    if lower.starts_with("ai") || q_trim.contains("ai生成") || q_trim.contains("ai图") || q_trim.contains("AI") {
        status_defs.push(StatusSuggest {
            key: "is_ai",
            label: "AI 生成",
            filter: ImageFilter { is_ai: Some(true), ..Default::default() },
        });
        status_defs.push(StatusSuggest {
            key: "not_ai",
            label: "非 AI 生成",
            filter: ImageFilter { is_ai: Some(false), ..Default::default() },
        });
    }
    // 溯源相关
    if lower.starts_with("sauce") || q_trim.contains("溯源") {
        status_defs.push(StatusSuggest {
            key: "sauced",
            label: "已溯源",
            filter: ImageFilter { sauce_status: Some("sauced".into()), ..Default::default() },
        });
        status_defs.push(StatusSuggest {
            key: "unsauced",
            label: "未溯源",
            filter: ImageFilter { sauce_status: Some("unsauced".into()), ..Default::default() },
        });
        status_defs.push(StatusSuggest {
            key: "un-sauced",
            label: "不可溯源",
            filter: ImageFilter { sauce_status: Some("un-sauced".into()), ..Default::default() },
        });
    }
    // 冗余
    if lower.starts_with("redundant") || q_trim.contains("冗余") {
        status_defs.push(StatusSuggest {
            key: "redundant",
            label: "冗余候选",
            filter: ImageFilter { is_redundant: Some(true), ..Default::default() },
        });
    }
    // 打标状态
    if lower.starts_with("tagged") || q_trim.contains("打标") || q_trim.contains("已打标") {
        status_defs.push(StatusSuggest {
            key: "tagged",
            label: "已打标",
            filter: ImageFilter { tagged: Some(true), ..Default::default() },
        });
        status_defs.push(StatusSuggest {
            key: "untagged",
            label: "未打标",
            filter: ImageFilter { tagged: Some(false), ..Default::default() },
        });
    }
    // 来源
    for (key, label, src) in [
        ("source_danbooru", "danbooru 来源", "danbooru"),
        ("source_gelbooru", "gelbooru 来源", "gelbooru"),
        ("source_local", "本地来源", "local"),
    ] {
        if lower.starts_with(src) || q_trim.contains("来源") || lower.starts_with("source") {
            status_defs.push(StatusSuggest {
                key,
                label,
                filter: ImageFilter { source: Some(src.into()), ..Default::default() },
            });
        }
    }

    // 计数（每个状态一次 COUNT，数量很少）
    let statuses = if status_defs.is_empty() {
        Vec::new()
    } else {
        let mut out: Vec<Value> = Vec::new();
        for sd in status_defs {
            let f = sd.filter.clone();
            let db_for_cnt = db.clone();
            let cnt = tokio::task::spawn_blocking(move || db_for_cnt.count_images_filtered("active", &f))
                .await
                .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
                .map_err(db_error_response)?;
            out.push(json!({ "key": sd.key, "label": sd.label, "count": cnt }));
        }
        out
    };

    Ok(Json(json!({
        "query": q_trim,
        "tags": tags,
        "statuses": statuses,
    })))
}
