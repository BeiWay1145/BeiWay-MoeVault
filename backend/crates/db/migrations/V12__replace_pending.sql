-- V12: 待确认替换表（增强3：批量溯源自动替换时，严格查重未通过的候选 → 查重界面人工确认）。
CREATE TABLE IF NOT EXISTS replace_pending (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  image_id   INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
  temp_path  TEXT    NOT NULL,
  net_size   INTEGER NOT NULL,
  local_size INTEGER NOT NULL,
  net_width  INTEGER,
  net_height INTEGER,
  source_url TEXT,
  created_at INTEGER NOT NULL
);
