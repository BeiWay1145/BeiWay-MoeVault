-- V8: 应用日志表（设置页日志追踪器用，持久化限量保留）。
CREATE TABLE IF NOT EXISTS app_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    level       TEXT    NOT NULL DEFAULT 'info',  -- info / warn / error
    category    TEXT    NOT NULL DEFAULT 'system', -- task / sauce / tag / aesthetic / frontend / import / system
    message     TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_app_logs_created ON app_logs (created_at DESC);
