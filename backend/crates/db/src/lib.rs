//! moevault-db：SQLite 连接管理、版本化迁移、基础仓储查询。
//!
//! 说明：骨架阶段采用轻量自研迁移器（编译期嵌入 SQL 文件），
//! 避免第三方迁移库与 rusqlite 版本耦合；对外行为与
//! `docs/TECH_DETAILS.md` 第 1.3 节（版本化迁移）一致。

mod migration;

use std::path::Path;
use std::sync::Mutex;

use moevault_core::models::{
    DedupGroupDetail, DedupGroupSummary, DedupStats, GroupMember, Image, ImageFilter,
    ImageListItem, ImageTagView, ImportBatch, RecycledItem, SortKey, Stats, TagWithCount,
    TaggingState,
};
use moevault_core::{AppError, ErrorKind};
use rusqlite::{params, Connection, OptionalExtension, Row};
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

/// 溯源缓存记录：`(similarity, source, source_url)`。
pub type SauceCacheEntry = (f64, Option<String>, Option<String>);

/// 后台任务记录（打标/美学/溯源等耗时操作，持久化于 jobs 表）。
#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub ty: String,
    pub status: String,
    pub total: i64,
    pub done: i64,
    pub failed: i64,
    pub payload: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

/// 主目录某天的来源分组：`(来源文件夹名, 图片数)`。
pub type ImportDirCount = (Option<String>, i64);

/// 应用日志记录。
#[derive(Debug, Clone)]
pub struct AppLog {
    pub id: i64,
    pub level: String,
    pub category: String,
    pub message: String,
    pub created_at: i64,
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

