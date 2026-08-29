-- V10: 分类浏览手动封面映射（tag_covers）。
-- manual 封面规则读取本表；未设置时回退美学最高规则。
CREATE TABLE IF NOT EXISTS tag_covers (
  tag_id   INTEGER PRIMARY KEY REFERENCES tags(id)  ON DELETE CASCADE,
  image_id INTEGER NOT NULL REFERENCES images(id)   ON DELETE CASCADE
);
