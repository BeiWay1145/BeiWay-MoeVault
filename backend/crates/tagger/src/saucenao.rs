//! SauceNAO 溯源客户端。
//!
//! API（https://saucenao.com/user.php?page=search-api）：
//! POST https://saucenao.com/search.php，multipart form-data：
//!   api_key, output_type=2 (JSON), db=999 (全库), minsim, numres, file
//!
//! 响应 JSON：
//!   header.status (0=成功, -1=失败, 3=限流), header.short_remaining (30s 内剩余),
//!   header.long_remaining (当日剩余), results[] { header.similarity, header.index_id,
//!   data.ext_urls[], data.title, data.author }
//!
//! 说明：本客户端不内置限流（限流由 ApiKeyPool 调度器统一管理），
//! 每次请求接收一个 API key，并返回配额头供调度器更新。

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::TaggerError;

/// SauceNAO API 端点。
pub const SAUCENAO_ENDPOINT: &str = "https://saucenao.com/search.php";

/// 单文件上传上限：官方 20MB，留 1MB 余量避免边界拒收。
pub const MAX_UPLOAD_BYTES: usize = 19 * 1024 * 1024;

/// 未返回 retry_in 时的限流等待秒数（免费账号 30 秒窗口）。
pub const DEFAULT_RATE_RETRY_SECS: u64 = 30;

