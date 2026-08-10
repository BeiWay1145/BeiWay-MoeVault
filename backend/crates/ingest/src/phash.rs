//! pHash（64-bit DCT 感知哈希）。
//!
//! 算法：缩放 32×32 → 灰度 → 可分 2D DCT → 取 8×8 低频系数 →
//! 与均值比较生成 64 bit。汉明距离越小越相似。

use image::DynamicImage;

/// 一维 DCT-II（非归一化，与 scipy.fftpack.dct type=2 norm=None 一致，
/// 保证跨频率系数幅度可比——标准 pHash 的做法）。
fn dct_1d(input: &[f64]) -> Vec<f64> {
    let n = input.len();
    (0..n)
        .map(|k| {
            input
                .iter()
                .enumerate()
                .map(|(x, v)| {
                    v * (std::f64::consts::PI * (2.0 * x as f64 + 1.0) * k as f64 / (2.0 * n as f64)).cos()
                })
                .sum()
        })
        .collect()
}

/// 计算图片的 64-bit DCT pHash。
pub fn phash(img: &DynamicImage) -> u64 {
    const N: usize = 32;
    const BLOCK: usize = 8;

    let small = img.resize_exact(N as u32, N as u32, image::imageops::FilterType::Lanczos3);
    let gray = small.to_luma8();

    // 像素矩阵
    let mut m = vec![vec![0.0f64; N]; N];
    for (y, row) in m.iter_mut().enumerate() {
        for (x, v) in row.iter_mut().enumerate() {
            *v = gray.get_pixel(x as u32, y as u32).0[0] as f64;
        }
    }

    // 行 DCT
    let rows: Vec<Vec<f64>> = m.iter().map(|row| dct_1d(row)).collect();
    // 列 DCT
    let mut dct2: Vec<Vec<f64>> = vec![vec![0.0; N]; N];
    for (x, dct2_col) in dct2.iter_mut().enumerate() {
        let col: Vec<f64> = (0..N).map(|y| rows[y][x]).collect();
        let transformed = dct_1d(&col);
        for (y, v) in transformed.iter().enumerate() {
            dct2_col[y] = *v;
        }
    }

    // 取 8×8 低频系数
    let mut coeffs = Vec::with_capacity(BLOCK * BLOCK);
    for row in dct2.iter().take(BLOCK) {
        coeffs.extend_from_slice(&row[..BLOCK]);
    }
    // 标准 pHash：用中位数作为阈值（均值会被 DC 分量拉高，导致仅 DC 置位）
    let mut sorted = coeffs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = sorted[sorted.len() / 2];

    let mut hash: u64 = 0;
    for (i, c) in coeffs.iter().enumerate() {
        if *c > threshold {
            hash |= 1u64 << i;
        }
    }
    hash
}

/// 两个 64-bit 哈希的汉明距离。
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// 确定性复合图案：渐变背景 + 同心圆 + 棋盘（seed 偏移棋盘相位）。
    /// 特征尺寸按图幅比例缩放，保证不同分辨率下内容同构（真实图片的代表）。
    fn pattern_image(w: u32, h: u32, seed: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let scale = (w.min(h)) as f64 / 320.0;
        let ring_step = 14.0 * scale;
        let cell = (24.0 * scale) as u32;
        for (x, y, px) in img.enumerate_pixels_mut() {
            let bg = (x * 255 / w.max(1)) as u8;
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let ring = ((r / ring_step) as u32).is_multiple_of(2);
            let check = (x / cell + y / cell + seed).is_multiple_of(2);
            let v = if ring {
                255
            } else if check {
                50 + (seed * 47 % 180) as u8
            } else {
                bg
            };
            *px = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn identical_images_have_same_phash() {
        let a = phash(&pattern_image(320, 240, 1));
        let b = phash(&pattern_image(320, 240, 1));
        assert_eq!(a, b);
        // 同内容不同分辨率：缩放插值会带来细微差异，汉明距离应很小
        // （M3 查重正是基于距离阈值聚类，而非要求完全相等）
        let c = phash(&pattern_image(1024, 768, 1));
        let d = hamming(a, c);
        let e = phash(&pattern_image(480, 360, 1));
        let d2 = hamming(a, e);
        let f = phash(&pattern_image(640, 480, 1));
        let d3 = hamming(a, f);
        // pHash 覆盖"格式/压缩/小幅重采样"的重复；大幅跨分辨率由 M3 距离阈值
        // + 后期 CLIP 近重复识别补充（见 docs/PLAN.md 5.1）
        assert!(d2 <= 4, "同内容小幅重采样汉明距离应很小，实际 {d2}");
        assert!(d3 <= 8, "同内容中等重采样汉明距离应在查重阈值内，实际 {d3}");
        assert!(d <= 16, "同内容大幅重采样汉明距离应有限，实际 {d}");
    }

    #[test]
    fn different_images_differ() {
        let a = phash(&pattern_image(320, 240, 1));
        let b = phash(&pattern_image(320, 240, 2));
        assert_ne!(a, b);
        let d = hamming(a, b);
        assert!(d > 4, "不同图案应有多位差异，实际汉明距离 {d}");
    }

    #[test]
    fn hamming_distance_works() {
        assert_eq!(hamming(0b0000, 0b0000), 0);
        assert_eq!(hamming(0b1010, 0b1111), 2);
    }
}
