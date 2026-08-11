-- V5: 任务表（打标/美学/溯源等耗时任务的持久化记录）
CREATE TABLE IF NOT EXISTS jobs (
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
