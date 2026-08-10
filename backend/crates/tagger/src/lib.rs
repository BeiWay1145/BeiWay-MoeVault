//! moevault-tagger：自动打标流水线。
//!
//! 流程（docs/PLAN.md 2.3）：
//! 1. SauceNAO 溯源（multipart 上传本地图片）
//! 2. 有效判定：相似度 ≥ 阈值 且 ext_urls 含 danbooru/gelbooru 链接
//! 3. 爬取标签（danbooru 官方 API / gelbooru dapi）
//! 4. 溯源失败 → 回退本地 cl_tagger（推理服务 HTTP）

pub mod booru;
pub mod keypool;
pub mod pipeline;
pub mod saucenao;

pub use keypool::{ApiKeyPool, KeyState};
pub use pipeline::{run_tag_pipeline, InferClient, TagProgress};
pub use saucenao::{SauceNaoClient, SauceNaoResult};

/// 打标错误。
#[derive(Debug, thiserror::Error)]
pub enum TaggerError {
    #[error("HTTP 请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] moevault_db::DbError),
    #[error("无效输入: {0}")]
    Invalid(String),
    #[error("溯源未命中: {0}")]
    NoSource(String),
    #[error("限流中: 剩余 {0} 次")]
    RateLimited(i64),
}
