//! moevault-ingest：图片导入与索引。
//!
//! 职责（对应 docs/PLAN.md M2）：
//! - 扫描源路径（文件/目录）收集图片
//! - 提取特征：MD5、pHash、清晰度、尺寸、格式、EXIF 日期
//! - 移动进库（哈希分片）+ 生成缩略图 + 写入 SQLite
//! - 导入批次进度管理

pub mod clarity;
pub mod exif;
pub mod features;
pub mod importer;
pub mod phash;
pub mod scan;

pub use features::{extract_features, ImageFeatures};
pub use importer::{run_import, ImportProgress};
pub use scan::collect_images;

/// 支持的图片扩展名（小写）。
pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

/// 导入/索引错误。
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("图像解码失败 {path}: {source}")]
    Image {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("数据库错误: {0}")]
    Db(#[from] moevault_db::DbError),
    #[error("无效输入: {0}")]
    Invalid(String),
}
