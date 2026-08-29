-- V11: 标签多中文别名表（一个 tag 可有多条中文别名，用于搜索联想与显示）。
CREATE TABLE IF NOT EXISTS tag_aliases (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  alias      TEXT    NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tag_aliases_tag ON tag_aliases(tag_id);
