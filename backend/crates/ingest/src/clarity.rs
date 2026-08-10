//! 清晰度评分：Laplacian 方差（对数归一化）。
//!
//! 对灰度图做 3×3 Laplacian 卷积，方差越大越清晰。
//! 存储 `ln(1 + variance)` 以便不同分辨率可比（docs/PLAN.md 5.2）。

use image::{DynamicImage, GenericImageView, GrayImage};

const LAP: [[i32; 3]; 3] = [[0, 1, 0], [1, -4, 1], [0, 1, 0]];

/// 计算清晰度分数（对数归一化 Laplacian 方差）。
pub fn clarity(img: &DynamicImage) -> f64 {
    // 先缩到最长边 512 以内加速（足够支撑方差统计）
    let small = {
        let (w, h) = img.dimensions();
        let max_dim = w.max(h);
        if max_dim > 512 {
            let scale = 512.0 / max_dim as f64;
            img.resize_exact(
                (w as f64 * scale).max(1.0) as u32,
                (h as f64 * scale).max(1.0) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            img.clone()
        }
    };
    let gray = small.to_luma8();
    laplacian_variance(&gray)
}

fn laplacian_variance(gray: &GrayImage) -> f64 {
    let (w, h) = gray.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut n = 0.0f64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut acc = 0i32;
            for ky in 0..3i32 {
                for kx in 0..3i32 {
                    let px = (x as i32 + kx - 1) as u32;
                    let py = (y as i32 + ky - 1) as u32;
                    let v = gray.get_pixel(px, py).0[0] as i32;
                    acc += LAP[ky as usize][kx as usize] * v;
                }
            }
            let v = acc as f64;
            sum += v;
            sum_sq += v * v;
            n += 1.0;
        }
    }
    if n == 0.0 {
        return 0.0;
    }
    let mean = sum / n;
    let var = (sum_sq / n - mean * mean).max(0.0);
    var.ln_1p() // ln(1 + variance) 对数归一化
}

/// 生成 8-bit 灰度图（内部工具，供测试/其他模块使用）。
#[allow(dead_code)]
pub fn to_luma8(img: &DynamicImage) -> GrayImage {
    img.to_luma8()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// 纯色图（方差 0）。
    fn solid(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for px in img.pixels_mut() {
            *px = Rgb([128, 128, 128]);
        }
        DynamicImage::ImageRgb8(img)
    }

    /// 棋盘图（高频 → 方差大）。
    fn checker(w: u32, h: u32, cell: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let on = (x / cell + y / cell).is_multiple_of(2);
            *px = if on { Rgb([255, 255, 255]) } else { Rgb([0, 0, 0]) };
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn sharp_image_scores_higher_than_flat() {
        let flat = clarity(&solid(100, 100));
        let sharp = clarity(&checker(100, 100, 2));
        assert!(sharp > flat, "sharp={sharp} flat={flat}");
    }

    #[test]
    fn tiny_image_does_not_panic() {
        assert_eq!(clarity(&solid(2, 2)), 0.0);
        assert!(clarity(&checker(1, 1, 1)).is_finite());
    }

    #[test]
    fn blur_lowers_score() {
        let sharp = checker(128, 128, 2);
        let blur = image::imageops::blur(&sharp, 3.0);
        let s = clarity(&sharp);
        let b = clarity(&DynamicImage::ImageRgba8(blur));
        assert!(s > b, "sharp={s} blur={b}");
    }
}
