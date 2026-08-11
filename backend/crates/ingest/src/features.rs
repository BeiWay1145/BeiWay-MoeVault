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
/// 图片解码失败时降级返回（尺寸 0 / phash 0 / clarity 0），确保文件仍能入库。
pub fn extract_features(path: &Path) -> Result<ImageFeatures, IngestError> {
    let md5 = file_md5(path)?;
    let size_bytes = std::fs::metadata(path).map(|m| m.len() as i64)?;
    let format = extension_of(path).unwrap_or_else(|| "unknown".to_string());

    // 解码（失败降级，不阻塞导入）
    let (width, height, phash, clarity) = match image::open(path) {
        Ok(img) => {
            let (w, h) = img.dimensions();
            (
                w,
                h,
                crate::phash::phash(&img),
                crate::clarity::clarity(&img),
            )
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "图片解码失败，降级入库");
            (0, 0, 0, 0.0)
        }
    };
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

/// AI 生成图片的解析结果。
#[derive(Debug, Clone, Default)]
pub struct AiMetadata {
    /// 是否 AI 生成（检测到 Generation data / Prompt / parameters 等关键 chunk）。
    pub is_ai: bool,
    /// 原始元信息拼接（存 ai_metadata 列）。
    pub raw: String,
    /// 提取的正向 prompt（若有）。
    pub prompt: Option<String>,
    /// 提取的负向 prompt（若有）。
    pub negative_prompt: Option<String>,
    /// 从 prompt 提取的有效生图 tag（已忽略 artist: 与质量黑名单）。
    pub tags: Vec<String>,
}

/// 质量 tag 黑名单（录入无意义，忽略）。
const QUALITY_TAGS: &[&str] = &[
    "fine fabric emphasis",
    "original",
    "official art",
    "depth of field",
    "best quality",
    "amazing quality",
    "very aesthetic",
    "absurdres",
    "masterpiece",
    "ultra detailed",
    "newest",
    "8k",
    "hdr",
    "highres",
    "paid_reward_available",
    "3d",
    "koikatsu_(medium)",
    "blender_(medium)",
];

/// 判断是否为质量 tag（归一化后大小写不敏感比较）。
fn is_quality_tag(tag: &str) -> bool {
    let norm = tag.trim().to_lowercase();
    QUALITY_TAGS.iter().any(|q| norm == *q)
}

/// 读取 AI 生成图片的元信息（PNG tEXt chunks）。
/// - `Generation data` / `Prompt`（Fooocus/ComfyUI 系）→ 确定 AI
/// - `parameters`（webui）→ 确定 AI
/// - 提取 prompt 中的有效 tag（忽略 artist: 前缀 + 质量黑名单）
pub fn read_ai_metadata(path: &Path) -> Option<AiMetadata> {
    let data = std::fs::read(path).ok()?;
    // PNG 签名
    if data.len() < 8 || &data[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut pos = 8usize;
    let mut chunks: Vec<(String, String)> = Vec::new();
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let chunk_start = pos + 8;
        if chunk_start + len > data.len() {
            break;
        }
        if ctype == b"tEXt" {
            let chunk = &data[chunk_start..chunk_start + len];
            if let Some(sep) = chunk.iter().position(|&b| b == 0) {
                let key = String::from_utf8_lossy(&chunk[..sep]).to_string();
                let val = String::from_utf8_lossy(&chunk[sep + 1..]).to_string();
                chunks.push((key, val));
            }
        }
        if ctype == b"IEND" {
            break;
        }
        pos = chunk_start + len + 4; // + CRC
    }

    // 关键 chunk 判定 AI
    let ai_keys = ["Generation data", "Prompt", "parameters", "workflow", "Description", "Comment"];
    let is_ai = chunks.iter().any(|(k, _)| ai_keys.iter().any(|ak| k.eq_ignore_ascii_case(ak)));

    let raw = chunks
        .iter()
        .map(|(k, v)| format!("[{k}] {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    if raw.is_empty() {
        return None;
    }

    // 提取 prompt（Generation data 的 JSON prompt 字段 / parameters 的 prompt 段）
    let mut prompt: Option<String> = None;
    let mut negative: Option<String> = None;
    for (k, v) in &chunks {
        if k.eq_ignore_ascii_case("Generation data") || k.eq_ignore_ascii_case("parameters") {
            // 尝试 JSON 解析（Generation data 是 JSON）
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(v) {
                if let Some(p) = json.get("prompt").and_then(|x| x.as_str()) {
                    prompt = Some(p.to_string());
                }
                if let Some(n) = json.get("negativePrompt").and_then(|x| x.as_str()) {
                    negative = Some(n.to_string());
                }
            } else if let Some(sep) = v.find("Negative prompt:") {
                // webui parameters 格式：prompt\nNegative prompt: ...\nSteps: ...
                prompt = Some(v[..sep].trim().to_string());
                let rest = &v[sep + "Negative prompt:".len()..];
                if let Some(end) = rest.find("\nSteps:") {
                    negative = Some(rest[..end].trim().to_string());
                } else {
                    negative = Some(rest.trim().to_string());
                }
            } else {
                prompt = Some(v.trim().to_string());
            }
        } else if k.eq_ignore_ascii_case("Prompt") {
            // ComfyUI workflow 是 JSON，尝试提取 text 字段
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(v) {
                // 递归找第一个含 "text" 且值含逗号的节点（正向 prompt）
                let texts = collect_prompt_texts(&json);
                if let Some(first) = texts.first() {
                    prompt = Some(first.clone());
                }
            }
        }
    }

    // 从 prompt 提取 tag（逗号分隔，忽略 artist: 与质量黑名单）
    let tags = prompt
        .as_deref()
        .map(extract_prompt_tags)
        .unwrap_or_default();

    Some(AiMetadata {
        is_ai,
        raw,
        prompt,
        negative_prompt: negative,
        tags,
    })
}

/// 从 prompt 文本提取有效 tag。
/// 逗号分隔；忽略 `artist:xxx` 前缀；忽略质量黑名单；去空白。
pub fn extract_prompt_tags(prompt: &str) -> Vec<String> {
    prompt
        .split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter(|t| !t.to_lowercase().starts_with("artist:"))
        .filter(|t| !is_quality_tag(t))
        .map(|t| t.to_string())
        .collect()
}

/// 递归收集 ComfyUI workflow JSON 中所有含 `text` 的字符串（按出现顺序）。
fn collect_prompt_texts(v: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if k == "text" {
                    if let Some(s) = val.as_str() {
                        out.push(s.to_string());
                    }
                } else {
                    out.extend(collect_prompt_texts(val));
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                out.extend(collect_prompt_texts(item));
            }
        }
        _ => {}
    }
    out
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
