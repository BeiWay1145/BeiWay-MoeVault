//! moevault-db：SQLite 连接管理、版本化迁移、基础仓储查询。
//!
//! 说明：骨架阶段采用轻量自研迁移器（编译期嵌入 SQL 文件），
//! 避免第三方迁移库与 rusqlite 版本耦合；对外行为与
//! `docs/TECH_DETAILS.md` 第 1.3 节（版本化迁移）一致。

mod migration;

use std::path::Path;
use std::sync::Mutex;

use moevault_core::models::{Image, ImageListItem, ImportBatch, Stats};
use moevault_core::{AppError, ErrorKind};
use rusqlite::{params, Connection, Row};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("数据库错误: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("迁移错误: {0}")]
    Migration(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl From<DbError> for AppError {
    fn from(e: DbError) -> Self {
        AppError::new(ErrorKind::Db, e.to_string())
    }
}

/// SQLite 数据库封装。单连接 + Mutex：SQLite 写串行化，WAL 下读并发足够。
#[derive(Clone)]
pub struct Db {
    conn: std::sync::Arc<Mutex<Connection>>,
}

impl Db {
    /// 打开（或创建）数据库并执行未应用的迁移。
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        // WAL + 常用 PRAGMA（与 docs/TECH_DETAILS.md 第 1.1 节一致）
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;

        let db = Self {
            conn: std::sync::Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 执行所有未应用的迁移（幂等）。
    pub fn migrate(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        migration::run(&conn)
    }

    /// 图库列表（骨架版：按 id 游标分页，status 过滤）。
    ///
    /// 返回 `(items, next_cursor_id)`；next_cursor_id 为 None 表示没有更多。
    pub fn list_images(
        &self,
        status: &str,
        limit: i64,
        cursor_id: Option<i64>,
    ) -> Result<(Vec<ImageListItem>, Option<i64>), DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut stmt = conn.prepare(
            "SELECT id, md5, rel_path, width, height, format, size_bytes,
                    exif_datetime, clarity_score, aesthetic_score,
                    is_redundant, source, imported_at
             FROM images
             WHERE status = ?1 AND (?2 IS NULL OR id > ?2)
             ORDER BY id
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![status, cursor_id, limit], row_to_item)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        let next = if items.len() as i64 == limit {
            items.last().map(|i| i.id)
        } else {
            None
        };
        Ok((items, next))
    }

    /// 指定状态的图片总数。
    pub fn count_images(&self, status: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM images WHERE status = ?1",
            params![status],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 总览统计。
    pub fn stats(&self) -> Result<Stats, DbError> {
        let conn = self.conn.lock().unwrap();
        let total_images =
            conn.query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))?;
        let active_images =
            conn.query_row("SELECT COUNT(*) FROM images WHERE status = 'active'", [], |r| {
                r.get(0)
            })?;
        let recycled_images =
            conn.query_row("SELECT COUNT(*) FROM images WHERE status = 'recycled'", [], |r| {
                r.get(0)
            })?;
        let redundant_candidates =
            conn.query_row("SELECT COUNT(*) FROM images WHERE is_redundant = 1", [], |r| {
                r.get(0)
            })?;
        let total_tags = conn.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0))?;
        Ok(Stats {
            total_images,
            active_images,
            recycled_images,
            redundant_candidates,
            total_tags,
        })
    }

    // ---------- 导入批次 ----------

    /// 创建导入批次，返回新 id。
    pub fn create_import_batch(&self, source_path: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO import_batches (source_path, total, done, failed, duplicate, state, created_at)
             VALUES (?1, 0, 0, 0, 0, 'pending', ?2)",
            params![source_path, now_secs()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 更新批次计数与状态。
    pub fn update_import_batch(
        &self,
        id: i64,
        total: i64,
        done: i64,
        failed: i64,
        duplicate: i64,
        state: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE import_batches SET total = ?1, done = ?2, failed = ?3, duplicate = ?4, state = ?5
             WHERE id = ?6",
            params![total, done, failed, duplicate, state, id],
        )?;
        Ok(())
    }

    fn row_to_batch(r: &Row) -> rusqlite::Result<ImportBatch> {
        Ok(ImportBatch {
            id: r.get(0)?,
            source_path: r.get(1)?,
            total: r.get(2)?,
            done: r.get(3)?,
            failed: r.get(4)?,
            duplicate: r.get(5)?,
            state: r.get(6)?,
            created_at: r.get(7)?,
        })
    }

    /// 查询单个批次。
    pub fn get_import_batch(&self, id: i64) -> Result<Option<ImportBatch>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_path, total, done, failed, duplicate, state, created_at
             FROM import_batches WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], Self::row_to_batch)?;
        Ok(rows.next().transpose()?)
    }

    /// 批次列表（按创建时间倒序）。
    pub fn list_import_batches(&self, limit: i64) -> Result<Vec<ImportBatch>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 200);
        let mut stmt = conn.prepare(
            "SELECT id, source_path, total, done, failed, duplicate, state, created_at
             FROM import_batches ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], Self::row_to_batch)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ---------- 图片写入 ----------

    /// 判断 md5 是否已存在。
    pub fn md5_exists(&self, md5: &str) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM images WHERE md5 = ?1)",
            params![md5],
            |r| r.get(0),
        )?;
        Ok(n != 0)
    }

    /// 批量插入图片（单事务，失败整体回滚）。
    pub fn insert_images(&self, images: &[Image]) -> Result<(), DbError> {
        if images.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO images
                 (md5, phash, rel_path, width, height, format, size_bytes, file_mtime,
                  exif_datetime, clarity_score, aesthetic_score, dedup_group, is_redundant,
                  status, source, source_url, thumb_rel, imported_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            )?;
            for img in images {
                stmt.execute(params![
                    img.md5,
                    img.phash,
                    img.rel_path,
                    img.width,
                    img.height,
                    img.format,
                    img.size_bytes,
                    img.file_mtime,
                    img.exif_datetime,
                    img.clarity_score,
                    img.aesthetic_score,
                    img.dedup_group,
                    img.is_redundant as i64,
                    img.status,
                    img.source,
                    img.source_url,
                    img.thumb_rel,
                    img.imported_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn row_to_item(r: &Row) -> rusqlite::Result<ImageListItem> {
    Ok(ImageListItem {
        id: r.get(0)?,
        md5: r.get(1)?,
        rel_path: r.get(2)?,
        width: r.get(3)?,
        height: r.get(4)?,
        format: r.get(5)?,
        size_bytes: r.get(6)?,
        exif_datetime: r.get(7)?,
        clarity_score: r.get(8)?,
        aesthetic_score: r.get(9)?,
        is_redundant: r.get::<_, i64>(10)? != 0,
        source: r.get(11)?,
        imported_at: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        let path = std::env::temp_dir().join(format!(
            "moevault_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).expect("打开测试库失败");
        let _ = std::fs::remove_file(&path);
        db
    }

    #[test]
    fn migrate_is_idempotent() {
        let db = test_db();
        // 二次迁移幂等
        db.migrate().expect("重复迁移失败");
    }

    #[test]
    fn stats_empty_library() {
        let db = test_db();
        let s = db.stats().expect("统计失败");
        assert_eq!(s.total_images, 0);
        assert_eq!(s.active_images, 0);
        assert_eq!(s.redundant_candidates, 0);
        assert_eq!(s.total_tags, 0);
    }

    #[test]
    fn list_images_empty_and_filtered() {
        let db = test_db();
        let (items, next) = db.list_images("active", 100, None).expect("列表失败");
        assert!(items.is_empty());
        assert_eq!(next, None);
        let n = db.count_images("active").expect("计数失败");
        assert_eq!(n, 0);
    }
}
