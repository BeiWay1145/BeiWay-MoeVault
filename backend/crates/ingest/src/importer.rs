//! 导入流程编排：扫描 → 特征提取 → 移动进库 → 缩略图 → 写库。
//!
//! 设计：
//! - 同步函数（调用方以 spawn_blocking 执行，避免阻塞 async runtime）
//! - 单张失败不中断批次（记 failed 继续）
//! - md5 重复：跳过并删除库外源文件（移动模式语义）
//! - 批量入库（每 100 条一个事务）

use std::path::{Path, PathBuf};

use image::GenericImageView;
use moevault_core::models::{Image, STATUS_ACTIVE, SOURCE_LOCAL};
use moevault_db::Db;
use tracing::{info, warn};

use crate::{collect_images, extract_features, IngestError, SUPPORTED_EXTENSIONS};

/// 导入进度计数。
#[derive(Debug, Clone, Default, Copy)]
pub struct ImportProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub duplicate: usize,
    /// 问题2：重复源文件移入回收站的张数。
    pub duplicates_recycled: usize,
}

/// 导入模式。
/// - Move：移动进库（默认），源文件移入库目录（跨卷退化为复制+删除源）
/// - Copy：复制进库，源文件保留原位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Move,
    Copy,
}

impl Default for ImportMode {
    fn default() -> Self {
        Self::Move
    }
}

/// 缩略图卡片规格（最长边像素）。
const THUMB_CARD_PX: u32 = 512;

/// 批量入库阈值。
const INSERT_BATCH: usize = 100;

/// 执行一次导入批次。
///
/// - `db`：SQLite 连接
/// - `batch_id`：import_batches 表 id（进度写回）
/// - `paths`：源路径（文件或目录）
/// - `library_dir`：库目录（`data/library`），文件移动/复制至此按哈希分片
/// - `thumbs_dir`：缩略图目录（`data/thumbs`）
/// - `mode`：Move 移动进库（默认）/ Copy 复制进库
pub fn run_import(
    db: &Db,
    batch_id: i64,
    paths: Vec<PathBuf>,
    library_dir: &Path,
    thumbs_dir: &Path,
    recycle_dir: &Path,
    mode: ImportMode,
) -> Result<ImportProgress, IngestError> {
    std::fs::create_dir_all(library_dir)?;
    std::fs::create_dir_all(thumbs_dir)?;

    let files = collect_images(&paths);
    info!(batch_id, total = files.len(), "导入批次：扫描完成");

    let mut progress = ImportProgress {
        total: files.len(),
        ..Default::default()
    };
    db.update_import_batch(batch_id, progress.total as i64, 0, 0, 0, "indexing")?;

    let mut pending: Vec<Image> = Vec::with_capacity(INSERT_BATCH);
    let mut seen_md5: std::collections::HashSet<String> = std::collections::HashSet::new();
    let now = now_secs();

    for (i, src) in files.iter().enumerate() {
        match process_one(db, src, library_dir, thumbs_dir, recycle_dir, now, &mut seen_md5, mode) {
            Ok(ProcessOutcome::Imported(img)) => {
                pending.push(*img);
                progress.done += 1;
                if pending.len() >= INSERT_BATCH {
                    db.insert_images(&pending)?;
                    pending.clear();
                }
            }
            Ok(ProcessOutcome::Duplicate(recycled)) => {
                progress.duplicate += 1;
                if recycled {
                    progress.duplicates_recycled += 1;
                }
            }
            Err(e) => {
                warn!(path = %src.display(), error = %e, "导入单张失败");
                progress.failed += 1;
            }
        }

        // 每处理 16 张或最后一张更新一次批次进度
        if (i + 1) % 16 == 0 || i + 1 == files.len() {
            db.update_import_batch(
                batch_id,
                progress.total as i64,
                progress.done as i64,
                progress.failed as i64,
                progress.duplicate as i64,
                "indexing",
            )?;
        }
    }

    // 收尾：flush 剩余 + 标记完成
    db.insert_images(&pending)?;
    db.update_import_batch(
        batch_id,
        progress.total as i64,
        progress.done as i64,
        progress.failed as i64,
        progress.duplicate as i64,
        "done",
    )?;
    info!(
        batch_id,
        done = progress.done,
        failed = progress.failed,
        duplicate = progress.duplicate,
        "导入批次完成"
    );
    Ok(progress)
}

enum ProcessOutcome {
    Imported(Box<Image>),
    /// 重复：源文件是否已移入回收站目录。
    Duplicate(bool),
}

