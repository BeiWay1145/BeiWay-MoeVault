//! pHash 聚类：把内容相同（感知哈希相近）的图片归组。
//!
//! 算法（docs/PLAN.md 5.1）：
//! - 每簇以首个成员的 64-bit pHash 为种子
//! - 新图与全部簇种子比较汉明距离，≤ 阈值（默认 8）入簇，否则开新簇
//! - 簇内 refresh：按清晰度（Laplacian 方差）降序，最优为 best，其余 is_redundant=1

use moevault_db::Db;
use tracing::{debug, info};

use crate::DedupError;

/// 默认汉明距离阈值（可配置）。
pub const DEFAULT_HAMMING_THRESHOLD: u32 = 8;

/// 聚类结果统计。
#[derive(Debug, Clone, Default)]
pub struct ClusterStats {
    /// 新建簇数。
    pub groups_created: usize,
    /// 归入已有簇的图片数。
    pub images_clustered: usize,
    /// 新标记为冗余候选的图片数。
    pub redundant_marked: usize,
}

/// 汉明距离（64-bit）。
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 增量聚类：处理所有 `status='active' AND dedup_group IS NULL` 的图片。
/// 已分组的图片不受影响；新图入现有簇或开新簇，随后刷新受影响簇的 best/redundant。
pub fn incremental_cluster(db: &Db, threshold: u32) -> Result<ClusterStats, DedupError> {
    let unclustered = db.unclustered_active_images()?;
    if unclustered.is_empty() {
        return Ok(ClusterStats::default());
    }
    debug!(count = unclustered.len(), "增量聚类：待处理图片");

    let mut seeds = db.cluster_seeds()?; // (group_id, seed_phash)
    let mut stats = ClusterStats::default();
    let mut touched: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();

    for (image_id, phash, _clarity) in &unclustered {
        let phash = *phash as u64;
        // 找距离最近的簇种子
        let mut best_dist = u32::MAX;
        let mut best_group: Option<i64> = None;
        for (gid, seed) in &seeds {
            let d = hamming(phash, *seed as u64);
            if d < best_dist {
                best_dist = d;
                best_group = Some(*gid);
            }
        }

        if let Some(gid) = best_group {
            if best_dist <= threshold {
                db.assign_to_group(*image_id, gid)?;
                touched.insert(gid);
                stats.images_clustered += 1;
                continue;
            }
        }
        // 未命中 → 开新簇（种子 = 该图 phash），并归入该簇
        let gid = db.create_dedup_group(phash as i64, *image_id)?;
        db.assign_to_group(*image_id, gid)?;
        seeds.push((gid, phash as i64));
        touched.insert(gid);
        stats.groups_created += 1;
    }

    // 刷新受影响簇
    for gid in &touched {
        stats.redundant_marked += refresh_group(db, *gid)?;
    }
    info!(
        groups_created = stats.groups_created,
        images_clustered = stats.images_clustered,
        redundant_marked = stats.redundant_marked,
        "增量聚类完成"
    );
    Ok(stats)
}

/// 全量重建：清空全部查重结果后重新聚类。
/// 贪心：按 id 顺序，每张图与已建簇种子比较（O(n·k)，k=簇数）。
pub fn full_recluster(db: &Db, threshold: u32) -> Result<ClusterStats, DedupError> {
    db.clear_dedup()?;
    let all = db.all_active_images()?;
    info!(count = all.len(), "全量聚类：开始");

    let mut stats = ClusterStats::default();
    let mut seeds: Vec<(i64, i64)> = Vec::new();

    for (image_id, phash, _clarity) in &all {
        let phash = *phash as u64;
        let mut best_dist = u32::MAX;
        let mut best_group: Option<i64> = None;
        for (gid, seed) in &seeds {
            let d = hamming(phash, *seed as u64);
            if d < best_dist {
                best_dist = d;
                best_group = Some(*gid);
            }
        }
        if let Some(gid) = best_group {
            if best_dist <= threshold {
                db.assign_to_group(*image_id, gid)?;
                stats.images_clustered += 1;
                continue;
            }
        }
        // 未命中 → 开新簇（种子 = 该图 phash），并归入该簇
        let gid = db.create_dedup_group(phash as i64, *image_id)?;
        db.assign_to_group(*image_id, gid)?;
        seeds.push((gid, phash as i64));
        stats.groups_created += 1;
    }

    // 全量刷新所有组（best/redundant）
    for (gid, _) in &seeds {
        stats.redundant_marked += refresh_group(db, *gid)?;
    }
    info!(
        groups_created = stats.groups_created,
        images_clustered = stats.images_clustered,
        redundant_marked = stats.redundant_marked,
        "全量聚类完成"
    );
    Ok(stats)
}

