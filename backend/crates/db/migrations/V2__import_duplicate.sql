-- V2: import_batches 增加 duplicate 计数列
ALTER TABLE import_batches ADD COLUMN duplicate INTEGER NOT NULL DEFAULT 0;
