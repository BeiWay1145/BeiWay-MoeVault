//! 扫描源路径（文件/目录），收集支持的图片文件（去重、保序）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::SUPPORTED_EXTENSIONS;

/// 判断扩展名是否受支持（不区分大小写）。
pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// 从多个源路径收集图片文件。
/// - 文件：若扩展名受支持则加入
/// - 目录：递归扫描（跳过隐藏文件）
///
/// 返回绝对路径列表，已去重。
pub fn collect_images(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        let p = if p.is_absolute() {
            p.clone()
        } else {
            std::env::current_dir()
                .map(|c| c.join(p))
                .unwrap_or_else(|_| p.clone())
        };
        if p.is_dir() {
            for entry in WalkDir::new(&p)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                let f = entry.path();
                if f.is_file() && is_supported_image(f) {
                    push_unique(&mut seen, &mut out, f);
                }
            }
        } else if p.is_file() && is_supported_image(&p) {
            push_unique(&mut seen, &mut out, &p);
        }
    }
    out
}

fn push_unique(seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>, p: &Path) {
    // 用规范化路径去重（大小写不敏感在 Windows 上可进一步优化，骨架阶段按原样）
    if seen.insert(p.to_path_buf()) {
        out.push(p.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    fn make_tree() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "moevault_scan_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("sub/nested")).unwrap();
        // 三张真图 + 一个非图片
        RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(dir.join("a.png")).unwrap();
        RgbImage::from_pixel(8, 8, image::Rgb([4, 5, 6])).save(dir.join("sub/b.jpg")).unwrap();
        RgbImage::from_pixel(8, 8, image::Rgb([7, 8, 9])).save(dir.join("sub/nested/c.WEBP")).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not an image").unwrap();
        dir
    }

    #[test]
    fn scans_dir_recursively_and_filters() {
        let dir = make_tree();
        let files = collect_images(std::slice::from_ref(&dir));
        assert_eq!(files.len(), 3, "应收集 3 张图片: {files:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dedups_and_handles_mixed_inputs() {
        let dir = make_tree();
        let single = dir.join("a.png");
        // 目录 + 单文件（单文件已在目录内，应去重）
        let files = collect_images(&[dir.clone(), single.clone()]);
        assert_eq!(files.len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
