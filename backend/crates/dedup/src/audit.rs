//! 回收站一致性审计与修复。
//!
//! 背景（BUG-B / BUG-C）：历史上有多处「搬了文件但不登记元数据」的路径
//! （`replace_from_url`、`replace.rs`、查重 resolve 的半途失败），累积出三类残局：
//!
//! - **半完成**：文件已在 `data/recycle/`，但 `images.status` 仍是 `active`
//!   且 `recycle_bin` 无记录 → 图库显示一张打开就 404 的图，回收/恢复都会报错
//! - **僵尸**：`status=recycled` 但没有 `recycle_bin` 记录（回收站页面看不见），
//!   且文件已经不在了 → 既恢复不了也删不掉
//! - **孤儿文件**：`data/recycle/` 里有文件，但没有任何 `images.rel_path`
//!   或 `recycle_bin.original_rel` 引用它 → 白占磁盘
//!
//! 本模块只做「扫描 + 按明确规则修复」，不做任何猜测性删除：
//! 每一类残局的判定条件都写死在代码里，报告里逐项列出，修复前后可对比。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use moevault_core::models::STATUS_RECYCLED;
use moevault_db::Db;
use serde::Serialize;
use tracing::{info, warn};

use crate::DedupError;

/// 残局类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// 文件在回收站、元数据还是 active（可修复：补 status + bin 记录）
    HalfRecycled,
    /// status=recycled 但没有 bin 记录且文件已丢（可修复：清掉僵尸行）
    ZombieRecycled,
    /// status=recycled 有 bin 记录但文件已丢（可修复：清掉，否则恢复必失败）
    LostFile,
}

/// 一条图片残局。
#[derive(Debug, Clone, Serialize)]
pub struct ImageIssue {
    pub image_id: i64,
    pub rel_path: String,
    pub kind: IssueKind,
    /// 回收站目录里是否还有该文件
    pub file_in_recycle: bool,
    /// 库目录里是否还有该文件
    pub file_in_library: bool,
}

/// 一个孤儿回收站文件。
#[derive(Debug, Clone, Serialize)]
pub struct OrphanFile {
    /// 相对 `data/recycle` 的路径（反斜杠形式，与 images.rel_path 同风格）
    pub rel_path: String,
    pub bytes: u64,
}

/// 审计结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecycleAudit {
    pub issues: Vec<ImageIssue>,
    pub orphans: Vec<OrphanFile>,
    pub orphan_bytes: u64,
    /// 数据库里回收站记录数（recycle_bin 行数）
    pub bin_records: i64,
    /// status=recycled 的图片数
    pub recycled_images: i64,
    /// 回收站目录里的文件总数（含孤儿与正常项）
    pub files_on_disk: usize,
}

/// 修复动作统计。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RepairReport {
    /// 半完成态修补数量（补 status + bin 记录）
    pub healed_half: usize,
    /// 僵尸行清除数量
    pub removed_zombie: usize,
    /// 丢文件的 recycled 行清除数量
    pub removed_lost: usize,
    /// 孤儿文件删除数量
    pub removed_orphans: usize,
    /// 随之清掉的空目录数量（只在 remove_orphans 且非 dry_run 时统计）
    pub removed_empty_dirs: usize,
    /// 孤儿文件释放字节数
    pub freed_bytes: u64,
    /// 逐项失败的明细
    pub errors: Vec<String>,
    /// dry_run=true 时不做任何改动，needs 里列出「将会」发生的数量
    pub dry_run: bool,
}

/// 把相对路径统一成反斜杠小写无关比较用的键。
fn norm(rel: &str) -> String {
    rel.replace('/', "\\").to_lowercase()
}

