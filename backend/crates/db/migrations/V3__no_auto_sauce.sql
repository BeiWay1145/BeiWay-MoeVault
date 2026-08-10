-- V3: images 增加 no_auto_sauce 标记（确认无法溯源到 booru 的图，自动打标跳过）
ALTER TABLE images ADD COLUMN no_auto_sauce INTEGER NOT NULL DEFAULT 0;