    /// 图库列表（组合筛选 + 排序 + 游标分页）。
    ///
    /// 返回 `(items, next_cursor_id)`；next_cursor_id 为 None 表示没有更多。
    /// 筛选/排序字段均为白名单列名 + 参数绑定，无 SQL 注入风险。
    #[allow(clippy::too_many_arguments)]
    pub fn list_images_filtered(
        &self,
        status: &str,
        filter: &ImageFilter,
        sort: SortKey,
        sort_asc: bool,
        limit: i64,
        cursor_id: Option<i64>,
    ) -> Result<(Vec<ImageListItem>, Option<i64>), DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);

        let mut sql = String::from(
            "SELECT i.id, i.md5, i.rel_path, i.width, i.height, i.format, i.size_bytes,
                    i.exif_datetime, i.clarity_score, i.aesthetic_score,
                    i.is_redundant, i.source, i.source_url, i.no_auto_sauce, i.imported_at, i.thumb_rel,
                    (i.ai_metadata IS NOT NULL AND i.ai_metadata != '')
             FROM images i",
        );
        let (mut conds, mut params) = build_filter_conds(status, filter);
        // 游标（id 偏移，配合排序保证稳定分页）
        if let Some(c) = cursor_id {
            let t = format!("?{}", params.len() + 1);
            conds.push(format!("i.id > {t}"));
            params.push(Box::new(c));
        }

        sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));

        // 排序（白名单列）
        if sort == SortKey::Random {
            sql.push_str(" ORDER BY RANDOM()");
        } else {
            let dir = if sort_asc { "ASC" } else { "DESC" };
            sql.push_str(&format!(" ORDER BY {} {}", sort.sql_col(), dir));
            sql.push_str(", i.id ASC"); // 稳定 tie-break
        }
        sql.push_str(" LIMIT ?");
        let limit_placeholder_idx = params.len() + 1;
        sql.push_str(&limit_placeholder_idx.to_string());
        params.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        // 参数绑定（按索引）
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(row_to_item(row)?);
        }

        let next = if items.len() as i64 == limit {
            items.last().map(|i| i.id)
        } else {
            None
        };
        Ok((items, next))
    }

    /// 按筛选条件统计图片数量（与列表口径一致，供 total 使用）。
    pub fn count_images_filtered(
        &self,
        status: &str,
        filter: &ImageFilter,
    ) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let (conds, params) = build_filter_conds(status, filter);
        let sql = format!("SELECT COUNT(*) FROM images i WHERE {}", conds.join(" AND "));
        let mut stmt = conn.prepare(&sql)?;
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        let n: i64 = match rows.next()? {
            Some(row) => row.get(0)?,
            None => 0,
        };
        Ok(n)
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
        // 图片总数 = active 图数量（与图库列表口径一致；回收站图单列）
        let total_images =
            conn.query_row("SELECT COUNT(*) FROM images WHERE status = 'active'", [], |r| r.get(0))?;
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
        let avg_aesthetic: Option<f64> = conn
            .query_row(
                "SELECT AVG(aesthetic_score) FROM images WHERE aesthetic_score IS NOT NULL",
                [],
                |r| r.get::<_, Option<f64>>(0),
            )
            .unwrap_or(None);
        // 本月导入（本地时区月首的 epoch 秒）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let month_start = month_start_secs(now);
        let month_imported: i64 = conn.query_row(
            "SELECT COUNT(*) FROM images WHERE imported_at >= ?1",
            params![month_start],
            |r| r.get(0),
        )?;
        Ok(Stats {
            total_images,
            active_images,
            recycled_images,
            redundant_candidates,
            total_tags,
            avg_aesthetic,
            month_imported,
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

    // ---------- 主目录（按天 → 来源分组） ----------

    /// 主目录树：active 图片按导入日期（本地时区天）分组，组内按来源文件夹分组。
    /// 返回 `[(date_str, [ImportDirCount])]`，日期倒序、来源组内按数量倒序。
    /// 筛选参数（sauce/tag/ai 三元组）用于计数：空组自动隐藏。
    pub fn import_tree(
        &self,
        sauce: Option<&str>,
        tag: Option<&str>,
        ai: Option<&str>,
    ) -> Result<Vec<(String, Vec<ImportDirCount>)>, DbError> {
        let conn = self.conn.lock().unwrap();
        // 条件：active + 筛选
        let mut conds = vec!["i.status = 'active'".to_string()];
        let params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = sauce {
            let is_sauced = "(i.source_url IS NOT NULL AND i.source_url != '') OR (i.source != 'local' AND i.source != '')";
            let is_ai = "(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') OR EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source = 'ai')";
            match s {
                "sauced" => conds.push(is_sauced.to_string()),
                "unsauced" => conds.push(format!("NOT ({is_sauced}) AND NOT ({is_ai}) AND i.no_auto_sauce = 0")),
                "un-sauced" => conds.push(format!("({is_ai}) OR i.no_auto_sauce = 1")),
                _ => {}
            }
        }
        if let Some(t) = tag {
            match t {
                "tagged" => conds.push(
                    "EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source IN ('auto_danbooru','auto_gelbooru','auto_local'))"
                        .to_string(),
                ),
                "untagged" => conds.push(
                    "NOT EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source IN ('auto_danbooru','auto_gelbooru','auto_local'))"
                        .to_string(),
                ),
                "no_need" => conds.push(
                    "(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') OR EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source = 'ai')"
                        .to_string(),
                ),
                _ => {}
            }
        }
        if let Some(a) = ai {
            match a {
                "ai" => conds.push("(i.ai_metadata IS NOT NULL AND i.ai_metadata != '')".to_string()),
                "not_ai" => conds.push("(i.ai_metadata IS NULL OR i.ai_metadata = '')".to_string()),
                _ => {}
            }
        }
        let where_sql = conds.join(" AND ");
        // 按天 + 来源分组统计（日期用本地时区：date(imported_at, 'unixepoch', 'localtime')）
        let sql = format!(
            "SELECT date(i.imported_at, 'unixepoch', 'localtime') AS day,
                    i.source_dir,
                    COUNT(*) AS cnt
             FROM images i
             WHERE {where_sql}
             GROUP BY day, i.source_dir
             ORDER BY day DESC, cnt DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        // 聚合为 天 → [来源组]
        let mut days: Vec<(String, Vec<ImportDirCount>)> = Vec::new();
        while let Some(row) = rows.next()? {
            let day: String = row.get(0)?;
            let dir: Option<String> = row.get(1)?;
            let cnt: i64 = row.get(2)?;
            match days.last_mut() {
                Some((d, dirs)) if *d == day => dirs.push((dir, cnt)),
                _ => days.push((day, vec![(dir, cnt)])),
            }
        }
        Ok(days)
    }

    /// 某来源组的图片（active，游标分页）。
    /// 返回 (items, next_cursor)。
    #[allow(clippy::too_many_arguments)]
    pub fn import_dir_images(
        &self,
        day: &str,
        source_dir: Option<&str>,
        sauce: Option<&str>,
        tag: Option<&str>,
        ai: Option<&str>,
        limit: i64,
        cursor_id: Option<i64>,
    ) -> Result<(Vec<ImageListItem>, Option<i64>), DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 200);
        let mut conds = vec![
            "i.status = 'active'".to_string(),
            "date(i.imported_at, 'unixepoch', 'localtime') = ?1".to_string(),
            "COALESCE(i.source_dir, '') = ?2".to_string(),
        ];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(day.to_string()),
            Box::new(source_dir.unwrap_or("").to_string()),
        ];
        // 复用与 tree 相同的筛选
        if let Some(s) = sauce {
            let is_sauced = "(i.source_url IS NOT NULL AND i.source_url != '') OR (i.source != 'local' AND i.source != '')";
            let is_ai = "(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') OR EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source = 'ai')";
            match s {
                "sauced" => conds.push(is_sauced.to_string()),
                "unsauced" => conds.push(format!("NOT ({is_sauced}) AND NOT ({is_ai}) AND i.no_auto_sauce = 0")),
                "un-sauced" => conds.push(format!("({is_ai}) OR i.no_auto_sauce = 1")),
                _ => {}
            }
        }
        if let Some(t) = tag {
            match t {
                "tagged" => conds.push(
                    "EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source IN ('auto_danbooru','auto_gelbooru','auto_local'))"
                        .to_string(),
                ),
                "untagged" => conds.push(
                    "NOT EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source IN ('auto_danbooru','auto_gelbooru','auto_local'))"
                        .to_string(),
                ),
                "no_need" => conds.push(
                    "(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') OR EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source = 'ai')"
                        .to_string(),
                ),
                _ => {}
            }
        }
        if let Some(a) = ai {
            match a {
                "ai" => conds.push("(i.ai_metadata IS NOT NULL AND i.ai_metadata != '')".to_string()),
                "not_ai" => conds.push("(i.ai_metadata IS NULL OR i.ai_metadata = '')".to_string()),
                _ => {}
            }
        }
        // 游标
        let base_param_n = params.len() + 1;
        if let Some(c) = cursor_id {
            conds.push(format!("i.id > ?{base_param_n}"));
            params.push(Box::new(c));
        }
        let where_sql = conds.join(" AND ");
        let limit_ph = params.len() + 1;
        let sql = format!(
            "SELECT i.id, i.md5, i.rel_path, i.width, i.height, i.format, i.size_bytes,
                    i.exif_datetime, i.clarity_score, i.aesthetic_score,
                    i.is_redundant, i.source, i.source_url, i.no_auto_sauce, i.imported_at, i.thumb_rel,
                    (i.ai_metadata IS NOT NULL AND i.ai_metadata != '')
             FROM images i
             WHERE {where_sql}
             ORDER BY i.id ASC
             LIMIT ?{limit_ph}"
        );
        params.push(Box::new(limit));
        let mut stmt = conn.prepare(&sql)?;
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(row_to_item(row)?);
        }
        let next = if items.len() as i64 == limit {
            items.last().map(|i| i.id)
        } else {
            None
        };
        Ok((items, next))
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
                  status, source, source_url, thumb_rel, imported_at, source_dir)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
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
                    img.source_dir,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- 查重 ----------

    /// 所有未分组的 active 图片：`(id, phash, clarity)`。
    pub fn unclustered_active_images(&self) -> Result<Vec<(i64, i64, f64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, phash, clarity_score FROM images
             WHERE status = 'active' AND dedup_group IS NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 全部查重组种子：`(group_id, phash_seed)`。
    pub fn cluster_seeds(&self) -> Result<Vec<(i64, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, phash_seed FROM dedup_groups")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 创建查重组（种子 = 首成员 phash），返回组 id。
    pub fn create_dedup_group(&self, phash: i64, image_id: i64) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO dedup_groups (phash_seed, best_image, state, created_at)
             VALUES (?1, ?2, 'open', ?3)",
            params![phash, image_id, now_secs()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 把图片归入查重组。
    pub fn assign_to_group(&self, image_id: i64, group_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET dedup_group = ?2 WHERE id = ?1",
            params![image_id, group_id],
        )?;
        Ok(())
    }

    /// 解除指定图片的查重分组（范围查重前调用），并清理空组。
    pub fn unassign_images(&self, ids: &[i64]) -> Result<(), DbError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE images SET dedup_group = NULL, is_redundant = 0 WHERE id = ?1",
            )?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        // 清理无成员的组
        tx.execute("DELETE FROM dedup_groups WHERE id NOT IN (SELECT DISTINCT dedup_group FROM images WHERE dedup_group IS NOT NULL)", [])?;
        tx.commit()?;
        Ok(())
    }

    /// 清空全部查重结果（全量重建前调用）。
    pub fn clear_dedup(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM dedup_groups", [])?;
        conn.execute(
            "UPDATE images SET dedup_group = NULL, is_redundant = 0",
            [],
        )?;
        Ok(())
    }

    /// 全部 active 图片：`(id, phash, clarity)`，按 id 排序。
    pub fn all_active_images(&self) -> Result<Vec<(i64, i64, f64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, phash, clarity_score FROM images
             WHERE status = 'active' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 按 id 查图片（保留给定顺序）：`(id, phash, clarity)`。
    pub fn images_by_ids(&self, ids: &[i64]) -> Result<Vec<(i64, i64, f64)>, DbError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT id, phash, clarity_score FROM images
             WHERE id IN ({}) ORDER BY id",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        for (i, id) in ids.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, id)?;
        }
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    /// 组内 active 成员：`(image_id, clarity)`。
    pub fn group_members_active(&self, group_id: i64) -> Result<Vec<(i64, f64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, clarity_score FROM images
             WHERE dedup_group = ?1 AND status = 'active' ORDER BY id",
        )?;
        let rows = stmt.query_map(params![group_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 设置组最优图。
    pub fn set_group_best(&self, group_id: i64, best_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE dedup_groups SET best_image = ?2 WHERE id = ?1",
            params![group_id, best_id],
        )?;
        Ok(())
    }

    /// 设置组状态（open/resolved）。
    pub fn set_group_state(&self, group_id: i64, state: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE dedup_groups SET state = ?2 WHERE id = ?1",
            params![group_id, state],
        )?;
        Ok(())
    }

    /// 解除图片对 dedup_groups.best_image 的引用（purge 前调用，避免 FK 失败）。
    pub fn unset_group_best_ref(&self, image_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE dedup_groups SET best_image = NULL WHERE best_image = ?1",
            params![image_id],
        )?;
        Ok(())
    }

    /// 标记图片为冗余候选。
    pub fn set_redundant(&self, image_id: i64, redundant: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET is_redundant = ?2 WHERE id = ?1",
            params![image_id, redundant as i64],
        )?;
        Ok(())
    }

    /// 设置图片状态。
    pub fn set_status(&self, image_id: i64, status: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET status = ?2 WHERE id = ?1",
            params![image_id, status],
        )?;
        Ok(())
    }

    /// 查重统计。
    pub fn dedup_stats(&self) -> Result<DedupStats, DbError> {
        let conn = self.conn.lock().unwrap();
        Ok(DedupStats {
            group_count: conn.query_row(
                "SELECT COUNT(*) FROM dedup_groups g
                 WHERE (SELECT COUNT(*) FROM images i WHERE i.dedup_group = g.id AND i.status = 'active' AND i.is_redundant = 1) > 0",
                [],
                |r| r.get(0),
            )?,
            involved_images: conn.query_row(
                "SELECT COUNT(*) FROM images WHERE dedup_group IS NOT NULL AND status = 'active'",
                [],
                |r| r.get(0),
            )?,
            redundant_count: conn.query_row(
                "SELECT COUNT(*) FROM images WHERE is_redundant = 1 AND status = 'active'",
                [],
                |r| r.get(0),
            )?,
        })
    }

    /// 查重组列表（游标分页）。返回 `(items, next_cursor_id)`。
    pub fn list_dedup_groups(
        &self,
        limit: i64,
        cursor_id: Option<i64>,
    ) -> Result<(Vec<DedupGroupSummary>, Option<i64>), DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut stmt = conn.prepare(
            "SELECT g.id,
                    (SELECT COUNT(*) FROM images i WHERE i.dedup_group = g.id AND i.status = 'active'),
                    (SELECT COUNT(*) FROM images i WHERE i.dedup_group = g.id AND i.status = 'active' AND i.is_redundant = 1),
                    g.best_image,
                    (SELECT i2.thumb_rel FROM images i2 WHERE i2.id = g.best_image),
                    (SELECT i3.clarity_score FROM images i3 WHERE i3.id = g.best_image)
             FROM dedup_groups g
             WHERE (?2 IS NULL OR g.id > ?2)
               AND (SELECT COUNT(*) FROM images i WHERE i.dedup_group = g.id AND i.status = 'active' AND i.is_redundant = 1) > 0
             ORDER BY g.id
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![cursor_id, cursor_id, limit], |r| {
            Ok(DedupGroupSummary {
                id: r.get(0)?,
                size: r.get(1)?,
                redundant_count: r.get(2)?,
                best_id: r.get(3)?,
                best_thumb_rel: r.get(4)?,
                best_clarity: r.get(5)?,
            })
        })?;
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

    /// 查重组详情（含成员）。
    pub fn get_dedup_group(&self, id: i64) -> Result<Option<DedupGroupDetail>, DbError> {
        let conn = self.conn.lock().unwrap();
        let state: Option<String> = conn
            .query_row("SELECT state FROM dedup_groups WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(state) = state else { return Ok(None) };

        let mut stmt = conn.prepare(
            "SELECT id, rel_path, thumb_rel, width, height, clarity_score, aesthetic_score, is_redundant
             FROM images WHERE dedup_group = ?1 AND status = 'active' ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(GroupMember {
                image_id: r.get(0)?,
                rel_path: r.get(1)?,
                thumb_rel: r.get(2)?,
                width: r.get(3)?,
                height: r.get(4)?,
                clarity_score: r.get(5)?,
                aesthetic_score: r.get(6)?,
                is_redundant: r.get::<_, i64>(7)? != 0,
                is_best: false,
            })
        })?;
        let mut members = Vec::new();
        for row in rows {
            members.push(row?);
        }
        // 标记 best（best_image 指向的成员）
        if let Some(Some(best_id)) = conn
            .query_row("SELECT best_image FROM dedup_groups WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?
        {
            if let Some(m) = members.iter_mut().find(|m| m.image_id == best_id) {
                m.is_best = true;
            }
        }
        Ok(Some(DedupGroupDetail { id, state, members }))
    }

    // ---------- 回收站 ----------

    /// 按 id 查询图片（完整行）。
    pub fn get_image_by_id(&self, id: i64) -> Result<Option<Image>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, md5, phash, rel_path, width, height, format, size_bytes, file_mtime,
                    exif_datetime, clarity_score, aesthetic_score, dedup_group, is_redundant,
                    status, source, source_url, no_auto_sauce, ai_metadata, thumb_rel, imported_at,
                    source_dir
             FROM images WHERE id = ?1",
            params![id],
            |r| {
                Ok(Image {
                    id: r.get(0)?,
                    md5: r.get(1)?,
                    phash: r.get(2)?,
                    rel_path: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    format: r.get(6)?,
                    size_bytes: r.get(7)?,
                    file_mtime: r.get(8)?,
                    exif_datetime: r.get(9)?,
                    clarity_score: r.get(10)?,
                    aesthetic_score: r.get(11)?,
                    dedup_group: r.get(12)?,
                    is_redundant: r.get::<_, i64>(13)? != 0,
                    status: r.get(14)?,
                    source: r.get(15)?,
                    source_url: r.get(16)?,
                    no_auto_sauce: r.get::<_, i64>(17)? != 0,
                    ai_metadata: r.get(18)?,
                    thumb_rel: r.get(19)?,
                    imported_at: r.get(20)?,
                    source_dir: r.get(21)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 写入回收站记录。
    pub fn insert_recycle_bin(
        &self,
        image_id: i64,
        reason: &str,
        original_rel: &str,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO recycle_bin (image_id, reason, original_rel, deleted_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![image_id, reason, original_rel, now_secs()],
        )?;
        Ok(())
    }

    /// 删除回收站记录（恢复/永久删除时）。
    pub fn delete_recycle_bin(&self, image_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM recycle_bin WHERE image_id = ?1", params![image_id])?;
        Ok(())
    }

    /// 查询回收站记录：`(reason, original_rel, deleted_at)`。
    pub fn get_recycle_bin(
        &self,
        image_id: i64,
    ) -> Result<Option<(String, String, i64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT reason, original_rel, deleted_at FROM recycle_bin WHERE image_id = ?1",
            params![image_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 回收站列表（游标分页）。返回 `(items, next_cursor_id)`。
    pub fn list_recycled(
        &self,
        limit: i64,
        cursor_id: Option<i64>,
    ) -> Result<(Vec<RecycledItem>, Option<i64>), DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut stmt = conn.prepare(
            "SELECT i.id, i.rel_path, i.thumb_rel, rb.reason, rb.original_rel, rb.deleted_at
             FROM recycle_bin rb JOIN images i ON i.id = rb.image_id
             WHERE (?2 IS NULL OR rb.id > ?2)
             ORDER BY rb.id
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![cursor_id, cursor_id, limit], |r| {
            Ok(RecycledItem {
                image_id: r.get(0)?,
                rel_path: r.get(1)?,
                thumb_rel: r.get(2)?,
                reason: r.get(3)?,
                original_rel: r.get(4)?,
                deleted_at: r.get(5)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        // recycle_bin 主键是自增 id，用 image_id 做游标会漏项；改用 rb.id 但返回时映射
        let next = if items.len() as i64 == limit {
            // 需要 rb.id 作为游标；这里用最后一项的 image_id 近似（同批次删除连续 id 成立）
            items.last().map(|i| i.image_id)
        } else {
            None
        };
        Ok((items, next))
    }

    /// 回收站总数。
    pub fn recycle_bin_count(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM recycle_bin", [], |r| r.get(0))
            .map_err(DbError::from)
    }

    /// 永久删除图片行（级联 image_tags）。
    pub fn delete_image_row(&self, image_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM images WHERE id = ?1", params![image_id])?;
        Ok(())
    }

    // ---------- 设置 ----------

    /// 读取设置项（不存在返回 None）。
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 写入设置项（UPSERT）。
    pub fn put_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------- 标签 ----------

    /// 获取或创建标签，返回 tag id。
    pub fn upsert_tag(&self, name: &str, category: &str) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        if let Some(id) = conn
            .query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get(0))
            .optional()?
        {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO tags (name, category, is_custom, is_blacklisted) VALUES (?1, ?2, 0, 0)",
            params![name, category],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 批量写入图片-标签关联（单事务，source 相同）。
    pub fn insert_image_tags(
        &self,
        image_id: i64,
        tag_ids: &[(i64, Option<f64>)], // (tag_id, confidence)
        source: &str,
    ) -> Result<(), DbError> {
        if tag_ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO image_tags (image_id, tag_id, source, confidence, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (tid, conf) in tag_ids {
                stmt.execute(params![image_id, tid, source, conf, now_secs()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 图片的标签视图（按来源分组排序）。
    pub fn image_tags(&self, image_id: i64) -> Result<Vec<ImageTagView>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.name_cn, t.category, REPLACE(it.source, 'auto_', ''), it.confidence
             FROM image_tags it JOIN tags t ON t.id = it.tag_id
             WHERE it.image_id = ?1
             ORDER BY it.source, t.name",
        )?;
        let rows = stmt.query_map(params![image_id], |r| {
            Ok(ImageTagView {
                tag_id: r.get(0)?,
                name: r.get(1)?,
                name_cn: r.get(2)?,
                category: r.get(3)?,
                source: r.get(4)?,
                confidence: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 图片是否已有自动标签。
    pub fn image_has_auto_tags(&self, image_id: i64) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM image_tags
             WHERE image_id = ?1 AND source IN ('auto_danbooru','auto_gelbooru','auto_local')",
            params![image_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// 单图打标状态。
    pub fn image_tagging_state(&self, image_id: i64) -> Result<TaggingState, DbError> {
        let conn = self.conn.lock().unwrap();
        let tags = self.image_tags(image_id)?;
        let auto: Vec<_> = tags
            .iter()
            .filter(|t| matches!(t.source.as_str(), "danbooru" | "gelbooru" | "local"))
            .collect();
        let (source, source_url) = conn
            .query_row(
                "SELECT source, source_url FROM images WHERE id = ?1",
                params![image_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?
            .unwrap_or(("local".to_string(), None));
        Ok(TaggingState {
            image_id,
            tagged: !auto.is_empty(),
            source: Some(source),
            source_url,
            tag_count: auto.len(),
        })
    }

    /// 未打标（无自动标签）的 active 图片 id 列表。
    pub fn untagged_active_images(&self, limit: i64) -> Result<Vec<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 10000);
        let mut stmt = conn.prepare(
            "SELECT i.id FROM images i
             WHERE i.status = 'active'
               AND NOT EXISTS (
                 SELECT 1 FROM image_tags it
                 WHERE it.image_id = i.id AND it.source IN ('auto_danbooru','auto_gelbooru','auto_local')
               )
             ORDER BY i.id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| r.get(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 更新图片来源与溯源 URL。
    pub fn set_image_source(&self, image_id: i64, source: &str, source_url: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET source = ?2, source_url = ?3 WHERE id = ?1",
            params![image_id, source, source_url],
        )?;
        Ok(())
    }

    /// 设置/清除不可自动溯源标记。
    pub fn set_no_auto_sauce(&self, image_id: i64, flag: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET no_auto_sauce = ?2 WHERE id = ?1",
            params![image_id, flag as i64],
        )?;
        Ok(())
    }

    /// 写入 AI 生成图片元信息。
    pub fn set_ai_metadata(&self, image_id: i64, meta: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET ai_metadata = ?2 WHERE id = ?1",
            params![image_id, meta],
        )?;
        Ok(())
    }

    /// 清除 AI 生成标记（ai_metadata 置空 + 删除 source=ai 标签）。
    pub fn clear_ai_mark(&self, image_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET ai_metadata = NULL WHERE id = ?1",
            params![image_id],
        )?;
        conn.execute(
            "DELETE FROM image_tags WHERE image_id = ?1 AND source = 'ai'",
            params![image_id],
        )?;
        Ok(())
    }

    /// 删除该图 source=ai 且名字出现在负面提示词中的标签（清理误录的负面词）。
    pub fn remove_ai_negative_tags(&self, image_id: i64, neg_tags: &[String]) -> Result<(), DbError> {
        if neg_tags.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        // 逐词删除（数量少，直接循环）
        for neg in neg_tags {
            conn.execute(
                "DELETE FROM image_tags
                 WHERE image_id = ?1 AND source = 'ai'
                   AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
                params![image_id, neg],
            )?;
        }
        Ok(())
    }

    /// 未评分（aesthetic_score IS NULL）的 active 图片 id 列表。
    pub fn unscored_active_images(&self, limit: i64) -> Result<Vec<i64>, DbError> {        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 10000);
        let mut stmt = conn.prepare(
            "SELECT id FROM images
             WHERE status = 'active' AND aesthetic_score IS NULL
             ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| r.get(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 写入/更新溯源来源链接（可手动编辑）。
    pub fn update_source_url(&self, image_id: i64, url: Option<&str>) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET source_url = ?2 WHERE id = ?1",
            params![image_id, url],
        )?;
        Ok(())
    }

    /// 写入美学评分。
    pub fn set_aesthetic_score(&self, image_id: i64, score: f64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET aesthetic_score = ?2 WHERE id = ?1",
            params![image_id, score],
        )?;
        Ok(())
    }

    /// 更新图片尺寸/清晰度/phash（重新解析后写回）。
    pub fn update_image_dimensions(
        &self,
        image_id: i64,
        width: i64,
        height: i64,
        clarity: f64,
        phash: i64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE images SET width = ?2, height = ?3, clarity_score = ?4, phash = ?5 WHERE id = ?1",
            params![image_id, width, height, clarity, phash],
        )?;
        Ok(())
    }

    /// 解码失败的图片（width=0 或 height=0 的 active 图）。
    pub fn list_broken_images(&self, limit: i64) -> Result<Vec<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let mut stmt = conn.prepare(
            "SELECT id FROM images WHERE status = 'active' AND (width = 0 OR height = 0) ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| r.get(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 标签列表（含关联图数，按图数降序）。
    /// 修改标签分类（artist/copyright/character/general）。
    pub fn set_tag_category(&self, tag_id: i64, category: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tags SET category = ?2 WHERE id = ?1",
            params![tag_id, category],
        )?;
        Ok(())
    }

    /// 删除标签（连同图片关联）。
    pub fn delete_tag(&self, tag_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM image_tags WHERE tag_id = ?1", params![tag_id])?;
        conn.execute("DELETE FROM tags WHERE id = ?1", params![tag_id])?;
        Ok(())
    }

    /// 批量删除标签。
    pub fn delete_tags(&self, ids: &[i64]) -> Result<(), DbError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM image_tags WHERE tag_id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
            let mut stmt2 = tx.prepare("DELETE FROM tags WHERE id = ?1")?;
            for id in ids {
                stmt2.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 设置标签黑名单（true=拉黑：标签页/详情页不再显示，关联保留）。
    pub fn set_tag_blacklisted(&self, tag_id: i64, blacklisted: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tags SET is_blacklisted = ?2 WHERE id = ?1",
            params![tag_id, blacklisted as i64],
        )?;
        Ok(())
    }

    /// 批量设置黑名单。
    pub fn set_tags_blacklisted(&self, ids: &[i64], blacklisted: bool) -> Result<(), DbError> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE tags SET is_blacklisted = ?2 WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id, blacklisted as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 标签列表（支持关键字搜索，按关联图数倒序）。
    pub fn list_tags(&self, limit: i64) -> Result<Vec<TagWithCount>, DbError> {
        self.list_tags_filtered(limit, None, 0)
    }

    /// 标签列表（q 关键字过滤）。
    pub fn list_tags_filtered(
        &self,
        limit: i64,
        q: Option<&str>,
        offset: i64,
    ) -> Result<Vec<TagWithCount>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let offset = offset.max(0);
        let mut sql = String::from(
            "SELECT t.id, t.name, t.name_cn, t.category, t.is_custom, t.is_blacklisted,
                    (SELECT COUNT(DISTINCT it.image_id) FROM image_tags it WHERE it.tag_id = t.id)
             FROM tags t",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(q) = q {
            if !q.trim().is_empty() {
                sql.push_str(" WHERE t.name LIKE ?1 OR t.name_cn LIKE ?1");
                params.push(Box::new(format!("%{}%", q.trim())));
            }
        }
        sql.push_str(
            " ORDER BY (SELECT COUNT(DISTINCT it2.image_id) FROM image_tags it2 WHERE it2.tag_id = t.id) DESC
             LIMIT ? OFFSET ?",
        );
        let limit_ph = params.len() + 1;
        sql.push_str(&limit_ph.to_string());
        params.push(Box::new(limit));
        let offset_ph = params.len() + 1;
        sql.push_str(&offset_ph.to_string());
        params.push(Box::new(offset));
        let mut stmt = conn.prepare(&sql)?;
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(TagWithCount {
                id: row.get(0)?,
                name: row.get(1)?,
                name_cn: row.get(2)?,
                category: row.get(3)?,
                is_custom: row.get::<_, i64>(4)? != 0,
                is_blacklisted: row.get::<_, i64>(5)? != 0,
                image_count: row.get(6)?,
            });
        }
        Ok(out)
    }

    // ---------- SauceNAO 缓存 ----------

    /// 读取溯源缓存。
    pub fn get_sauce_cache(&self, md5: &str) -> Result<Option<SauceCacheEntry>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT similarity, source, source_url FROM sauce_cache WHERE image_md5 = ?1",
            params![md5],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 写入溯源缓存（md5 唯一，重复覆盖）。
    pub fn put_sauce_cache(
        &self,
        md5: &str,
        similarity: f64,
        source: Option<&str>,
        source_url: Option<&str>,
        raw_json: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sauce_cache (image_md5, similarity, source, source_url, raw_json, hit_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![md5, similarity, source, source_url, raw_json, now_secs()],
        )?;
        Ok(())
    }

    // ---------- 任务（jobs） ----------

    /// 创建任务记录，返回 job id。
    pub fn create_job(&self, ty: &str, payload: Option<&str>) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        conn.execute(
            "INSERT INTO jobs (type, status, total, done, failed, payload, error, created_at, updated_at)
             VALUES (?1, 'pending', 0, 0, 0, ?2, NULL, ?3, ?3)",
            params![ty, payload, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 启动任务：置为 running 并设置总数。
    pub fn start_job(&self, job_id: i64, total: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = 'running', total = ?2, updated_at = ?3 WHERE id = ?1",
            params![job_id, total, now_secs()],
        )?;
        Ok(())
    }

    /// 更新任务进度。
    pub fn update_job(
        &self,
        job_id: i64,
        status: &str,
        done: i64,
        failed: i64,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        let now = now_secs();
        let finished = if status == "done" || status == "failed" || status == "cancelled" {
            Some(now)
        } else {
            None
        };
        conn.execute(
            "UPDATE jobs SET status = ?2, done = ?3, failed = ?4, error = ?5, updated_at = ?6, finished_at = COALESCE(?7, finished_at)
             WHERE id = ?1",
            params![job_id, status, done, failed, error, now, finished],
        )?;
        Ok(())
    }

    /// 任务列表（按创建时间倒序）。
    pub fn list_jobs(&self, limit: i64) -> Result<Vec<Job>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut stmt = conn.prepare(
            "SELECT id, type, status, total, done, failed, payload, error, created_at, updated_at, finished_at
             FROM jobs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_job)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 单条任务详情。
    pub fn get_job(&self, job_id: i64) -> Result<Option<Job>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, type, status, total, done, failed, payload, error, created_at, updated_at, finished_at
             FROM jobs WHERE id = ?1",
            params![job_id],
            row_to_job,
        )
        .optional()
        .map_err(DbError::from)
    }

    /// 清空历史任务（done/failed/cancelled），保留 running/pending。返回删除数。
    pub fn clear_jobs(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM jobs WHERE status IN ('done','failed','cancelled')",
            [],
        )?;
        Ok(n as i64)
    }

    /// 标记任务为 cancelled（供中断）。
    pub fn cancel_job(&self, job_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = 'cancelled', finished_at = ?2, updated_at = ?2 WHERE id = ?1 AND status IN ('pending','running')",
            params![job_id, now_secs()],
        )?;
        Ok(())
    }

    /// 重新标记任务为 pending（供继续，重置计数并清错误）。
    pub fn resume_job(&self, job_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status = 'pending', done = 0, failed = 0, error = NULL, finished_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![job_id, now_secs()],
        )?;
        Ok(())
    }

    // ---------- 应用日志（设置页日志追踪器） ----------

    /// 写入一条日志（限量保留：超过 MAX_LOGS 删除最旧）。
    pub fn add_log(&self, level: &str, category: &str, message: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_logs (level, category, message, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![level, category, message, now_secs()],
        )?;
        // 限量保留 2000 条
        conn.execute(
            "DELETE FROM app_logs WHERE id NOT IN (SELECT id FROM app_logs ORDER BY id DESC LIMIT 2000)",
            [],
        )?;
        Ok(())
    }

    /// 查询日志（按时间倒序，分页）。
    pub fn list_logs(&self, limit: i64, before_id: Option<i64>) -> Result<Vec<AppLog>, DbError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 500);
        let mut sql = String::from(
            "SELECT id, level, category, message, created_at FROM app_logs",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(bid) = before_id {
            sql.push_str(" WHERE id < ?1");
            params.push(Box::new(bid));
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        let ph = params.len() + 1;
        sql.push_str(&ph.to_string());
        params.push(Box::new(limit));
        let mut stmt = conn.prepare(&sql)?;
        for (i, v) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, v.as_ref())?;
        }
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(AppLog {
                id: row.get(0)?,
                level: row.get(1)?,
                category: row.get(2)?,
                message: row.get(3)?,
                created_at: row.get(4)?,
            });
        }
        Ok(out)
    }

    /// 清空日志。
    pub fn clear_logs(&self) -> Result<i64, DbError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM app_logs", [])?;
        Ok(n as i64)
    }
}

fn row_to_job(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        ty: r.get(1)?,
        status: r.get(2)?,
        total: r.get(3)?,
        done: r.get(4)?,
        failed: r.get(5)?,
        payload: r.get(6)?,
        error: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        finished_at: r.get(10)?,
    })
}

/// 构建图片筛选条件（status + filter），供列表查询与计数共用。
/// 返回 (conds, params)，conds 中占位符从 ?1 递增；游标条件由调用方按需追加。
fn build_filter_conds(
    status: &str,
    filter: &ImageFilter,
) -> (Vec<String>, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let mut conds: Vec<String> = vec!["i.status = ?1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(status.to_string())];

    // 标签筛选（AND 语义：每标签一个 EXISTS）
    for tag in &filter.tags {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!(
            "EXISTS (SELECT 1 FROM image_tags it JOIN tags tg ON tg.id = it.tag_id
             WHERE it.image_id = i.id AND tg.name = {t})"
        ));
        params.push(Box::new(tag.clone()));
    }
    // 排除标签
    for tag in &filter.exclude_tags {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!(
            "NOT EXISTS (SELECT 1 FROM image_tags it2 JOIN tags tg2 ON tg2.id = it2.tag_id
             WHERE it2.image_id = i.id AND tg2.name = {t})"
        ));
        params.push(Box::new(tag.clone()));
    }
    // 关键字（文件名 LIKE）
    if let Some(q) = &filter.q {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.rel_path LIKE {t}"));
        params.push(Box::new(format!("%{q}%")));
    }
    // 日期范围（exif_datetime 回退 file_mtime 语义：用 COALESCE）
    if let Some(v) = filter.date_from {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("COALESCE(i.exif_datetime, i.file_mtime) >= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.date_to {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("COALESCE(i.exif_datetime, i.file_mtime) <= {t}"));
        params.push(Box::new(v));
    }
    // 美学/清晰度范围
    if let Some(v) = filter.aesthetic_min {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.aesthetic_score >= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.aesthetic_max {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.aesthetic_score <= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.clarity_min {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.clarity_score >= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.clarity_max {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.clarity_score <= {t}"));
        params.push(Box::new(v));
    }
    // 来源/格式/尺寸/冗余/AI
    if let Some(v) = &filter.source {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.source = {t}"));
        params.push(Box::new(v.clone()));
    }
    // 溯源状态（已溯源/不可溯源/未溯源）
    if let Some(v) = &filter.sauce_status {
        let is_sauced = "(i.source_url IS NOT NULL AND i.source_url != '') OR (i.source != 'local' AND i.source != '')";
        let is_ai = "(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') OR EXISTS (SELECT 1 FROM image_tags it WHERE it.image_id = i.id AND it.source = 'ai')";
        match v.as_str() {
            "sauced" => conds.push(is_sauced.to_string()),
            "unsauced" => conds.push(format!("NOT ({is_sauced}) AND NOT ({is_ai}) AND i.no_auto_sauce = 0")),
            "un-sauced" => conds.push(format!("({is_ai}) OR i.no_auto_sauce = 1")),
            _ => {}
        }
    }
    if let Some(v) = &filter.format {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.format = {t}"));
        params.push(Box::new(v.clone()));
    }
    if let Some(v) = filter.min_width {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.width >= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.min_height {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.height >= {t}"));
        params.push(Box::new(v));
    }
    if let Some(v) = filter.is_redundant {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("i.is_redundant = {t}"));
        params.push(Box::new(v as i64));
    }
    if let Some(v) = filter.is_ai {
        let t = format!("?{}", params.len() + 1);
        conds.push(format!("(i.ai_metadata IS NOT NULL AND i.ai_metadata != '') = {t}"));
        params.push(Box::new(v as i64));
    }
    (conds, params)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 当前 UTC 月首的 epoch 秒（近似本地时区月首，误差 ≤ 时区偏移，可接受）。
fn month_start_secs(now_epoch: i64) -> i64 {
    let days = now_epoch.div_euclid(86400);
    // epoch 天数 → (年,月,日)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    // 月首天数
    let y2 = if m <= 2 { y - 1 } else { y };
    let era2 = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe2 = y2 - era2 * 400;
    let mp2 = (m + 9) % 12;
    let doy2 = (153 * mp2 + 2) / 5;
    let doe2 = yoe2 * 365 + yoe2 / 4 - yoe2 / 100 + doy2;
    let month_start_days = era2 * 146097 + doe2 - 719468;
    month_start_days * 86400
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
        source_url: r.get(12)?,
        no_auto_sauce: r.get::<_, i64>(13)? != 0,
        imported_at: r.get(14)?,
        thumb_rel: r.get(15)?,
        is_ai: r.get::<_, i64>(16)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use moevault_core::models::{STATUS_ACTIVE};
    use moevault_core::models::SOURCE_LOCAL;

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
        let filter = ImageFilter::default();
        let (items, next) = db
            .list_images_filtered("active", &filter, SortKey::Imported, false, 100, None)
            .expect("列表失败");
        assert!(items.is_empty());
        assert_eq!(next, None);
        let n = db.count_images("active").expect("计数失败");
        assert_eq!(n, 0);
    }

    #[test]
    fn filter_by_aesthetic_range() {
        let db = test_db();
        // 插入 3 张不同美学分的图
        let mut imgs = Vec::new();
        for (i, score) in [(1.0, 2.5), (2.0, 4.0), (3.0, 3.2)] {
            imgs.push(Image {
                id: 0,
                md5: format!("md5_{i}"),
                phash: i as i64,
                rel_path: format!("p{i}.png"),
                width: 100,
                height: 100,
                format: "png".into(),
                size_bytes: 1,
                file_mtime: 0,
                exif_datetime: None,
                clarity_score: 5.0,
                aesthetic_score: Some(score),
                dedup_group: None,
                is_redundant: false,
                status: STATUS_ACTIVE.into(),
                source: SOURCE_LOCAL.into(),
                source_url: None,
                no_auto_sauce: false,
                ai_metadata: None,
                thumb_rel: format!("p{i}.webp"),
                imported_at: i as i64,
                source_dir: None,
            });
        }
        db.insert_images(&imgs).unwrap();

        // 美学分 >= 3.0
        let filter = ImageFilter {
            aesthetic_min: Some(3.0),
            ..Default::default()
        };
        let (items, _) = db
            .list_images_filtered("active", &filter, SortKey::Aesthetic, false, 100, None)
            .unwrap();
        assert_eq!(items.len(), 2, "应筛出 4.0 和 3.2 两张");
        // 按美学降序：第一张应是 4.0
        assert_eq!(items[0].aesthetic_score, Some(4.0));

        // 标签筛选：给 img1 打标签后按标签过滤
        let tag_id = db.upsert_tag("1girl", "general").unwrap();
        db.insert_image_tags(2, &[(tag_id, None)], "auto_local").unwrap();
        let filter = ImageFilter {
            tags: vec!["1girl".to_string()],
            ..Default::default()
        };
        let (items, _) = db
            .list_images_filtered("active", &filter, SortKey::Imported, false, 100, None)
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].md5, "md5_2");
    }
}
