//! 图片特征提取：MD5 / pHash / 清晰度 / 尺寸 / 格式 / EXIF 日期。

use std::fs::File;
use std::path::Path;

use image::GenericImageView;
use md5::{Digest, Md5};

use crate::IngestError;

/// 单张图片的完整特征（入库所需）。
#[derive(Debug, Clone)]
pub struct ImageFeatures {
    pub md5: String,
    pub phash: u64,
    pub width: u32,
    pub height: u32,
    /// 归一化小写扩展名（无点），如 jpg/png/webp。
    pub format: String,
    pub size_bytes: i64,
    /// 清晰度（对数归一化 Laplacian 方差）。
    pub clarity: f64,
    /// EXIF 拍摄日期（Unix 秒），无则 None。
    pub exif_datetime: Option<i64>,
}

/// 计算文件 MD5（流式，16 进制小写）。
pub fn file_md5(path: &Path) -> Result<String, IngestError> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 从源文件提取全部特征。
pub fn extract_features(path: &Path) -> Result<ImageFeatures, IngestError> {
    let md5 = file_md5(path)?;
    let size_bytes = std::fs::metadata(path).map(|m| m.len() as i64)?;

    let img = image::open(path).map_err(|source| IngestError::Image {
        path: path.display().to_string(),
        source,
    })?;
    let (width, height) = img.dimensions();
    let phash = crate::phash::phash(&img);
    let clarity = crate::clarity::clarity(&img);
    let format = extension_of(path).unwrap_or_else(|| "unknown".to_string());
    let exif_datetime = crate::exif::exif_datetime(path);

    Ok(ImageFeatures {
        md5,
        phash,
        width,
        height,
        format,
        size_bytes,
        clarity,
        exif_datetime,
    })
}

/// 取小写扩展名（无点）。
pub fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(content: &[u8], ext: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "moevault_feat_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("a.{ext}"));
        let mut f = File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    #[test]
    fn md5_is_stable_and_hex() {
        let p = temp_file(b"hello world", "txt");
        let h1 = file_md5(&p).unwrap();
        let h2 = file_md5(&p).unwrap();
        assert_eq!(h1, h2);
        // "hello world" 的 md5
        assert_eq!(h1, "5eb63bbbe01eeed093cb22bb8f5acdc3");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn extract_features_from_png() {
        // 用 image crate 生成一张小 PNG
        let dir = std::env::temp_dir().join(format!(
            "moevault_feat_png_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.png");
        let img = image::RgbImage::from_pixel(64, 48, image::Rgb([10, 200, 30]));
        img.save(&path).unwrap();

        let feats = extract_features(&path).unwrap();
        assert_eq!(feats.format, "png");
        assert_eq!(feats.width, 64);
        assert_eq!(feats.height, 48);
        assert_eq!(feats.md5.len(), 32);
        assert!(feats.clarity.is_finite());
        std::fs::remove_dir_all(&dir).ok();
    }
}