/// 自底向上删除回收站目录下的空目录（保留 `recycle_dir` 本身）。
///
/// 孤儿文件清完后会留下大量空的哈希前缀目录，看着像没清干净。
/// 只在显式要求删孤儿时调用——空目录本身无害，不做无授权的隐式清理。
fn prune_empty_dirs(recycle_dir: &Path) -> usize {
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(recycle_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .collect();
    // 深的先删，父目录才可能变空
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut removed = 0;
    for d in dirs {
        if std::fs::remove_dir(&d).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        info!(removed, "回收站：清理空目录");
    }
    removed
}

/// 扫描回收站目录下所有文件，返回 (相对路径, 字节数)。
fn list_recycle_files(recycle_dir: &Path) -> Result<Vec<(String, u64)>, DedupError> {
    let mut out = Vec::new();
    if !recycle_dir.exists() {
        return Ok(out);
    }
    for entry in walkdir::WalkDir::new(recycle_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(recycle_dir) else { continue };
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\\");
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push((rel_str, bytes));
    }
    Ok(out)
}

/// 扫描回收站一致性（只读）。
pub fn audit_recycle(
    db: &Db,
    library_dir: &Path,
    recycle_dir: &Path,
) -> Result<RecycleAudit, DedupError> {
    let active = db.active_image_rel_paths()?;
    let bin_rels = db.all_recycle_original_rels()?;

    // 库目录里实际存在的文件集合（只查 active 图，够了）
    let mut issues = Vec::new();
    let mut known: HashSet<String> = HashSet::new();

    for (id, rel) in &active {
        known.insert(norm(rel));
        let in_lib = library_dir.join(rel).exists();
        let in_rec = recycle_dir.join(rel).exists();
        if !in_lib && in_rec {
            issues.push(ImageIssue {
                image_id: *id,
                rel_path: rel.clone(),
                kind: IssueKind::HalfRecycled,
                file_in_recycle: true,
                file_in_library: false,
            });
        }
    }

    // status=recycled 的图：分「有 bin 记录 / 无 bin 记录」两类查文件
    let recycled = db.recycled_image_rel_paths()?;
    let bin_set: HashSet<String> = bin_rels.iter().map(|r| norm(r)).collect();
    for (id, rel) in recycled {
        known.insert(norm(&rel));
        let in_rec = recycle_dir.join(&rel).exists();
        let has_bin = bin_set.contains(&norm(&rel));
        if !has_bin && !in_rec {
            issues.push(ImageIssue {
                image_id: id,
                rel_path: rel,
                kind: IssueKind::ZombieRecycled,
                file_in_recycle: false,
                file_in_library: false,
            });
        } else if has_bin && !in_rec {
            issues.push(ImageIssue {
                image_id: id,
                rel_path: rel,
                kind: IssueKind::LostFile,
                file_in_recycle: false,
                file_in_library: false,
            });
        }
    }

    let files = list_recycle_files(recycle_dir)?;
    let referenced: HashSet<String> = known.iter().cloned().collect();
    let mut orphans = Vec::new();
    let mut orphan_bytes = 0u64;
    for (rel, bytes) in &files {
        if !referenced.contains(&norm(rel)) {
            orphans.push(OrphanFile { rel_path: rel.clone(), bytes: *bytes });
            orphan_bytes += *bytes;
        }
    }

    let bin_records = db.recycle_bin_count()?;
    let recycled_images = issues
        .iter()
        .filter(|i| matches!(i.kind, IssueKind::ZombieRecycled | IssueKind::LostFile))
        .count() as i64;

    Ok(RecycleAudit {
        issues,
        orphans,
        orphan_bytes,
        bin_records,
        recycled_images,
        files_on_disk: files.len(),
    })
}

/// 修复选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct RepairOptions {
    /// 只报告不落盘
    pub dry_run: bool,
    /// 是否删除孤儿文件
    pub remove_orphans: bool,
}

