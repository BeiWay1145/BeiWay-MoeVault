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
///
/// 容忍「文件已在回收站」的半完成状态：替换原图（`replace_from_url` / `replace.rs`）
/// 会把旧文件搬进 `data/recycle/` 却不登记 `recycle_bin`，也不改 `status`。
/// 这种图再走回收时源文件已不存在，旧实现直接报 NotFound，导致查重「保留最优」
/// 永远回收失败（用户看到「已回收 0 张」）。现在只要目标位置已有文件
/// 就视为搬运已完成，继续补元数据；两处都没有才报明确的错误。
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
    // 幂等：已经在回收站的图直接算完成。批量回收（查重「保留最优」）里
    // 重复点击、或图先被别处回收，都不该让整批任务报错。
    if img.status == STATUS_RECYCLED {
        debug!(image_id, "图片已在回收站，回收操作按幂等处理");
        if db.get_recycle_bin(image_id)?.is_none() {
            db.insert_recycle_bin(image_id, reason, &img.rel_path)?;
        }
        return Ok(());
    }
    if img.status != STATUS_ACTIVE {
        return Err(DedupError::Invalid(format!(
            "图片 {image_id} 当前状态 {}，不能回收",
            img.status
        )));
    }

    // 移动文件（库目录 → 回收站目录），保留相对路径
    let src = library_dir.join(&img.rel_path);
    let dst = recycle_dir.join(&img.rel_path);
    move_file_recycling(&src, &dst)?;

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
    // 半完成态容忍：文件已回到库目录（例如上次恢复搬完文件但没写库），
    // 不再因为源缺失而报 NotFound，只把元数据补正。
    if src.exists() {
        move_file(&src, &dst)?;
    } else if dst.exists() {
        debug!(image_id, "回收站无文件但库目录已有，按半完成态补正元数据");
    } else {
        return Err(DedupError::NotFound(format!(
            "原图已不在磁盘上（回收站与库目录都没有），无法恢复：{}",
            original_rel
        )));
    }

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
    // 删文件失败不阻断元数据清理（否则会留下「行还在、文件已删」的更难收拾的残局）
    let candidates = [library_dir.join(&img.rel_path), recycle_dir.join(&img.rel_path)];
    for p in &candidates {
        if !p.exists() {
            continue;
        }
        if let Err(e) = std::fs::remove_file(p) {
            warn!(path = %p.display(), error = %e, "永久删除：文件删除失败，继续清理元数据");
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

/// 回收专用搬运：源不存在但目标已存在时视为「已搬过」，只补元数据。
///
/// 之所以不复用 `move_file`：那里 src 缺失会直接冒泡成
/// `IO 错误: 系统找不到指定的文件。 (os error 2)`，用户只看到一句
/// 无上下文的 500。这里把三种情形分清：
/// - src 存在 → 正常搬运
/// - src 缺失、dst 存在 → 半完成态（文件早已在回收站），返回 `Ok` 让调用方补元数据
/// - src、dst 都缺失 → `NotFound`，明确告诉用户文件已不在磁盘上
fn move_file_recycling(src: &Path, dst: &Path) -> Result<(), DedupError> {
    if src.exists() {
        return move_file(src, dst);
    }
    if dst.exists() {
        debug!(src = %src.display(), dst = %dst.display(), "源文件缺失但回收站已有同名文件，按半完成态继续");
        return Ok(());
    }
    Err(DedupError::NotFound(format!(
        "源文件与回收站副本都不存在，无法回收：{}",
        src.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use moevault_core::models::{Image, SOURCE_LOCAL, STATUS_ACTIVE};

    fn make_image(md5: &str, rel: &str) -> Image {
        Image {
            id: 0,
            md5: md5.to_string(),
            phash: 0,
            rel_path: rel.to_string(),
            width: 100,
            height: 100,
            format: "png".into(),
            size_bytes: 1,
            file_mtime: 0,
            exif_datetime: None,
            clarity_score: 5.0,
            aesthetic_score: None,
            dedup_group: None,
            is_redundant: false,
            status: STATUS_ACTIVE.into(),
            source: SOURCE_LOCAL.into(),
            source_url: None,
            no_auto_sauce: false,
            ai_metadata: None,
            thumb_rel: format!("{md5}.webp"),
            imported_at: 0,
            source_dir: None,
        }
    }

    fn test_db() -> Db {
        let path = std::env::temp_dir().join(format!(
            "moevault_recycle_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        db
    }

    /// 独立的库目录 / 回收站目录（进程内唯一），返回 (根, library, recycle)。
    fn test_dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "moevault_recycle_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let lib = root.join("library");
        let rec = root.join("recycle");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(&rec).unwrap();
        (root, lib, rec)
    }

    fn write_file(dir: &Path, rel: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, b"x").unwrap();
    }

    /// 半完成态：文件已在回收站目录、状态还是 active、recycle_bin 无记录。
    /// 这是查重「保留最优」永远报「已回收 0 张」的现场。
    #[test]
    fn recycle_heals_half_completed_state() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("half");
        db.insert_images(&[make_image("aa", "aa/a.png")]).unwrap();
        let id = db.active_image_rel_paths().unwrap()[0].0;
        // 只有回收站里有文件
        write_file(&rec, "aa/a.png");
        assert!(!lib.join("aa/a.png").exists());

        recycle_image(&db, id, "duplicate", &lib, &rec).expect("半完成态应能回收成功");

        assert!(db.get_recycle_bin(id).unwrap().is_some(), "应补上回收站记录");
        let img = db.get_image_by_id(id).unwrap().unwrap();
        assert_eq!(img.status, STATUS_RECYCLED);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 重复回收（已 recycled）应幂等成功，不报错。
    #[test]
    fn recycle_is_idempotent() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("idem");
        db.insert_images(&[make_image("bb", "bb/b.png")]).unwrap();
        let id = db.active_image_rel_paths().unwrap()[0].0;
        write_file(&lib, "bb/b.png");

        recycle_image(&db, id, "duplicate", &lib, &rec).unwrap();
        recycle_image(&db, id, "duplicate", &lib, &rec).expect("重复回收不该报错");
        assert!(db.get_recycle_bin(id).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 两处文件都没有：给明确错误，而不是裸的 os error 2。
    #[test]
    fn recycle_reports_missing_file_clearly() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("missing");
        db.insert_images(&[make_image("cc", "cc/c.png")]).unwrap();
        let id = db.active_image_rel_paths().unwrap()[0].0;
        let err = recycle_image(&db, id, "duplicate", &lib, &rec).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("无法回收"), "错误应带上下文，实际：{msg}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 恢复：文件已回到库目录（半完成），只补元数据。
    #[test]
    fn restore_tolerates_half_completed_move() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("restore");
        db.insert_images(&[make_image("dd", "dd/d.png")]).unwrap();
        let id = db.active_image_rel_paths().unwrap()[0].0;
        write_file(&lib, "dd/d.png");
        recycle_image(&db, id, "manual", &lib, &rec).unwrap();
        // 手动把文件搬回库目录，模拟「搬完文件但没写库」
        std::fs::create_dir_all(lib.join("dd")).unwrap();
        std::fs::rename(rec.join("dd/d.png"), lib.join("dd/d.png")).unwrap();

        restore_image(&db, id, &lib, &rec).expect("半完成恢复应成功");
        assert_eq!(db.get_image_by_id(id).unwrap().unwrap().status, STATUS_ACTIVE);
        assert!(db.get_recycle_bin(id).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
