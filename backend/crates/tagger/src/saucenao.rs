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
    pub async fn search_file(
        &self,
        path: &Path,
        api_key: &str,
    ) -> Result<(SauceNaoResult, QuotaHeaders), (TaggerError, QuotaHeaders)> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| (TaggerError::Io(e), QuotaHeaders::default()))?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image.jpg".into());

        let part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
        let form = reqwest::multipart::Form::new()
            .text("api_key", api_key.to_string())
            .text("output_type", "2")
            .text("db", "999")
            .text("minsim", self.min_sim.to_string())
            .text("numres", "5")
            .part("file", part);

        let resp = match self.http.post(SAUCENAO_ENDPOINT).multipart(form).send().await {
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
            let msg = body
                .pointer("/header/message")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误")
                .to_string();
            return Err(if code == 3 {
                (
                    TaggerError::RateLimited(
                        body.pointer("/header/retry_in").and_then(|v| v.as_i64()).unwrap_or(30),
                    ),
                    quota,
                )
            } else {
                (
                    TaggerError::Invalid(format!("SauceNAO 返回错误 {code}: {msg}")),
                    quota,
                )
            });
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
