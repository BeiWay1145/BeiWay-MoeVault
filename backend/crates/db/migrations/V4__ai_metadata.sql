-- V4: images 增加 ai_metadata（AI 生成图片元信息，PNG tEXt 读取）
ALTER TABLE images ADD COLUMN ai_metadata TEXT;