/// 单条溯源结果。
#[derive(Debug, Clone, Default)]
pub struct SauceNaoResult {
    pub similarity: f64,
    pub ext_urls: Vec<String>,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// 请求后返回的配额头（供 ApiKeyPool 更新）。
#[derive(Debug, Clone, Default)]
pub struct QuotaHeaders {
    pub short_remaining: Option<i64>,
    pub long_remaining: Option<i64>,
}

/// SauceNAO 客户端（无状态，key 由调用方传入）。
#[derive(Clone)]
pub struct SauceNaoClient {
    http: reqwest::Client,
    min_sim: f64,
}

impl SauceNaoClient {
    pub fn new(min_sim: f64) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("MoeVault/0.1 (image manager)")
                .build()
                .expect("构建 HTTP 客户端失败"),
            min_sim,
        }
    }

    /// 用本地文件溯源（指定 API key）。返回 (结果, 配额头)。
    /// 错误时也携带配额头（Err 元组第二项），供调度器在失败/限流时更新配额。
    /// 文件超过 MAX_UPLOAD_BYTES 时自动近无损压缩后再上传（SauceNAO 拒收超大文件）。
    pub async fn search_file(
        &self,
        path: &Path,
        api_key: &str,
    ) -> Result<(SauceNaoResult, QuotaHeaders), (TaggerError, QuotaHeaders)> {
        self.search_file_with(&self.http, path, api_key).await
    }

    ///
    /// 用**指定 HTTP 客户端**溯源（增强2：多出口 IP 轮换）。
    ///
    /// 出口池为每个 Clash 入站端口维护一个绑定该代理的 Client，
    /// 调度时选一个传进来 —— 于是不同请求从不同出口 IP 发出，
    /// 各自独立计算 SauceNAO 的 4 次/30 秒 限额。
    ///
    pub async fn search_file_with(
        &self,
        http: &reqwest::Client,
        path: &Path,
        api_key: &str,
    ) -> Result<(SauceNaoResult, QuotaHeaders), (TaggerError, QuotaHeaders)> {
        let raw = tokio::fs::read(path)
            .await
            .map_err(|e| (TaggerError::Io(e), QuotaHeaders::default()))?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image.jpg".into());
        // 大图处理：>20MB 先压缩到限制内再上传（保持分辨率优先，必要时降质/缩放）
        let (bytes, upload_name) = if raw.len() > MAX_UPLOAD_BYTES {
            let mb = raw.len() as f64 / 1024.0 / 1024.0;
            match compress_to_limit(&raw, MAX_UPLOAD_BYTES) {
                Ok(compressed) => {
                    let cmb = compressed.len() as f64 / 1024.0 / 1024.0;
                    tracing::info!(
                        file = %filename,
                        from_mb = format!("{mb:.1}"),
                        to_mb = format!("{cmb:.1}"),
                        "溯源：图片超过 API 大小上限，已近无损压缩后上传"
                    );
                    (compressed, jpeg_name(&filename))
                }
                Err(e) => {
                    return Err((
                        TaggerError::Invalid(format!(
                            "图片过大（{mb:.1}MB，API 上限 {}MB）且压缩失败：{e}",
                            MAX_UPLOAD_BYTES / 1024 / 1024
                        )),
                        QuotaHeaders::default(),
                    ));
                }
            }
        } else {
            (raw, filename)
        };

        let part = reqwest::multipart::Part::bytes(bytes).file_name(upload_name);
        let form = reqwest::multipart::Form::new()
            .text("api_key", api_key.to_string())
            .text("output_type", "2")
            .text("db", "999")
            .text("minsim", self.min_sim.to_string())
            .text("numres", "5")
            .part("file", part);

        let resp = match http.post(SAUCENAO_ENDPOINT).multipart(form).send().await {
            Ok(r) => r,
            Err(e) => return Err((TaggerError::Http(e), QuotaHeaders::default())),
        };
        // 先取响应头配额（json() 会消费 resp），再解析 body
        let header_short = resp
            .headers()
            .get("X-Short-Remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());
        let header_long = resp
            .headers()
            .get("X-Long-Remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());
        let body: Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => return Err((TaggerError::Http(e), QuotaHeaders::default())),
        };
        tracing::debug!(
            "SauceNAO 原始响应（前 300 字符）: {}",
            &body.to_string()[..body.to_string().len().min(300)]
        );

        // 配额：SauceNAO 实际放在 JSON body 的 header.short_remaining / header.long_remaining，
        // 响应头 X-Short-Remaining / X-Long-Remaining 作为回退。
        let body_short = body
            .pointer("/header/short_remaining")
            .and_then(|v| v.as_i64());
        let body_long = body
            .pointer("/header/long_remaining")
            .and_then(|v| v.as_i64());
        let quota = QuotaHeaders {
            short_remaining: body_short.or(header_short),
            long_remaining: body_long.or(header_long),
        };

        let code = body
            .pointer("/header/status")
            .and_then(|v| v.as_i64())
            .unwrap_or(-99);
        if code != 0 {
            let raw_msg = body
                .pointer("/header/message")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误");
            let msg = strip_html(raw_msg);
            // 限流判定：SauceNAO 实际用 -2 表示 "Search Rate Too High"
            // （免费账号共享 IP 池：4 次 / 30 秒），status=3 为官方文档限流码，消息文本兜底。
            let lower = raw_msg.to_lowercase();
            let is_rate_limited = code == 3
                || code == -2
                || lower.contains("rate limit")
                || lower.contains("too high");
            if is_rate_limited {
                let retry_in = body
                    .pointer("/header/retry_in")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let secs = if retry_in > 0 {
                    retry_in
                } else {
                    DEFAULT_RATE_RETRY_SECS as i64
                };
                tracing::warn!(
                    status = code,
                    retry_secs = secs,
                    "SauceNAO 触发限流（免费账号共享 IP 池），将等待后重试"
                );
                return Err((TaggerError::RateLimited(secs), quota));
            }
            return Err((
                TaggerError::Invalid(format!("SauceNAO 返回错误 {code}: {msg}")),
                quota,
            ));
        }

        // 取相似度最高的结果
        let results = body
            .pointer("/results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let best = results
            .iter()
            .filter_map(|r| {
                // similarity 可能是数字或字符串（SauceNAO 实际返回字符串 "94.55"）
                let similarity = r
                    .pointer("/header/similarity")
                    .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))?;
                let ext_urls: Vec<String> = r
                    .pointer("/data/ext_urls")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|u| u.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(SauceNaoResult {
                    similarity,
                    ext_urls,
                    title: r.pointer("/data/title").and_then(|v| v.as_str()).map(String::from),
                    author: r.pointer("/data/author").and_then(|v| v.as_str()).map(String::from),
                })
            })
            .max_by(|a, b| a.similarity.partial_cmp(&b.similarity).unwrap_or(std::cmp::Ordering::Equal));

        match best {
            Some(r) => {
                tracing::debug!(similarity = r.similarity, urls = r.ext_urls.len(), "SauceNAO 溯源成功");
                Ok((r, quota))
            }
            None => Err((TaggerError::NoSource("SauceNAO 无匹配结果".into()), quota)),
        }
    }
}

