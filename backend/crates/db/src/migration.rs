//! 轻量版本化迁移器。
//!
//! 约定：迁移以 `V{n}__name` 命名，编译期嵌入（`include_str!`）。
//! 每个迁移在单个事务中执行，成功后在 `schema_migrations` 记录版本。

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use super::DbError;

/// 迁移清单：`(版本名, SQL)`。新增迁移时在此追加并更新序号。
const MIGRATIONS: &[(&str, &str)] = &[
    ("V1__init", include_str!("../migrations/V1__init.sql")),
    ("V2__import_duplicate", include_str!("../migrations/V2__import_duplicate.sql")),
    ("V3__no_auto_sauce", include_str!("../migrations/V3__no_auto_sauce.sql")),
    ("V4__ai_metadata", include_str!("../migrations/V4__ai_metadata.sql")),
    ("V5__jobs", include_str!("../migrations/V5__jobs.sql")),
    ("V6__jobs_rebuild", include_str!("../migrations/V6__jobs_rebuild.sql")),
];

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 执行所有未应用的迁移（幂等，可重复调用）。
pub fn run(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version    TEXT PRIMARY KEY,
             applied_at INTEGER NOT NULL
         );",
    )?;

    for (name, sql) in MIGRATIONS {
        let applied: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [name],
            |r| r.get(0),
        )?;
        if applied != 0 {
            continue;
        }

        // 单事务执行迁移（SQLite 的 execute_batch 支持 BEGIN/COMMIT 包裹）
        let tx_sql = format!("BEGIN;\n{sql}\nCOMMIT;");
        conn.execute_batch(&tx_sql).map_err(|e| {
            DbError::Migration(format!("迁移 {name} 失败: {e}"))
        })?;

        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![name, now_secs()],
        )?;
    }
    Ok(())
}
