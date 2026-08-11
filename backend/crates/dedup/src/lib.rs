//! moevault-dedup：图片查重（pHash 聚类 + 模糊/清晰对判定）与回收站。
//!
//! 职责（对应 docs/PLAN.md M3）：
//! - pHash 聚类：增量（新图入现有簇/开新簇）与全量（清空重建）
//! - 簇内 refresh：按清晰度选 best，其余标记冗余候选（is_redundant）
//! - 回收站：软删除（文件移入 recycle/ + 元数据保留）、恢复、永久删除

pub mod cluster;
pub mod recycle;

pub use cluster::{cluster_scope, full_recluster, incremental_cluster, ClusterStats, DEFAULT_HAMMING_THRESHOLD};
pub use recycle::{purge_all, purge_image, recycle_image, restore_image};

/// 查重/回收站错误。
#[derive(Debug, thiserror::Error)]
pub enum DedupError {
    #[error("数据库错误: {0}")]
    Db(#[from] moevault_db::DbError),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("无效输入: {0}")]
    Invalid(String),
    #[error("未找到: {0}")]
    NotFound(String),
}