/// 刷新单簇：按清晰度降序选 best，其余 active 成员标记冗余候选。
/// 返回新标记为冗余的成员数。
pub fn refresh_group(db: &Db, group_id: i64) -> Result<usize, DedupError> {
    let mut members = db.group_members_active(group_id)?; // (image_id, clarity)
    if members.is_empty() {
        return Ok(0);
    }
    // 清晰度降序（相同则 id 小优先，保持稳定）
    members.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let best_id = members[0].0;
    db.set_group_best(group_id, best_id)?;
    db.set_redundant(best_id, false)?;

    let mut marked = 0;
    for (image_id, _) in members.iter().skip(1) {
        db.set_redundant(*image_id, true)?;
        marked += 1;
    }
    Ok(marked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moevault_core::models::{Image, STATUS_ACTIVE, SOURCE_LOCAL};

    fn make_image(md5: &str, phash: u64, clarity: f64) -> Image {
        Image {
            id: 0,
            md5: md5.to_string(),
            phash: phash as i64,
            rel_path: format!("{md5}.png"),
            width: 100,
            height: 100,
            format: "png".into(),
            size_bytes: 1,
            file_mtime: 0,
            exif_datetime: None,
            clarity_score: clarity,
            aesthetic_score: None,
            dedup_group: None,
            is_redundant: false,
            status: STATUS_ACTIVE.into(),
            source: SOURCE_LOCAL.into(),
            source_url: None,
            thumb_rel: format!("{md5}.webp"),
            imported_at: 0,
        }
    }

    fn test_db() -> Db {
        let path = std::env::temp_dir().join(format!(
            "moevault_dedup_test_{}_{}.db",
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

    fn insert(db: &Db, imgs: Vec<Image>) {
        db.insert_images(&imgs).unwrap();
    }

    #[test]
    fn cluster_groups_similar_phash() {
        let db = test_db();
        // 3 组：A 组 3 张相近（距离 0/1/1），B 组 2 张，C 组 1 张独立
        // 组间用 9 个连续 bit 置位保证汉明距离 > 8（默认阈值）
        let a0 = 0b0000_0000_0000_0000_0000_0000_0000_0000u64;
        let a1 = a0 | 0b1;
        let a2 = a0 | 0b10;
        let b0 = (1u64 << 40) | (1u64 << 41) | (1u64 << 42) | (1u64 << 43) | (1u64 << 44)
            | (1u64 << 45) | (1u64 << 46) | (1u64 << 47) | (1u64 << 48); // 距 a0 = 9 > 8
        let b1 = b0 | 0b1;
        let c0 = (1u64 << 55) | (1u64 << 56) | (1u64 << 57) | (1u64 << 58) | (1u64 << 59)
            | (1u64 << 60) | (1u64 << 61) | (1u64 << 62) | (1u64 << 63); // 距 a0 = 9，距 b0 = 18

        insert(
            &db,
            vec![
                make_image("a1", a1, 5.0),
                make_image("a0", a0, 9.0),
                make_image("a2", a2, 3.0),
                make_image("b0", b0, 7.0),
                make_image("b1", b1, 2.0),
                make_image("c0", c0, 6.0),
            ],
        );

        let stats = full_recluster(&db, 8).unwrap();
        assert_eq!(stats.groups_created, 3);
        assert_eq!(stats.images_clustered, 3); // a1,a2 入 a0；b1 入 b0
        // 验证 A 簇：3 成员，最优为 a0（clarity 9），其余冗余
        let groups = db.list_dedup_groups(100, None).unwrap().0;
        assert_eq!(groups.len(), 3);
        let group_a = groups.iter().find(|g| g.size == 3).expect("A 组应存在");
        assert_eq!(group_a.redundant_count, 2);
        // B 组 2 成员 1 冗余
        let group_b = groups.iter().find(|g| g.size == 2).expect("B 组应存在");
        assert_eq!(group_b.redundant_count, 1);
        let stats = db.dedup_stats().unwrap();
        assert_eq!(stats.redundant_count, 3);
    }

    #[test]
    fn incremental_clusters_only_new() {
        let db = test_db();
        let a0 = 0x0000_0000_0000_0000u64;
        let a1 = a0 | 1;
        let b0 = (1u64 << 40) | (1u64 << 41) | (1u64 << 42) | (1u64 << 43) | (1u64 << 44)
            | (1u64 << 45) | (1u64 << 46) | (1u64 << 47) | (1u64 << 48); // 距 a0 = 9 > 8

        insert(&db, vec![make_image("a0", a0, 9.0), make_image("b0", b0, 5.0)]);
        let s1 = full_recluster(&db, 8).unwrap();
        assert_eq!(s1.groups_created, 2);

        // 增量：新图 a1 应入 a0 簇；新独立图 c0 开新簇
        let c0 = (1u64 << 55) | (1u64 << 56) | (1u64 << 57) | (1u64 << 58) | (1u64 << 59)
            | (1u64 << 60) | (1u64 << 61) | (1u64 << 62) | (1u64 << 63); // 距 a0/b0 均 > 8
        insert(&db, vec![make_image("a1", a1, 4.0), make_image("c0", c0, 3.0)]);

        let s2 = incremental_cluster(&db, 8).unwrap();
        assert_eq!(s2.groups_created, 1); // 只有 c0 开新簇
        assert_eq!(s2.images_clustered, 1); // a1 入 a0 簇
        assert_eq!(db.dedup_stats().unwrap().group_count, 3);
        // A 簇 best 仍是最清晰的 a0（已分组不重排）
        let groups = db.list_dedup_groups(100, None).unwrap().0;
        let ga = groups.iter().find(|g| g.size == 2).unwrap();
        assert_eq!(ga.redundant_count, 1);
    }

    #[test]
    fn hamming_fn() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0b1010, 0b1111), 2);
    }
}
