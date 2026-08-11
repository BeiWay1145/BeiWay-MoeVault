-- V6: 修复 jobs 表结构（V5 早期版本误用 job_type/image_id 列，现统一为 type/total/done/failed 新结构）。
-- jobs 为任务记录表，历史无有效数据，直接重建安全。
DROP TABLE IF EXISTS jobs;
CREATE TABLE jobs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    type        TEXT    NOT NULL,             -- tag / aesthetic / sauce / ai-detect / import
    status      TEXT    NOT NULL DEFAULT 'pending', -- pending / running / done / failed / cancelled
    total       INTEGER NOT NULL DEFAULT 0,
    done        INTEGER NOT NULL DEFAULT 0,
    failed      INTEGER NOT NULL DEFAULT 0,
    payload     TEXT,                         -- JSON：image_ids 等
    error       TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs (status);
CREATE INDEX IF NOT EXISTS idx_jobs_created ON jobs (created_at DESC);