fn process_one(
    db: &Db,
    src: &Path,
    library_dir: &Path,
    thumbs_dir: &Path,
    recycle_dir: &Path,
    now: i64,
    seen_md5: &mut std::collections::HashSet<String>,
    mode: ImportMode,
) -> Result<ProcessOutcome, IngestError> {
    let feats = extract_features(src)?;

    // 重复检测：批内（内存集合，因批量插入延迟 flush）+ 库内（数据库）
    if seen_md5.contains(&feats.md5) || db.md5_exists(&feats.md5)? {
        // 问题2：重复源文件移入回收站目录（可恢复，防误判），保留原名
        let mut recycled = false;
        if !src.starts_with(library_dir) {
            let ext = crate::features::extension_of(src).unwrap_or_else(|| "unknown".to_string());
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{}.{}", &feats.md5[..feats.md5.len().min(8)], ext));
            let dup_dir = recycle_dir.join("import_duplicate");
            let _ = std::fs::create_dir_all(&dup_dir);
            let dst = dup_dir.join(&name);
            // 目标已存在 → 加短 md5 后缀
            let dst = if dst.exists() {
                let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
                let new_name = format!("{stem}_{}.{ext}", &feats.md5[..feats.md5.len().min(8)]);
                dup_dir.join(new_name)
            } else {
                dst
            };
            if std::fs::rename(src, &dst).is_ok() {
                recycled = true;
            } else if std::fs::copy(src, &dst).is_ok() {
                // 跨卷：复制后删源（Move 语义）；Copy 模式复制到回收站不删源
                if mode == ImportMode::Move {
                    let _ = std::fs::remove_file(src);
                }
                recycled = true;
            }
        }
        return Ok(ProcessOutcome::Duplicate(recycled));
    }
    seen_md5.insert(feats.md5.clone());

    // 进库：library/{md5前2}/{原文件名}.{ext}（保留原名；重名自动加短 md5 后缀）
    let ext = crate::features::extension_of(src).unwrap_or_else(|| "unknown".to_string());
    if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IngestError::Invalid(format!("不支持的扩展名: {ext}")));
    }
    let orig_stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| feats.md5.clone());
    let base_name = sanitize_filename(&orig_stem);
    // 文件名冲突检测：同分片目录内已存在同名 → 加短 md5 后缀
    let prefix_dir = &feats.md5[..feats.md5.len().min(2)];
    let mut file_stem = base_name.clone();
    let mut rel = PathBuf::from(prefix_dir).join(format!("{file_stem}.{ext}"));
    if library_dir.join(&rel).exists() {
        file_stem = format!("{base_name}_{}", &feats.md5[..feats.md5.len().min(8)]);
        rel = PathBuf::from(prefix_dir).join(format!("{file_stem}.{ext}"));
    }
    let dst = library_dir.join(&rel);
    match mode {
        ImportMode::Move => move_file(src, &dst)?,
        ImportMode::Copy => copy_file(src, &dst)?,
    }

    // 生成缩略图（WebP）：缩略图按 md5 命名（与图同分片前缀，重名无冲突）
    let thumb_rel = hash_rel_path(&feats.md5, "webp");
    let thumb_path = thumbs_dir.join(&thumb_rel);
    generate_thumbnail(&dst, &thumb_path);

    let img = Image {
        id: 0,
        md5: feats.md5.clone(),
        phash: feats.phash as i64,
        rel_path: rel.to_string_lossy().into_owned(),
        width: feats.width as i64,
        height: feats.height as i64,
        format: feats.format,
        size_bytes: feats.size_bytes,
        file_mtime: std::fs::metadata(&dst)
            .map(|m| m.modified().map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)).unwrap_or(now))
            .unwrap_or(now),
        exif_datetime: feats.exif_datetime,
        clarity_score: feats.clarity,
        aesthetic_score: None,
        dedup_group: None,
        is_redundant: false,
        status: STATUS_ACTIVE.to_string(),
        source: SOURCE_LOCAL.to_string(),
        source_url: None,
        no_auto_sauce: false,
        ai_metadata: None,
        // 来源文件夹名：取源文件所在目录名（主目录按来源分组）
        source_dir: src
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned()),
        thumb_rel: thumb_rel.to_string_lossy().into_owned(),
        imported_at: now,
    };
    Ok(ProcessOutcome::Imported(Box::new(img)))
}

/// 生成哈希分片相对路径 `{前2位}/{完整哈希}.{ext}`。
fn hash_rel_path(md5: &str, ext: &str) -> PathBuf {
    let prefix = &md5[..md5.len().min(2)];
    PathBuf::from(prefix).join(format!("{md5}.{ext}"))
}

/// 清理文件名中的 Windows 非法字符（\/:*?"<>| 与控制符）→ `_`；空名回退。
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        trimmed
    }
}

