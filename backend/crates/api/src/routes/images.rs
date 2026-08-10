//! 图库接口：GET /api/v1/images、GET /api/v1/stats。
//!
//! 骨架版实现基础游标分页 + status 过滤；完整筛选（标签/日期/质量/来源）
//! 按 docs/TECH_DETAILS.md 第 2.1 节在 M2+ 补全。

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use moevault_core::models::{ImageFilter, Page, SortKey, Stats, STATUS_ACTIVE};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/images", get(list_images))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/images/{id}/recycle", post(recycle_image))
        .route("/api/v1/images/{id}/sidecar", post(generate_sidecar))
}

/// POST /api/v1/images/{id}/sidecar：生成 sidecar .txt（逗号分隔标签，与 cl_tagger 格式一致）。
async fn generate_sidecar(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let library_dir = state.library_dir();

    let result = tokio::task::spawn_blocking(move || {
        // 1. 取图片 rel_path
        let img = db
            .get_image_by_id(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
        // 2. 取标签（所有来源，按名字逗号拼接）
        let tags = db.image_tags(id).map_err(db_error_response)?;
        let tag_str: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
        let content = tag_str.join(", ");
        // 3. 写同名 .txt（图片旁）
        let src_path = library_dir.join(&img.rel_path);
        let txt_path = src_path.with_extension("txt");
        std::fs::write(&txt_path, content)
            .map_err(|e| error_response(ErrorKind::Internal, format!("写入失败: {e}")))?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(txt_path.display().to_string())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;

    Ok(Json(json!({ "ok": true, "path": result })))
}

#[derive(Debug, Deserialize)]
pub struct RecycleRequest {
    /// duplicate / manual / auto。
    pub reason: Option<String>,
}

/// POST /api/v1/images/{id}/recycle：把单张图片移入回收站。
async fn recycle_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<RecycleRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let reason = req.reason.unwrap_or_else(|| "manual".to_string());
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    tokio::task::spawn_blocking(move || {
        moevault_dedup::recycle_image(&db, id, &reason, &library, &recycle)
    })
    .await
    .map_err(join_error_response)?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "recycled": id })))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListParams {
    /// active / recycled，默认 active。
    pub status: Option<String>,
    /// 每页条数，默认 100，最大 500。
    pub limit: Option<i64>,
    /// 游标：上一页返回的 next_cursor（数字 id 字符串）。
    pub cursor: Option<String>,
    /// 关键字（文件名 LIKE）。
    pub q: Option<String>,
    /// 包含标签（逗号分隔，AND 语义）。
    pub tags: Option<String>,
    /// 排除标签（逗号分隔）。
    pub exclude_tags: Option<String>,
    /// 日期范围（epoch 秒）。
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    /// 美学分范围（1-5）。
    pub aesthetic_min: Option<f64>,
    pub aesthetic_max: Option<f64>,
    /// 清晰度范围。
    pub clarity_min: Option<f64>,
    pub clarity_max: Option<f64>,
    /// 来源（danbooru/gelbooru/local）。
    pub source: Option<String>,
    /// 格式（jpg/png/webp）。
    pub format: Option<String>,
    /// 最小宽/高。
    pub min_width: Option<i64>,
    pub min_height: Option<i64>,
    /// 只看冗余候选（1/0/true/false）。
    pub is_redundant: Option<String>,
    /// 排序键：imported/date/aesthetic/clarity/size/random。
    pub sort: Option<String>,
    /// asc/desc。
    pub order: Option<String>,
}

impl ListParams {
    fn parse_sort(&self) -> Result<SortKey, (axum::http::StatusCode, Json<Value>)> {
        let s = self.sort.as_deref().unwrap_or("imported");
        let key = match s {
            "imported" => SortKey::Imported,
            "date" => SortKey::Date,
            "aesthetic" => SortKey::Aesthetic,
            "clarity" => SortKey::Clarity,
            "size" => SortKey::Size,
            "random" => SortKey::Random,
            _ => {
                return Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("sort 仅支持 imported/date/aesthetic/clarity/size/random，收到: {s}"),
                ))
            }
        };
        Ok(key)
    }

    fn parse_redundant(&self) -> Result<Option<bool>, (axum::http::StatusCode, Json<Value>)> {
        match &self.is_redundant {
            None => Ok(None),
            Some(v) if v.trim().is_empty() => Ok(None),
            Some(v) => match v.to_lowercase().as_str() {
                "1" | "true" | "yes" => Ok(Some(true)),
                "0" | "false" | "no" => Ok(Some(false)),
                _ => Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("is_redundant 仅支持 1/0/true/false，收到: {v}"),
                )),
            },
        }
    }

    fn split_tags(s: &Option<String>) -> Vec<String> {
        s.as_deref()
            .map(|v| {
                v.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn build_filter(&self) -> Result<ImageFilter, (axum::http::StatusCode, Json<Value>)> {
        Ok(ImageFilter {
            q: self.q.clone(),
            tags: Self::split_tags(&self.tags),
            exclude_tags: Self::split_tags(&self.exclude_tags),
            date_from: self.date_from,
            date_to: self.date_to,
            aesthetic_min: self.aesthetic_min,
            aesthetic_max: self.aesthetic_max,
            clarity_min: self.clarity_min,
            clarity_max: self.clarity_max,
            source: self.source.clone(),
            format: self.format.clone(),
            min_width: self.min_width,
            min_height: self.min_height,
            is_redundant: self.parse_redundant()?,
        })
    }
}

async fn list_images(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let status = params.status.clone().unwrap_or_else(|| STATUS_ACTIVE.to_string());
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
    let filter = params.build_filter()?;
    let sort = params.parse_sort()?;
    let sort_asc = params.order.as_deref() == Some("asc");
    let (items, next_id) = tokio::task::spawn_blocking(move || {
        db.list_images_filtered(&status_for_query, &filter, sort, sort_asc, limit, cursor_id)
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
