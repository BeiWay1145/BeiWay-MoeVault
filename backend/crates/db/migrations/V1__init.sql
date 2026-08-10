-- BeiWay-MoeVault 初始建表（V1）
-- 与 docs/TECH_DETAILS.md 第 1.1 节 DDL 一致。

CREATE TABLE IF NOT EXISTS images (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  md5             TEXT    NOT NULL UNIQUE,
  phash           INTEGER NOT NULL,
  rel_path        TEXT    NOT NULL UNIQUE,
  width           INTEGER NOT NULL,
  height          INTEGER NOT NULL,
  format          TEXT    NOT NULL,
  size_bytes      INTEGER NOT NULL,
  file_mtime      INTEGER NOT NULL,
  exif_datetime   INTEGER,
  clarity_score   REAL    NOT NULL,
  aesthetic_score REAL,
  dedup_group     INTEGER REFERENCES dedup_groups(id),
  is_redundant    INTEGER NOT NULL DEFAULT 0,
  status          TEXT    NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active','recycled')),
  source          TEXT    NOT NULL DEFAULT 'local',
  source_url      TEXT,
  thumb_rel       TEXT    NOT NULL,
  imported_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  name           TEXT    NOT NULL UNIQUE,
  name_cn        TEXT,
  category       TEXT    NOT NULL DEFAULT 'general',
  is_custom      INTEGER NOT NULL DEFAULT 0,
  is_blacklisted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS image_tags (
  image_id   INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
  source     TEXT    NOT NULL CHECK (source IN
               ('auto_danbooru','auto_gelbooru','auto_local','manual')),
  confidence REAL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (image_id, tag_id, source)
);

CREATE TABLE IF NOT EXISTS dedup_groups (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  phash_seed  INTEGER NOT NULL,
  best_image  INTEGER REFERENCES images(id),
  state       TEXT    NOT NULL DEFAULT 'open',
  created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS recycle_bin (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  image_id     INTEGER NOT NULL UNIQUE REFERENCES images(id),
  reason       TEXT    NOT NULL,
  original_rel TEXT    NOT NULL,
  deleted_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS import_batches (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  source_path TEXT    NOT NULL,
  total       INTEGER NOT NULL DEFAULT 0,
  done        INTEGER NOT NULL DEFAULT 0,
  failed      INTEGER NOT NULL DEFAULT 0,
  state       TEXT    NOT NULL DEFAULT 'pending',
  created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS jobs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  job_type     TEXT    NOT NULL,
  image_id     INTEGER REFERENCES images(id),
  batch_id     INTEGER REFERENCES import_batches(id),
  payload      TEXT,
  status       TEXT    NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending','running','done','failed','cancelled')),
  attempts     INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 5,
  next_run_at  INTEGER NOT NULL DEFAULT 0,
  error        TEXT,
  priority     INTEGER NOT NULL DEFAULT 10,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sauce_cache (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  image_md5   TEXT    NOT NULL UNIQUE,
  similarity  REAL,
  source      TEXT,
  source_url  TEXT,
  raw_json    TEXT,
  hit_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS smart_views (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT    NOT NULL UNIQUE,
  filter_json TEXT    NOT NULL,
  sort        TEXT,
  created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_images_status      ON images(status);
CREATE INDEX IF NOT EXISTS idx_images_md5         ON images(md5);
CREATE INDEX IF NOT EXISTS idx_images_phash       ON images(phash);
CREATE INDEX IF NOT EXISTS idx_images_dedup_group ON images(dedup_group) WHERE dedup_group IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_images_exif_date   ON images(exif_datetime);
CREATE INDEX IF NOT EXISTS idx_images_imported_at ON images(imported_at);
CREATE INDEX IF NOT EXISTS idx_images_aesthetic   ON images(aesthetic_score);
CREATE INDEX IF NOT EXISTS idx_images_clarity     ON images(clarity_score);

CREATE INDEX IF NOT EXISTS idx_image_tags_tag   ON image_tags(tag_id);
CREATE INDEX IF NOT EXISTS idx_image_tags_image ON image_tags(image_id);

CREATE INDEX IF NOT EXISTS idx_jobs_queue ON jobs(status, next_run_at, priority);
CREATE INDEX IF NOT EXISTS idx_recycle_deleted_at ON recycle_bin(deleted_at);
