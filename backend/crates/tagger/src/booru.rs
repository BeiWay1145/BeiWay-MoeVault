//! danbooru / gelbooru 标签爬取。
//!
//! 从 SauceNAO 返回的 ext_urls 中提取 danbooru/gelbooru 帖子链接，
//! 调用官方 API 获取标签：
//! - danbooru: GET https://danbooru.donmai.us/posts/{id}.json → tag_string（空格分隔）
//! - gelbooru: GET https://gelbooru.com/index.php?page=dapi&s=post&q=index&id={id}&json=1 → post.tags

use std::sync::OnceLock;

use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::TaggerError;

static DANBOORU_RE: OnceLock<Regex> = OnceLock::new();
static GELBOORU_RE: OnceLock<Regex> = OnceLock::new();

fn danbooru_re() -> &'static Regex {
    DANBOORU_RE.get_or_init(|| {
        // 兼容两种格式：danbooru.donmai.us/posts/123 和 danbooru.donmai.us/post/show/123
        Regex::new(r"danbooru\.donmai\.us/posts?/(?:show/)?(\d+)").expect("danbooru 正则编译失败")
    })
}

fn gelbooru_re() -> &'static Regex {
    GELBOORU_RE.get_or_init(|| {
        // 兼容各种格式：index.php?...id=123、post/view/123、posts/123 等
        Regex::new(r"gelbooru\.com/(?:index\.php\?[^#]*?id=|post/view/|posts/|post/show/)?(\d+)")
            .expect("gelbooru 正则编译失败")
    })
}

/// 从 ext_urls 中识别 booru 来源：返回 `(source, post_id)`。
/// source: "danbooru" | "gelbooru" | None。
pub fn extract_booru(urls: &[String]) -> Option<(&'static str, u64)> {
    for url in urls {
        if let Some(caps) = danbooru_re().captures(url) {
            if let Ok(id) = caps[1].parse() {
                return Some(("danbooru", id));
            }
        }
    }
    for url in urls {
        if url.contains("gelbooru.com") {
            if let Some(caps) = gelbooru_re().captures(url) {
                if let Ok(id) = caps[1].parse() {
                    return Some(("gelbooru", id));
                }
            }
        }
    }
    None
}

/// 标签 + 分类（danbooru 分类：artist/copyright/character/general）。
#[derive(Debug, Clone)]
pub struct BooruTag {
    pub name: String,
    pub category: String,
}

/// 从 SauceNAO 结果中爬取标签。
/// 返回 (source, source_url, tags)。
pub async fn fetch_tags(
    http: &Client,
    urls: &[String],
) -> Result<(&'static str, String, Vec<BooruTag>), TaggerError> {
    let Some((source, post_id)) = extract_booru(urls) else {
        return Err(TaggerError::NoSource("ext_urls 中无 danbooru/gelbooru 链接".into()));
    };
    match source {
        "danbooru" => {
            let url = format!("https://danbooru.donmai.us/posts/{post_id}.json");
            let tags = fetch_danbooru(http, &url).await?;
            Ok(("danbooru", url, tags))
        }
        "gelbooru" => {
            let url = format!(
                "https://gelbooru.com/index.php?page=dapi&s=post&q=index&id={post_id}&json=1"
            );
            let tags = fetch_gelbooru(http, &url).await?;
            Ok(("gelbooru", url, tags))
        }
        _ => unreachable!(),
    }
}

#[derive(Debug, Deserialize)]
struct DanbooruPost {
    tag_string: String,
    #[serde(default)]
    tag_string_artist: String,
    #[serde(default)]
    tag_string_copyright: String,
    #[serde(default)]
    tag_string_character: String,
    #[serde(default)]
    tag_string_general: String,
}

async fn fetch_danbooru(http: &Client, url: &str) -> Result<Vec<BooruTag>, TaggerError> {
    let resp = http.get(url).header("User-Agent", "MoeVault/0.1").send().await?;
    if !resp.status().is_success() {
        return Err(TaggerError::Invalid(format!(
            "danbooru API 返回 {}",
            resp.status()
        )));
    }
    let post: DanbooruPost = resp.json().await?;
    // 分类字段优先；缺失时从 tag_string 按前缀推断
    let mut out = Vec::new();
    let push = |s: &str, cat: &str, out: &mut Vec<BooruTag>| {
        for t in s.split_whitespace() {
            let (name, cat) = if let Some(stripped) = t.strip_prefix("artist:") {
                (stripped, "artist")
            } else if let Some(stripped) = t.strip_prefix("copyright:") {
                (stripped, "copyright")
            } else if let Some(stripped) = t.strip_prefix("character:") {
                (stripped, "character")
            } else {
                (t, cat)
            };
            if !name.is_empty() {
                out.push(BooruTag {
                    name: name.to_string(),
                    category: cat.to_string(),
                });
            }
        }
    };
    push(&post.tag_string_artist, "artist", &mut out);
    push(&post.tag_string_copyright, "copyright", &mut out);
    push(&post.tag_string_character, "character", &mut out);
    push(&post.tag_string_general, "general", &mut out);
    if out.is_empty() {
        // 兜底：从 tag_string 全量解析（含前缀推断）
        push(&post.tag_string, "general", &mut out);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct GelbooruResponse {
    post: Vec<GelbooruPost>,
}

#[derive(Debug, Deserialize)]
struct GelbooruPost {
    #[serde(default)]
    tags: Option<String>,
}

async fn fetch_gelbooru(http: &Client, url: &str) -> Result<Vec<BooruTag>, TaggerError> {
    let resp = http.get(url).header("User-Agent", "MoeVault/0.1").send().await?;
    if !resp.status().is_success() {
        return Err(TaggerError::Invalid(format!(
            "gelbooru API 返回 {}",
            resp.status()
        )));
    }
    let parsed: GelbooruResponse = resp.json().await?;
    let tags = parsed
        .post
        .first()
        .and_then(|p| p.tags.clone())
        .unwrap_or_default();
    Ok(tags
        .split_whitespace()
        .map(|s| BooruTag {
            name: s.to_string(),
            category: "general".to_string(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_danbooru_id() {
        let urls = vec![
            "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=123".to_string(),
            "https://danbooru.donmai.us/posts/4567890".to_string(),
        ];
        let (source, id) = extract_booru(&urls).unwrap();
        assert_eq!(source, "danbooru");
        assert_eq!(id, 4567890);
    }

    #[test]
    fn extracts_danbooru_old_url_format() {
        // SauceNAO 返回的旧格式：post/show/8258012
        let urls = vec!["https://danbooru.donmai.us/post/show/8258012".to_string()];
        let (source, id) = extract_booru(&urls).unwrap();
        assert_eq!(source, "danbooru");
        assert_eq!(id, 8258012);
    }

    #[test]
    fn extracts_gelbooru_id() {
        let urls = vec!["https://gelbooru.com/index.php?page=post&s=view&id=987654".to_string()];
        let (source, id) = extract_booru(&urls).unwrap();
        assert_eq!(source, "gelbooru");
        assert_eq!(id, 987654);
    }

    #[test]
    fn no_booru_returns_none() {
        let urls = vec!["https://pixiv.net/1.png".to_string()];
        assert!(extract_booru(&urls).is_none());
    }
}