/// 按审计规则修复回收站一致性。
pub fn repair_recycle(
    db: &Db,
    library_dir: &Path,
    recycle_dir: &Path,
    opts: RepairOptions,
) -> Result<RepairReport, DedupError> {
    let audit = audit_recycle(db, library_dir, recycle_dir)?;
    let mut report = RepairReport { dry_run: opts.dry_run, ..Default::default() };

    for issue in &audit.issues {
        match issue.kind {
            IssueKind::HalfRecycled => {
                if opts.dry_run {
                    report.healed_half += 1;
                    continue;
                }
                // 补「已在回收站」的元数据：status -> recycled + bin 记录
                let r = (|| -> Result<(), DedupError> {
                    db.set_status(issue.image_id, STATUS_RECYCLED)?;
                    db.insert_recycle_bin(issue.image_id, "repair", &issue.rel_path)?;
                    Ok(())
                })();
                match r {
                    Ok(()) => report.healed_half += 1,
                    Err(e) => report.errors.push(format!("图 {} 补元数据失败：{e}", issue.image_id)),
                }
            }
            IssueKind::ZombieRecycled | IssueKind::LostFile => {
                if opts.dry_run {
                    if issue.kind == IssueKind::ZombieRecycled {
                        report.removed_zombie += 1;
                    } else {
                        report.removed_lost += 1;
                    }
                    continue;
                }
                let r = (|| -> Result<(), DedupError> {
                    db.delete_recycle_bin(issue.image_id)?;
                    db.unset_group_best_ref(issue.image_id)?;
                    db.delete_image_row(issue.image_id)?;
                    Ok(())
                })();
                match r {
                    Ok(()) => {
                        if issue.kind == IssueKind::ZombieRecycled {
                            report.removed_zombie += 1;
                        } else {
                            report.removed_lost += 1;
                        }
                    }
                    Err(e) => report
                        .errors
                        .push(format!("图 {} 清理失败：{e}", issue.image_id)),
                }
            }
        }
    }

    if opts.remove_orphans {
        for orphan in &audit.orphans {
            if opts.dry_run {
                report.removed_orphans += 1;
                report.freed_bytes += orphan.bytes;
                continue;
            }
            let path: PathBuf = recycle_dir.join(&orphan.rel_path);
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    report.removed_orphans += 1;
                    report.freed_bytes += orphan.bytes;
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        // 已被并发清掉，按成功算
                        report.removed_orphans += 1;
                        report.freed_bytes += orphan.bytes;
                    } else {
                        report
                            .errors
                            .push(format!("孤儿文件删除失败 {}：{e}", orphan.rel_path));
                    }
                }
            }
        }
    }

    // 孤儿文件删完后，哈希前缀目录会剩一堆空壳（实测清 228 个文件留下 256 个空目录）。
    // 留着不影响正确性，但会让「回收站已清空」看起来没清干净，顺手自底向上收掉。
    if opts.remove_orphans && !opts.dry_run {
        report.removed_empty_dirs = prune_empty_dirs(recycle_dir);
    }

    if !opts.dry_run {
        info!(
            healed = report.healed_half,
            zombie = report.removed_zombie,
            lost = report.removed_lost,
            orphans = report.removed_orphans,
            empty_dirs = report.removed_empty_dirs,
            freed = report.freed_bytes,
            "回收站一致性修复完成"
        );
        if !report.errors.is_empty() {
            warn!(count = report.errors.len(), "回收站修复存在失败项");
        }
    }
    Ok(report)
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
            "moevault_audit_test_{}_{}.db",
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

    fn test_dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "moevault_audit_{tag}_{}_{}",
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

    fn write_file(dir: &Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
    }

    /// 三类残局各造一条，审计应分别识别、孤儿文件应统计字节数。
    #[test]
    fn audit_detects_three_kinds() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("kinds");
        db.insert_images(&[
            make_image("a1", "a1/a.png"),
            make_image("b1", "b1/b.png"),
            make_image("c1", "c1/c.png"),
        ])
        .unwrap();
        let ids: Vec<i64> = db.active_image_rel_paths().unwrap().into_iter().map(|(i, _)| i).collect();
        // a1：正常图（库里有文件）
        write_file(&lib, "a1/a.png", b"a");
        // b1：半完成态（文件只在回收站）
        write_file(&rec, "b1/b.png", b"bb");
        // c1：僵尸（改状态为 recycled，既无 bin 记录也无文件）
        db.set_status(ids[2], STATUS_RECYCLED).unwrap();
        // 孤儿文件
        write_file(&rec, "zz/orphan.png", b"orphan");

        let a = audit_recycle(&db, &lib, &rec).unwrap();
        assert_eq!(a.issues.len(), 2, "应识别半完成 + 僵尸各一条：{:?}", a.issues);
        assert!(a.issues.iter().any(|i| i.kind == IssueKind::HalfRecycled && i.image_id == ids[1]));
        assert!(a.issues.iter().any(|i| i.kind == IssueKind::ZombieRecycled && i.image_id == ids[2]));
        assert_eq!(a.orphans.len(), 1, "孤儿文件应只有一个：{:?}", a.orphans);
        assert_eq!(a.orphans[0].rel_path, "zz\\orphan.png");
        assert_eq!(a.orphan_bytes, 6);
        assert_eq!(a.bin_records, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// dry_run 只报数，不改任何状态与文件。
    #[test]
    fn repair_dry_run_changes_nothing() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("dry");
        db.insert_images(&[make_image("a2", "a2/a.png"), make_image("c2", "c2/c.png")])
            .unwrap();
        let ids: Vec<i64> = db.active_image_rel_paths().unwrap().into_iter().map(|(i, _)| i).collect();
        write_file(&rec, "a2/a.png", b"a");
        db.set_status(ids[1], STATUS_RECYCLED).unwrap();
        write_file(&rec, "zz/o.png", b"o");

        let rep = repair_recycle(&db, &lib, &rec, RepairOptions { dry_run: true, remove_orphans: true })
            .unwrap();
        assert_eq!(rep.healed_half, 1);
        assert_eq!(rep.removed_zombie, 1);
        assert_eq!(rep.removed_orphans, 1);
        // 数据没动
        assert_eq!(db.get_image_by_id(ids[0]).unwrap().unwrap().status, STATUS_ACTIVE);
        assert!(db.get_image_by_id(ids[1]).unwrap().is_some());
        assert!(rec.join("zz/o.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 真修复：半完成转正、僵尸行删除、孤儿文件删除。
    #[test]
    fn repair_heals_and_cleans() {
        let db = test_db();
        let (root, lib, rec) = test_dirs("fix");
        db.insert_images(&[make_image("a3", "a3/a.png"), make_image("c3", "c3/c.png")])
            .unwrap();
        let ids: Vec<i64> = db.active_image_rel_paths().unwrap().into_iter().map(|(i, _)| i).collect();
        write_file(&rec, "a3/a.png", b"a");
        db.set_status(ids[1], STATUS_RECYCLED).unwrap();
        write_file(&rec, "zz/o.png", b"orphan");

        let rep = repair_recycle(&db, &lib, &rec, RepairOptions { dry_run: false, remove_orphans: true })
            .unwrap();
        assert_eq!(rep.healed_half, 1);
        assert_eq!(rep.removed_zombie, 1);
        assert_eq!(rep.removed_orphans, 1);
        assert_eq!(rep.freed_bytes, 6);
        // 孤儿删掉后留下的空目录顺手收掉（zz/）
        assert_eq!(rep.removed_empty_dirs, 1, "应清掉空目录 zz/");
        assert!(rep.errors.is_empty(), "不该有失败项：{:?}", rep.errors);

        // 半完成的那张变成正式回收，能在回收站列表里看到
        assert_eq!(db.get_image_by_id(ids[0]).unwrap().unwrap().status, STATUS_RECYCLED);
        let (items, _) = db.list_recycled(10, None).unwrap();
        assert!(items.iter().any(|i| i.image_id == ids[0]), "修复后应出现在回收站列表");
        // 僵尸行被清掉
        assert!(db.get_image_by_id(ids[1]).unwrap().is_none());
        // 孤儿文件被删
        assert!(!rec.join("zz/o.png").exists());
        // 再审计应干净
        let a = audit_recycle(&db, &lib, &rec).unwrap();
        assert!(a.issues.is_empty(), "修复后不该还有问题：{:?}", a.issues);
        assert!(a.orphans.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
