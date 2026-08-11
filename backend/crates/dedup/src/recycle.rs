//! 回收站操作：软删除（文件移入 recycle/ + 元数据保留）、恢复、永久删除。
//!
//! 语义（docs/PLAN.md 2.6）：
//! - recycle：`status -> recycled` + recycle_bin 记录 + 文件移入 `data/recycle/<rel_path>`
//! - restore：文件移回库目录 + `status -> active` + 删除回收站记录
//! - purge：删除文件 + 缩略图 + images 行（级联 image_tags）

use std::path::Path;

use moevault_core::models::{STATUS_ACTIVE, STATUS_RECYCLED};
use moevault_db::Db;
use tracing::{debug, info, warn};

use crate::DedupError;

/// 软删除图片：文件移入回收站目录，元数据保留（status=recycled）。
pub fn recycle_image(
    db: &Db,
    image_id: i64,
    reason: &str,
    library_dir: &Path,
    recycle_dir: &Path,
) -> Result<(), DedupError> {
    let img = db
        .get_image_by_id(image_id)?
        .ok_or_else(|| DedupError::NotFound(format!("图片 {image_id} 不存在")))?;
    if img.status != STATUS_ACTIVE {
        return Err(DedupError::Invalid(format!(
            "图片 {image_id} 当前状态 {}，不能回收",
            img.status
        )));
    }

    // 移动文件（库目录 → 回收站目录），保留相对路径
    let src = library_dir.join(&img.rel_path);
    let dst = recycle_dir.join(&img.rel_path);
    move_file(&src, &dst)?;

    db.set_status(image_id, STATUS_RECYCLED)?;
    db.insert_recycle_bin(image_id, reason, &img.rel_path)?;
    debug!(image_id, reason, "图片已移入回收站");
    Ok(())
}

/// 恢复图片：文件移回库目录，元数据恢复 active。
pub fn restore_image(
    db: &Db,
    image_id: i64,
    library_dir: &Path,
    recycle_dir: &Path,
) -> Result<(), DedupError> {
    let (_, original_rel, _) = db
        .get_recycle_bin(image_id)?
        .ok_or_else(|| DedupError::NotFound(format!("图片 {image_id} 不在回收站")))?;

    let src = recycle_dir.join(&original_rel);
    let dst = library_dir.join(&original_rel);
    move_file(&src, &dst)?;

    db.delete_recycle_bin(image_id)?;
    db.set_status(image_id, STATUS_ACTIVE)?;
    debug!(image_id, "图片已从回收站恢复");
    Ok(())
}

/// 永久删除图片：删除文件 + 缩略图 + 数据库行（级联 image_tags）。
pub fn purge_image(
    db: &Db,
    image_id: i64,
    library_dir: &Path,
    recycle_dir: &Path,
    thumbs_dir: &Path,
) -> Result<(), DedupError> {
    let img = db
        .get_image_by_id(image_id)?
        .ok_or_else(|| DedupError::NotFound(format!("图片 {image_id} 不存在")))?;

    // 尝试删除库文件 / 回收站文件（best-effort，任一存在即删）
    let candidates = [library_dir.join(&img.rel_path), recycle_dir.join(&img.rel_path)];
    for p in &candidates {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    let thumb = thumbs_dir.join(&img.thumb_rel);
    if thumb.exists() {
        let _ = std::fs::remove_file(&thumb);
    }

    // 先删回收站记录（recycle_bin 外键无级联），再删图片行（image_tags 级联）
    db.delete_recycle_bin(image_id)?;
    // 若该图是某查重组的 best_image，先解除引用（dedup_groups.best_image 无级联）
    db.unset_group_best_ref(image_id)?;
    db.delete_image_row(image_id)?;
    debug!(image_id, "图片已永久删除");
    Ok(())
}

/// 清空回收站：永久删除全部回收站项。返回删除数量。
pub fn purge_all(
    db: &Db,
    library_dir: &Path,
    recycle_dir: &Path,
    thumbs_dir: &Path,
) -> Result<usize, DedupError> {
    let (items, _) = db.list_recycled(500, None)?;
    if items.is_empty() {
        return Ok(0);
    }
    for item in &items {
        match purge_image(db, item.image_id, library_dir, recycle_dir, thumbs_dir) {
            Ok(()) => {}
            Err(e) => warn!(image_id = item.image_id, error = %e, "清空回收站：单张删除失败，继续"),
        }
    }
    let n = items.len();
    info!(count = n, "回收站已清空");
    Ok(n)
}

/// 移动文件：同卷 rename；跨卷（EXDEV）退化为 copy + remove。
fn move_file(src: &Path, dst: &Path) -> Result<(), DedupError> {
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