/// 移动文件：同卷 rename；跨卷（EXDEV）退化为 copy + remove。
fn move_file(src: &Path, dst: &Path) -> Result<(), IngestError> {
    if src == dst {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)?;
            Ok(())
        }
    }
}

/// 复制文件（Copy 模式）：保留源文件，仅将副本写入库目录。
fn copy_file(src: &Path, dst: &Path) -> Result<(), IngestError> {
    if src == dst {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// 生成 512px WebP 缩略图（best-effort：失败仅告警，不阻塞入库）。
pub fn generate_thumbnail(src: &Path, dst: &Path) {
    let result = (|| -> Result<(), IngestError> {
        // 按文件头嗅探解码（修复 jpg 实际是 PNG 时缩略图失败）
        let img = crate::features::decode_image(src)?;
        let (w, h) = img.dimensions();
        let scale = (THUMB_CARD_PX as f64 / w.max(h) as f64).min(1.0);
        let thumb = if scale < 1.0 {
            img.resize_exact(
                (w as f64 * scale).max(1.0) as u32,
                (h as f64 * scale).max(1.0) as u32,
                image::imageops::FilterType::Lanczos3,
            )
        } else {
            img
        };
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        thumb
            .save_with_format(dst, image::ImageFormat::WebP)
            .map_err(|source| IngestError::Image {
                path: dst.display().to_string(),
                source,
            })?;
        Ok(())
    })();
    if let Err(e) = result {
        warn!(path = %dst.display(), error = %e, "缩略图生成失败（已跳过）");
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 重新解析解码失败的图片（width=0 或 height=0）。
/// 用按文件头嗅探的新解码逻辑重新提取尺寸/清晰度/phash，并重新生成缩略图。
/// 返回处理成功的数量。
pub fn reprocess_broken_images(
    db: &Db,
    library_dir: &Path,
    thumbs_dir: &Path,
) -> Result<(usize, usize), IngestError> {
    let ids = db.list_broken_images(1000)?;
    let mut ok = 0usize;
    let mut failed = 0usize;
    for id in ids {
        let Some(img) = db.get_image_by_id(id)? else { continue };
        let src = library_dir.join(&img.rel_path);
        // 解码（新逻辑：按文件头嗅探）
        match crate::features::decode_image(&src) {
            Ok(dyn_img) => {
                let (w, h) = dyn_img.dimensions();
                let clarity = crate::clarity::clarity(&dyn_img);
                let phash = crate::phash::phash(&dyn_img) as i64;
                db.update_image_dimensions(id, w as i64, h as i64, clarity, phash)?;
                // 重新生成缩略图
                let thumb_rel = img.thumb_rel.clone();
                let thumb_path = thumbs_dir.join(&thumb_rel);
                generate_thumbnail(&src, &thumb_path);
                ok += 1;
                info!(image_id = id, w, h, "重新解析成功");
            }
            Err(e) => {
                warn!(image_id = id, error = %e, "重新解析仍失败");
                failed += 1;
            }
        }
    }
    Ok((ok, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    fn make_img(dir: &Path, name: &str, color: [u8; 3]) -> PathBuf {
        let p = dir.join(name);
        RgbImage::from_pixel(64, 48, image::Rgb(color)).save(&p).unwrap();
        p
    }

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "moevault_import_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn import_moves_files_and_indexes() {
        let root = temp_root("ok");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let a = make_img(&src_dir, "a.png", [1, 2, 3]);
        let b = make_img(&src_dir, "b.jpg", [10, 20, 30]);

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let batch_id = db.create_import_batch(src_dir.to_str().unwrap()).unwrap();

        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");
        let progress = run_import(
            &db,
            batch_id,
            vec![src_dir.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Move,
        )
        .unwrap();

        assert_eq!(progress.total, 2);
        assert_eq!(progress.done, 2);
        assert_eq!(progress.failed, 0);
        assert_eq!(progress.duplicate, 0);

        // 源文件已被移走
        assert!(!a.exists());
        assert!(!b.exists());
        // 库中已有 2 张
        assert_eq!(db.count_images("active").unwrap(), 2);
        // 批次完成
        let batch = db.get_import_batch(batch_id).unwrap().unwrap();
        assert_eq!(batch.state, "done");
        assert_eq!(batch.done, 2);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_import_skips_and_counts() {
        let root = temp_root("dup");
        let src1 = root.join("src1");
        let src2 = root.join("src2");
        std::fs::create_dir_all(&src1).unwrap();
        std::fs::create_dir_all(&src2).unwrap();
        // 同一内容（同色图 → 相同字节）复制两份
        let p1 = make_img(&src1, "a.png", [7, 7, 7]);
        let p2 = src2.join("copy.png");
        std::fs::copy(&p1, &p2).unwrap();

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");

        let b1 = db.create_import_batch("src1").unwrap();
        let pr1 = run_import(
            &db,
            b1,
            vec![src1.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Move,
        )
        .unwrap();
        assert_eq!(pr1.done, 1);

        // 第二次导入相同内容 → duplicate
        let b2 = db.create_import_batch("src2").unwrap();
        let pr2 = run_import(
            &db,
            b2,
            vec![src2.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Move,
        )
        .unwrap();
        assert_eq!(pr2.duplicate, 1);
        assert_eq!(pr2.done, 0);
        assert!(!p2.exists(), "重复源文件应被移出原位");
        let recycled: Vec<_> = std::fs::read_dir(recycle.join("import_duplicate"))
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        assert!(
            recycled.iter().any(|n| n.contains("copy")),
            "重复源文件应进入回收站 import_duplicate/，实际 {recycled:?}"
        );

        assert_eq!(db.count_images("active").unwrap(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_within_same_batch_counts_once() {
        let root = temp_root("dup_in_batch");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // 同批次内两份字节相同的文件
        let p1 = make_img(&src, "a.png", [9, 9, 9]);
        let p2 = src.join("b.png");
        std::fs::copy(&p1, &p2).unwrap();

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");
        let b = db.create_import_batch("src").unwrap();
        let pr = run_import(
            &db,
            b,
            vec![src.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Move,
        )
        .unwrap();

        assert_eq!(pr.total, 2);
        assert_eq!(pr.done, 1, "批内重复应只入库 1 张");
        assert_eq!(pr.duplicate, 1, "批内重复应计数 1");
        assert_eq!(db.count_images("active").unwrap(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unsupported_files_are_failed() {
        let root = temp_root("bad");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("note.txt"), b"not an image").unwrap();

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");
        let b = db.create_import_batch("src").unwrap();
        let pr = run_import(
            &db,
            b,
            vec![src],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Move,
        )
        .unwrap();
        assert_eq!(pr.total, 0, "不支持的文件应在扫描阶段被过滤");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn import_copy_keeps_source_files() {
        let root = temp_root("copy");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let a = make_img(&src_dir, "a.png", [1, 2, 3]);
        let b = make_img(&src_dir, "b.jpg", [10, 20, 30]);

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let batch_id = db.create_import_batch(src_dir.to_str().unwrap()).unwrap();

        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");
        let progress = run_import(
            &db,
            batch_id,
            vec![src_dir.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Copy,
        )
        .unwrap();

        assert_eq!(progress.total, 2);
        assert_eq!(progress.done, 2);
        assert_eq!(progress.failed, 0);
        assert_eq!(progress.duplicate, 0);

        // Copy 模式：源文件保留
        assert!(a.exists());
        assert!(b.exists());
        // 库中已有 2 张
        assert_eq!(db.count_images("active").unwrap(), 2);

        std::fs::remove_dir_all(&root).ok();
    }

    /// 用户明确要求：重复图一律移入回收站（copy 模式也不再原地保留），便于人工复核。
    #[test]
    fn duplicate_in_copy_mode_moves_source_to_recycle() {
        let root = temp_root("copy_dup");
        let src1 = root.join("src1");
        let src2 = root.join("src2");
        std::fs::create_dir_all(&src1).unwrap();
        std::fs::create_dir_all(&src2).unwrap();
        let p1 = make_img(&src1, "a.png", [7, 7, 7]);
        let p2 = src2.join("copy.png");
        std::fs::copy(&p1, &p2).unwrap();

        let db_path = root.join("app.db");
        let db = Db::open(&db_path).unwrap();
        let library = root.join("library");
        let thumbs = root.join("thumbs");
        let recycle = root.join("recycle");

        let b1 = db.create_import_batch("src1").unwrap();
        let pr1 = run_import(
            &db,
            b1,
            vec![src1.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Copy,
        )
        .unwrap();
        assert_eq!(pr1.done, 1);
        assert!(p1.exists(), "copy 模式首次导入后源文件应保留");

        // 第二次相同内容 → duplicate；copy 模式不删除源
        let b2 = db.create_import_batch("src2").unwrap();
        let pr2 = run_import(
            &db,
            b2,
            vec![src2.clone()],
            &library,
            &thumbs,
            &recycle,
            ImportMode::Copy,
        )
        .unwrap();
        assert_eq!(pr2.duplicate, 1);
        assert_eq!(pr2.done, 0);
        assert!(!p2.exists(), "重复源文件应移入回收站（不再原地保留）");
        assert_eq!(pr2.duplicates_recycled, 1, "应记录移入回收站的数量");
        assert!(
            recycle.join("import_duplicate").is_dir(),
            "回收站 import_duplicate 目录应存在"
        );

        assert_eq!(db.count_images("active").unwrap(), 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
