//! 领域模型：与 `docs/TECH_DETAILS.md` 第 1 节 SQLite 表结构对应。

use serde::{Deserialize, Serialize};

/// 图片状态。
pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_RECYCLED: &str = "recycled";

/// 标签来源。
pub const SOURCE_LOCAL: &str = "local";
pub const SOURCE_DANBOORU: &str = "danbooru";
pub const SOURCE_GELEOORU: &str = "gelbooru";

/// 图片元数据（images 表一行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub id: i64,
    pub md5: String,
    pub phash: i64,
    pub rel_path: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    pub size_bytes: i64,
    pub file_mtime: i64,
    pub exif_datetime: Option<i64>,
    pub clarity_score: f64,
    pub aesthetic_score: Option<f64>,
    pub dedup_group: Option<i64>,
    pub is_redundant: bool,
    pub status: String,
    pub source: String,
    pub source_url: Option<String>,
    /// 不可自动溯源标记（确认无法溯源到 booru 的图，自动打标跳过）。
    pub no_auto_sauce: bool,
    pub thumb_rel: String,
    pub imported_at: i64,
}

/// 标签（tags 表一行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub name_cn: Option<String>,
    pub category: String,
    pub is_custom: bool,
    pub is_blacklisted: bool,
}

/// 图片-标签关联。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageTag {
    pub image_id: i64,
    pub tag_id: i64,
    pub source: String,
    pub confidence: Option<f64>,
    pub created_at: i64,
}

/// 图库列表项（/api/v1/images 返回的轻量结构，不含 phash 等内部字段）。
#[derive(Debug, Clone, Serialize)]
pub struct ImageListItem {
    pub id: i64,
    pub md5: String,
    pub rel_path: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    pub size_bytes: i64,
    pub exif_datetime: Option<i64>,
    pub clarity_score: f64,
    pub aesthetic_score: Option<f64>,
    pub is_redundant: bool,
    pub source: String,
    pub imported_at: i64,
}

/// 总览统计（/api/v1/stats）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    pub total_images: i64,
    pub active_images: i64,
    pub recycled_images: i64,
    pub redundant_candidates: i64,
    pub total_tags: i64,
}

/// 列表分页响应。
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 游标分页：null 表示没有更多。
    pub next_cursor: Option<String>,
    pub total: i64,
}

/// 导入批次（import_batches 表一行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBatch {
    pub id: i64,
    pub source_path: String,
    pub total: i64,
    pub done: i64,
    pub failed: i64,
    /// 重复跳过计数（md5 已存在）。
    pub duplicate: i64,
    pub state: String,
    pub created_at: i64,
}

/// 查重组列表项（/api/v1/dedup/groups 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct DedupGroupSummary {
    pub id: i64,
    /// 组内 active 成员数。
    pub size: i64,
    /// 冗余候选数（active 非最优）。
    pub redundant_count: i64,
    /// 最优图（清晰度最高）信息。
    pub best_id: Option<i64>,
    pub best_thumb_rel: Option<String>,
    pub best_clarity: Option<f64>,
}

/// 查重组详情（/api/v1/dedup/groups/{id} 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct DedupGroupDetail {
    pub id: i64,
    pub state: String,
    pub members: Vec<GroupMember>,
}

/// 查重组成员。
#[derive(Debug, Clone, Serialize)]
pub struct GroupMember {
    pub image_id: i64,
    pub rel_path: String,
    pub thumb_rel: String,
    pub width: i64,
    pub height: i64,
    pub clarity_score: f64,
    pub aesthetic_score: Option<f64>,
    pub is_redundant: bool,
    pub is_best: bool,
}

/// 回收站项（/api/v1/trash 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct RecycledItem {
    pub image_id: i64,
    pub rel_path: String,
    pub thumb_rel: String,
    pub reason: String,
    pub original_rel: String,
    pub deleted_at: i64,
}

/// 查重统计（/api/v1/dedup/stats 返回）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct DedupStats {
    pub group_count: i64,
    /// 参与查重的图片数（有组归属的 active 图）。
    pub involved_images: i64,
    pub redundant_count: i64,
}

/// 图片-标签关联（含标签信息，供详情页展示）。
#[derive(Debug, Clone, Serialize)]
pub struct ImageTagView {
    pub tag_id: i64,
    pub name: String,
    pub name_cn: Option<String>,
    pub category: String,
    /// 来源：danbooru/gelbooru/local/manual。
    pub source: String,
    pub confidence: Option<f64>,
}

/// 单图打标状态。
#[derive(Debug, Clone, Serialize)]
pub struct TaggingState {
    pub image_id: i64,
    pub tagged: bool,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub tag_count: usize,
}
