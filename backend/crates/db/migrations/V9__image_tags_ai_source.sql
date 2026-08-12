-- V9: image_tags.source 允许 'ai'（AI 图 prompt 标签来源）。
-- 原 CHECK 只允许 auto_danbooru/auto_gelbooru/auto_local/manual，
-- 导致代码中 source='ai' 的 INSERT OR IGNORE 被约束静默吞掉（标签从未入库，
-- 打标任务对 AI 图"立即完成"但无标签写入）。SQLite 改 CHECK 需重建表。
ALTER TABLE image_tags RENAME TO image_tags_old;

CREATE TABLE image_tags (
  image_id   INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
  source     TEXT    NOT NULL CHECK (source IN
               ('auto_danbooru','auto_gelbooru','auto_local','manual','ai')),
  confidence REAL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (image_id, tag_id, source)
);

INSERT INTO image_tags (image_id, tag_id, source, confidence, created_at)
  SELECT image_id, tag_id, source, confidence, created_at FROM image_tags_old;

DROP TABLE image_tags_old;

CREATE INDEX IF NOT EXISTS idx_image_tags_tag   ON image_tags(tag_id);
CREATE INDEX IF NOT EXISTS idx_image_tags_image ON image_tags(image_id);
