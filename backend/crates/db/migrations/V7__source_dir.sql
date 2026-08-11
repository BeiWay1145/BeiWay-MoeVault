-- V7: images 增加 source_dir（来源文件夹名，主目录按来源分组用）。
-- 仅新导入的图片会填充；历史图片为 NULL（显示"未知来源"）。
ALTER TABLE images ADD COLUMN source_dir TEXT;
CREATE INDEX IF NOT EXISTS idx_images_source_dir ON images (source_dir);