/// 响应 JSON 的结构化视图（调试/测试用）。
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SauceNaoResponse {
    pub header: ResponseHeader,
    pub results: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ResponseHeader {
    pub status: i64,
    pub message: Option<String>,
    pub short_remaining: Option<i64>,
    pub long_remaining: Option<i64>,
}

/// 去掉 SauceNAO 错误消息中的 HTML 标签与常见实体（响应里带 <strong>/<br />）。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 压缩后统一使用 .jpg 文件名（内容已编码为 JPEG）。
fn jpeg_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => format!("{stem}.jpg"),
        _ => "image.jpg".to_string(),
    }
}

/// 近无损压缩到大小上限：优先保持分辨率（JPEG 高质量，quality 92 起），
/// 仍超限则交替「降质量 → 降分辨率」，直到进入限制或尝试耗尽。
fn compress_to_limit(src: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(src).map_err(|e| format!("解码失败: {e}"))?;
    let mut quality: u8 = 92;
    let mut scale: f32 = 1.0;
    let mut last_len = 0usize;
    for _ in 0..10 {
        let resized;
        let frame: &image::DynamicImage = if scale < 1.0 {
            resized = img.resize(
                ((img.width() as f32 * scale) as u32).max(64),
                ((img.height() as f32 * scale) as u32).max(64),
                image::imageops::FilterType::Lanczos3,
            );
            &resized
        } else {
            &img
        };
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        enc.encode_image(frame)
            .map_err(|e| format!("JPEG 编码失败: {e}"))?;
        last_len = out.len();
        if out.len() <= limit {
            return Ok(out);
        }
        if quality > 70 {
            quality -= 8;
        } else {
            scale *= 0.75;
        }
    }
    Err(format!(
        "压缩后仍为 {:.1}MB（上限 {:.1}MB）",
        last_len as f64 / 1024.0 / 1024.0,
        limit as f64 / 1024.0 / 1024.0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_tags() {
        let raw = "<strong>Search Rate Too High.</strong><br /><br />BeiWay1, basic accounts share an IP";
        let clean = strip_html(raw);
        assert!(clean.starts_with("Search Rate Too High."));
        assert!(!clean.contains('<'));
        assert!(clean.contains("basic accounts share an IP"));
    }

    #[test]
    fn jpeg_name_swaps_extension() {
        assert_eq!(jpeg_name("a.png"), "a.jpg");
        assert_eq!(jpeg_name("a.b.webp"), "a.b.jpg");
        assert_eq!(jpeg_name("noext"), "image.jpg");
    }

    /// 大图压缩：构造一张细节丰富的 PNG，压缩后应显著变小并进入限制。
    #[test]
    fn compress_large_image_into_limit() {
        let mut img = image::RgbImage::new(3000, 3000);
        for (x, y, px) in img.enumerate_pixels_mut() {
            // 伪随机噪点：让 PNG 体积足够大，便于验证压缩效果
            let v = ((x * 7919 + y * 104729) % 256) as u8;
            *px = image::Rgb([v, v.wrapping_mul(3), v.wrapping_add(17)]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("编码 PNG 失败");

        // 限制设为 PNG 体积的 1/4，压缩后应满足
        let limit = (png.len() / 4).max(64 * 1024);
        let out = compress_to_limit(&png, limit).expect("应能压缩到限制内");
        assert!(out.len() <= limit, "压缩结果 {} 应 <= {limit}", out.len());
        // 输出应为可解码的 JPEG
        let decoded = image::load_from_memory(&out).expect("输出应可解码");
        assert!(decoded.width() > 0 && decoded.height() > 0);
    }
}

