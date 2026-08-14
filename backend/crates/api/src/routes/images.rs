//! 图库接口：GET /api/v1/images、GET /api/v1/stats。
//!
//! 骨架版实现基础游标分页 + status 过滤；完整筛选（标签/日期/质量/来源）
//! 按 docs/TECH_DETAILS.md 第 2.1 节在 M2+ 补全。

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use moevault_core::models::{ImageFilter, Page, SortKey, Stats, STATUS_ACTIVE};
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response, join_error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/images", get(list_images))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/images/{id}/recycle", post(recycle_image))
        .route("/api/v1/images/{id}/sidecar", post(generate_sidecar))
        .route("/api/v1/images/{id}/file", get(get_original_file))
        .route("/api/v1/images/{id}/similar", get(similar_images))
        .route("/api/v1/images/{id}/ai-info", post(read_ai_info))
        .route("/api/v1/images/{id}/mark-ai", post(mark_ai))
        .route("/api/v1/images/{id}/source-url", put(update_source_url))
        .route("/api/v1/images/{id}/rename", put(rename_image))
        .route("/api/v1/images/{id}/source-info", get(source_info))
        .route("/api/v1/images/{id}/replace-from-url", post(replace_from_url))
        .route("/api/v1/images/{id}/tags/{tag_id}", delete(remove_image_tag))
        .route("/api/v1/images/{id}/tags", post(add_image_tag))
        .route("/api/v1/images/reprocess", post(reprocess_images))
}

/// GET /api/v1/images/{id}/file：返回原图（stream）。
async fn get_original_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::response::Response, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let library_dir = state.library_dir();
    let img = tokio::task::spawn_blocking(move || db.get_image_by_id(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    let Some(img) = img else {
        return Err(error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")));
    };
    let path = library_dir.join(&img.rel_path);
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let mime = mime_for_ext(&img.format);
            let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
            Ok(axum::response::Response::builder()
                .header(axum::http::header::CONTENT_TYPE, mime)
                .body(body)
                .unwrap())
        }
        Err(_) => Err(error_response(ErrorKind::NotFound, "原图文件不存在".to_string())),
    }
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// GET /api/v1/images/{id}/similar：pHash 邻近图（汉明距离升序）。
async fn similar_images(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let limit = params.limit.unwrap_or(12).min(50);
    let db = state.db.clone();
    let items = tokio::task::spawn_blocking(move || {
        let target = db.get_image_by_id(id).map_err(db_error_response)?;
        let Some(target) = target else {
            return Err(error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")));
        };
        let all = db.all_active_images().map_err(db_error_response)?;
        // 汉明距离排序（排除自身）
        let mut sims: Vec<(i64, u32)> = all
            .iter()
            .filter(|(iid, _, _)| *iid != id)
            .map(|(iid, phash, _)| (*iid, (target.phash as u64 ^ *phash as u64).count_ones()))
            .collect();
        sims.sort_by_key(|(_, d)| *d);
        sims.truncate(limit as usize);
        let ids: Vec<i64> = sims.iter().map(|(iid, _)| *iid).collect();
        let mut out = Vec::new();
        for iid in ids {
            if let Ok(Some(img)) = db.get_image_by_id(iid) {
                out.push(json!({
                    "id": img.id,
                    "thumb_rel": img.thumb_rel,
                    "rel_path": img.rel_path,
                    "width": img.width,
                    "height": img.height,
                }));
            }
        }
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(out)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "items": items })))
}

/// POST /api/v1/images/{id}/ai-info：手动读取 AI 生成图片元信息（PNG tEXt）。
async fn read_ai_info(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let library_dir = state.library_dir();
    let result = tokio::task::spawn_blocking(move || {
        let img = db
            .get_image_by_id(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
        let path = library_dir.join(&img.rel_path);
        let meta = moevault_ingest::features::read_ai_metadata(&path);
        match &meta {
            Some(m) => {
                // 存原始元信息 + 提取的 prompt tag（source=ai）
                db.set_ai_metadata(id, &m.raw).map_err(db_error_response)?;
                if !m.tags.is_empty() {
                    let tag_ids: Vec<(i64, Option<f64>)> = m
                        .tags
                        .iter()
                        .map(|t| db.upsert_tag(t, "general").map(|tid| (tid, None)))
                        .collect::<Result<_, _>>()
                        .map_err(db_error_response)?;
                    db.insert_image_tags(id, &tag_ids, "ai").map_err(db_error_response)?;
                }
                // 清理历史录入的负面标签（名字出现在负面提示词里的 ai 标签）
                if let Some(neg) = &m.negative_prompt {
                    let neg_tags: Vec<String> = neg
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                    if !neg_tags.is_empty() {
                        let _ = db.remove_ai_negative_tags(id, &neg_tags);
                    }
                }
                Ok::<_, (axum::http::StatusCode, Json<Value>)>(json!({
                    "ok": true, "is_ai": m.is_ai, "metadata": m.raw,
                    "prompt": m.prompt, "negative_prompt": m.negative_prompt,
                    "tags": m.tags,
                }))
            }
            None => Ok(json!({ "ok": true, "is_ai": false, "metadata": null, "tags": [] })),
        }
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(result))
}

/// POST /api/v1/images/{id}/mark-ai：手动标记/取消标记图片为 AI 生成。
/// body 可选 `{ "ai": bool }`（默认 true=标记，false=取消）。
async fn mark_ai(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let set = body
        .as_ref()
        .and_then(|j| j.get("ai"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if set {
            db.set_ai_metadata(id, "[manual] marked as AI")
        } else {
            db.clear_ai_mark(id)
        }
        .map_err(db_error_response)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "is_ai": set })))
}

/// PUT /api/v1/images/{id}/source-url：手动编辑溯源来源链接。
/// body `{ "url": string | null }`（null 清除）。
async fn update_source_url(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .map(|s| strip_json_suffix(&s)) // 存储时去掉 .json（API 链接→页面链接）
        .filter(|s| !s.is_empty());
    let db = state.db.clone();
    let url_for_db = url.clone();
    tokio::task::spawn_blocking(move || {
        db.update_source_url(id, url_for_db.as_deref())
            .map_err(db_error_response)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "source_url": url })))
}

/// POST /api/v1/images/{id}/sidecar：生成 sidecar .txt（逗号分隔标签，与 cl_tagger 格式一致）。
async fn generate_sidecar(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let library_dir = state.library_dir();

    let result = tokio::task::spawn_blocking(move || {
        // 1. 取图片 rel_path
        let img = db
            .get_image_by_id(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
        // 2. 取标签（所有来源，按名字逗号拼接）
        let tags = db.image_tags(id).map_err(db_error_response)?;
        let tag_str: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
        let content = tag_str.join(", ");
        // 3. 写同名 .txt（图片旁）
        let src_path = library_dir.join(&img.rel_path);
        let txt_path = src_path.with_extension("txt");
        std::fs::write(&txt_path, content)
            .map_err(|e| error_response(ErrorKind::Internal, format!("写入失败: {e}")))?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(txt_path.display().to_string())
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;

    Ok(Json(json!({ "ok": true, "path": result })))
}

/// PUT /api/v1/images/{id}/rename：重命名库内图片文件。
/// body `{ "name": "新名字.jpg" }`；保持哈希目录前缀不变，冲突即失败。
async fn rename_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let name = req
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error_response(ErrorKind::InvalidInput, "name 不能为空"))?;
    // 校验：不能包含路径分隔符 / 反斜杠 / 冒号等
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(error_response(ErrorKind::InvalidInput, "文件名包含非法字符"));
    }
    if name.len() > 200 {
        return Err(error_response(ErrorKind::InvalidInput, "文件名过长"));
    }
    let db = state.db.clone();
    let library_dir = state.library_dir();
    let result = tokio::task::spawn_blocking(move || {
        let img = db
            .get_image_by_id(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
        // 取原 rel_path 的目录前缀（如 61/），新 rel_path = 前缀 + 新名字
        let prefix = img
            .rel_path
            .rsplit_once(['/', '\\'])
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        let new_rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let src = library_dir.join(&img.rel_path);
        let dst = library_dir.join(&new_rel);
        if dst.exists() {
            return Err(error_response(
                ErrorKind::InvalidInput,
                format!("文件已存在: {name}"),
            ));
        }
        std::fs::rename(&src, &dst).map_err(|e| {
            error_response(ErrorKind::Internal, format!("文件改名失败: {e}"))
        })?;
        db.rename_image_file(id, &new_rel).map_err(db_error_response)?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(new_rel)
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(json!({ "ok": true, "rel_path": result })))
}

/// 把 danbooru/gelbooru 的 .json API 链接转成页面链接（去掉 .json 后缀）。
fn strip_json_suffix(url: &str) -> String {
    let trimmed = url.trim_end();
    if let Some(stripped) = trimmed.strip_suffix(".json") {
        stripped.to_string()
    } else {
        trimmed.to_string()
    }
}

/// GET /api/v1/images/{id}/source-info：解析原图链接页面/API 获取网络图信息
/// （分辨率 + 文件大小 + 网络图直链）。支持 danbooru / gelbooru。
async fn source_info(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let img = tokio::task::spawn_blocking(move || db.get_image_by_id(id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?
        .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
    let Some(url) = img.source_url.as_deref().filter(|u| !u.is_empty()) else {
        return Err(error_response(ErrorKind::NotFound, "该图片没有原图链接"));
    };
    let page_url = strip_json_suffix(url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("MoeVault/0.1")
        .build()
        .map_err(|e| error_response(ErrorKind::Internal, format!("HTTP 客户端构建失败: {e}")))?;
    // 尝试解析帖子 id 并请求官方 API
    let info = parse_remote_source_info(&client, &page_url).await;
    Ok(Json(json!({
        "ok": true,
        "page_url": page_url,
        "info": info,
    })))
}

/// 解析网络来源信息：danbooru /posts/{id}.json、gelbooru dapi。
/// danbooru 返回单个 JSON 对象；gelbooru dapi 返回 `{"post": [...]}`。
async fn parse_remote_source_info(
    client: &reqwest::Client,
    page_url: &str,
) -> Value {
    let lower = page_url.to_lowercase();
    // danbooru.donmai.us/posts/6019533
    if lower.contains("danbooru.donmai.us") {
        if let Some(pid) = extract_post_id(page_url) {
            let api = format!("https://danbooru.donmai.us/posts/{pid}.json");
            if let Ok(resp) = client.get(&api).send().await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    // 兼容：单个对象 或 数组（取第一个）
                    let post = body.as_object().map(|_| &body)
                        .or_else(|| body.as_array().and_then(|a| a.first()));
                    if let Some(post) = post {
                        let fw = post.get("image_width").and_then(|v| v.as_i64());
                        let fh = post.get("image_height").and_then(|v| v.as_i64());
                        let fs = post.get("file_size").and_then(|v| v.as_i64());
                        // 原图直链优先，缺失回退大图/缩略图
                        let file_url = post
                            .get("file_url")
                            .or_else(|| post.get("large_file_url"))
                            .or_else(|| post.get("sample_url"))
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        if fw.is_some() || fs.is_some() {
                            return json!({
                                "width": fw, "height": fh, "size_bytes": fs, "file_url": file_url,
                            });
                        }
                    }
                }
            }
        }
    }
    // gelbooru.com/index.php?page=dapi&s=post&q=index ... id=xxx
    if lower.contains("gelbooru.com") {
        if let Some(pid) = extract_post_id(page_url) {
            let api = format!("https://gelbooru.com/index.php?page=dapi&s=post&q=index&json=1&id={pid}");
            if let Ok(resp) = client.get(&api).send().await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(post) = body
                        .pointer("/post")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                    {
                        let fw = post.get("image_width").and_then(|v| v.as_i64());
                        let fh = post.get("image_height").and_then(|v| v.as_i64());
                        let fs = post.get("image_size").and_then(|v| v.as_i64());
                        let file_url = post
                            .get("file_url")
                            .or_else(|| post.get("sample_url"))
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        if fw.is_some() || fs.is_some() {
                            return json!({
                                "width": fw, "height": fh, "size_bytes": fs, "file_url": file_url,
                            });
                        }
                    }
                }
            }
        }
    }
    json!({ "width": null, "height": null, "size_bytes": null, "file_url": null })
}

/// 从 URL 中提取帖子 id（/posts/6019533 或 ?id=6019533）。
fn extract_post_id(url: &str) -> Option<String> {
    // /posts/6019533
    if let Some(idx) = url.find("/posts/") {
        let rest = &url[idx + "/posts/".len()..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    // ?id=6019533
    if let Some(idx) = url.find("id=") {
        let rest = &url[idx + 3..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    None
}

/// POST /api/v1/images/{id}/replace-from-url：下载网络原图替换库内文件。
/// body `{ "url": "网络图直链" }`；重新哈希入库（保持 id），更新尺寸/格式/缩略图，
/// 旧文件删除；标签/评分/来源链接保留。
async fn replace_from_url(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error_response(ErrorKind::InvalidInput, "url 不能为空"))?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(error_response(ErrorKind::InvalidInput, "url 必须是 http(s) 链接"));
    }
    let db = state.db.clone();
    let library_dir = state.library_dir();
    let thumbs_dir = state.thumbs_dir();
    let result = tokio::task::spawn_blocking(move || {
        // 1. 取原图记录
        let img = db
            .get_image_by_id(id)
            .map_err(db_error_response)?
            .ok_or_else(|| error_response(ErrorKind::NotFound, format!("图片 {id} 不存在")))?;
        let old_path = library_dir.join(&img.rel_path);
        // 2. 同步下载（reqwest block）
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .user_agent("MoeVault/0.1")
            .build()
            .map_err(|e| error_response(ErrorKind::Internal, format!("HTTP 客户端构建失败: {e}")))?;
        let bytes = client
            .get(&url)
            .send()
            .map_err(|e| error_response(ErrorKind::Internal, format!("下载失败: {e}")))?
            .bytes()
            .map_err(|e| error_response(ErrorKind::Internal, format!("读取下载内容失败: {e}")))?;
        if bytes.is_empty() {
            return Err(error_response(ErrorKind::InvalidInput, "下载内容为空"));
        }
        // 3. 解码验证 + 嗅探格式
        let format_guess = image::guess_format(&bytes).map_err(|e| {
            error_response(ErrorKind::InvalidInput, format!("下载内容不是有效图片: {e}"))
        })?;
        let ext = match format_guess {
            image::ImageFormat::Png => "png",
            image::ImageFormat::Jpeg => "jpg",
            image::ImageFormat::WebP => "webp",
            image::ImageFormat::Gif => "gif",
            image::ImageFormat::Bmp => "bmp",
            _ => return Err(error_response(ErrorKind::InvalidInput, "不支持的图片格式")),
        };
        let decoded = image::load_from_memory(&bytes)
            .map_err(|e| error_response(ErrorKind::InvalidInput, format!("图片解码失败: {e}")))?;
        let (w, h) = (decoded.width(), decoded.height());
        // 4. 重新哈希 → 新分片路径
        use md5::Digest;
        let digest = md5::Md5::digest(&bytes);
        let md5_hex = format!("{digest:x}");
        let prefix = &md5_hex[..md5_hex.len().min(2)];
        let new_rel = format!("{prefix}/{md5_hex}.{ext}");
        let new_path = library_dir.join(&new_rel);
        // 日志：目标目录可能不存在（新 md5 前缀是全新目录）→ 先建目录再写
        tracing::info!(id, url = %url, bytes = bytes.len(), ext, new_rel = %new_rel,
            "替换网络图：下载完成，准备写入");
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                error_response(ErrorKind::Internal, format!("创建目录失败 {}: {e}", parent.display()))
            })?;
        }
        // 5. 写新文件（若与旧文件同路径则直接覆盖）
        std::fs::write(&new_path, &bytes)
            .map_err(|e| error_response(ErrorKind::Internal, format!("写入新文件失败: {e}")))?;
        tracing::info!(id, new_path = %new_path.display(), "替换网络图：写入成功");
        // 6. 删旧文件（若非同一路径）
        if new_path != old_path {
            match std::fs::remove_file(&old_path) {
                Ok(_) => tracing::info!(id, old = %old_path.display(), "替换网络图：旧文件已删除"),
                Err(e) => tracing::warn!(id, old = %old_path.display(), error = %e, "替换网络图：旧文件删除失败（忽略）"),
            }
        }
        // 7. 重新生成缩略图
        let thumb_rel = format!("{prefix}/{md5_hex}.webp");
        let thumb_path = thumbs_dir.join(&thumb_rel);
        moevault_ingest::importer::generate_thumbnail(&new_path, &thumb_path);
        tracing::info!(id, thumb = %thumb_rel, "替换网络图：缩略图已生成");
        // 8. 更新库记录
        db.replace_image_file(id, &md5_hex, &new_rel, w, h, ext, bytes.len() as i64)
            .map_err(db_error_response)?;
        Ok::<_, (axum::http::StatusCode, Json<Value>)>(json!({
            "ok": true,
            "md5": md5_hex,
            "rel_path": new_rel,
            "width": w,
            "height": h,
            "format": ext,
            "size_bytes": bytes.len(),
            "thumb_rel": thumb_rel,
        }))
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;
    Ok(Json(result))
}

/// DELETE /api/v1/images/{id}/tags/{tag_id}：从本图移除标签（仅本图）。
async fn remove_image_tag(
    State(state): State<AppState>,
    Path((id, tag_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.remove_image_tag(id, tag_id))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true })))
}

/// POST /api/v1/images/{id}/tags：给本图添加标签（source=manual）。
/// body `{ "name": "1girl", "category": "general" }`（category 可选，默认 general）。
async fn add_image_tag(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let name = req
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error_response(ErrorKind::InvalidInput, "name 不能为空"))?;
    if name.len() > 100 {
        return Err(error_response(ErrorKind::InvalidInput, "标签名过长"));
    }
    let category = req
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "general".to_string());
    let db = state.db.clone();
    let tag_id = tokio::task::spawn_blocking(move || db.add_image_tag(id, &name, &category))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;
    Ok(Json(json!({ "ok": true, "tag_id": tag_id })))
}

#[derive(Debug, Deserialize)]
pub struct RecycleRequest {
    /// duplicate / manual / auto。
    pub reason: Option<String>,
}

/// POST /api/v1/images/{id}/recycle：把单张图片移入回收站。
async fn recycle_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<RecycleRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let reason = req.reason.unwrap_or_else(|| "manual".to_string());
    let db = state.db.clone();
    let library = state.library_dir();
    let recycle = state.recycle_dir();
    tokio::task::spawn_blocking(move || {
        moevault_dedup::recycle_image(&db, id, &reason, &library, &recycle)
    })
    .await
    .map_err(join_error_response)?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "recycled": id })))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListParams {
    /// active / recycled，默认 active。
    pub status: Option<String>,
    /// 每页条数，默认 100，最大 500。
    pub limit: Option<i64>,
    /// 游标：上一页返回的 next_cursor（数字 id 字符串）。
    pub cursor: Option<String>,
    /// 关键字（文件名 LIKE）。
    pub q: Option<String>,
    /// 包含标签（逗号分隔，AND 语义）。
    pub tags: Option<String>,
    /// 排除标签（逗号分隔）。
    pub exclude_tags: Option<String>,
    /// 日期范围（epoch 秒）。
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    /// 美学分范围（1-5）。
    pub aesthetic_min: Option<f64>,
    pub aesthetic_max: Option<f64>,
    /// 美学筛选时包含未评分图片（1/0/true/false）。
    pub aesthetic_include_unscored: Option<String>,
    /// 清晰度范围。
    pub clarity_min: Option<f64>,
    pub clarity_max: Option<f64>,
    /// 来源（danbooru/gelbooru/local）。
    pub source: Option<String>,
    /// 格式（jpg/png/webp）。
    pub format: Option<String>,
    /// 最小宽/高。
    pub min_width: Option<i64>,
    pub min_height: Option<i64>,
    /// 只看冗余候选（1/0/true/false）。
    pub is_redundant: Option<String>,
    /// 只看 AI 生成图片（1/0/true/false）。
    pub is_ai: Option<String>,
    /// 溯源状态：sauced / unsauced / un-sauced。
    pub sauce_status: Option<String>,
    /// 排序键：imported/date/aesthetic/clarity/size/random。
    pub sort: Option<String>,
    /// asc/desc。
    pub order: Option<String>,
}

impl ListParams {
    fn parse_sort(&self) -> Result<SortKey, (axum::http::StatusCode, Json<Value>)> {
        let s = self.sort.as_deref().unwrap_or("imported");
        let key = match s {
            "imported" => SortKey::Imported,
            "date" => SortKey::Date,
            "aesthetic" => SortKey::Aesthetic,
            "clarity" => SortKey::Clarity,
            "size" => SortKey::Size,
            "random" => SortKey::Random,
            _ => {
                return Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("sort 仅支持 imported/date/aesthetic/clarity/size/random，收到: {s}"),
                ))
            }
        };
        Ok(key)
    }

    fn parse_redundant(&self) -> Result<Option<bool>, (axum::http::StatusCode, Json<Value>)> {
        match &self.is_redundant {
            None => Ok(None),
            Some(v) if v.trim().is_empty() => Ok(None),
            Some(v) => match v.to_lowercase().as_str() {
                "1" | "true" | "yes" => Ok(Some(true)),
                "0" | "false" | "no" => Ok(Some(false)),
                _ => Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("is_redundant 仅支持 1/0/true/false，收到: {v}"),
                )),
            },
        }
    }

    fn split_tags(s: &Option<String>) -> Vec<String> {
        s.as_deref()
            .map(|v| {
                v.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn build_filter(&self) -> Result<ImageFilter, (axum::http::StatusCode, Json<Value>)> {
        Ok(ImageFilter {
            q: self.q.clone(),
            tags: Self::split_tags(&self.tags),
            exclude_tags: Self::split_tags(&self.exclude_tags),
            date_from: self.date_from,
            date_to: self.date_to,
            aesthetic_min: self.aesthetic_min,
            aesthetic_max: self.aesthetic_max,
            aesthetic_include_unscored: self.parse_include_unscored()?,
            clarity_min: self.clarity_min,
            clarity_max: self.clarity_max,
            source: self.source.clone(),
            format: self.format.clone(),
            min_width: self.min_width,
            min_height: self.min_height,
            is_redundant: self.parse_redundant()?,
            is_ai: self.parse_ai()?,
            sauce_status: self.sauce_status.clone(),
        })
    }

    fn parse_ai(&self) -> Result<Option<bool>, (axum::http::StatusCode, Json<Value>)> {
        match &self.is_ai {
            None => Ok(None),
            Some(v) if v.trim().is_empty() => Ok(None),
            Some(v) => match v.to_lowercase().as_str() {
                "1" | "true" | "yes" => Ok(Some(true)),
                "0" | "false" | "no" => Ok(Some(false)),
                _ => Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("is_ai 仅支持 1/0/true/false，收到: {v}"),
                )),
            },
        }
    }

    fn parse_include_unscored(&self) -> Result<Option<bool>, (axum::http::StatusCode, Json<Value>)> {
        match &self.aesthetic_include_unscored {
            None => Ok(None),
            Some(v) if v.trim().is_empty() => Ok(None),
            Some(v) => match v.to_lowercase().as_str() {
                "1" | "true" | "yes" => Ok(Some(true)),
                "0" | "false" | "no" => Ok(Some(false)),
                _ => Err(error_response(
                    ErrorKind::InvalidInput,
                    format!("aesthetic_include_unscored 仅支持 1/0/true/false，收到: {v}"),
                )),
            },
        }
    }
}

async fn list_images(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let status = params.status.clone().unwrap_or_else(|| STATUS_ACTIVE.to_string());
    if status != "active" && status != "recycled" {
        return Err(error_response(
            moevault_core::ErrorKind::InvalidInput,
            format!("status 仅支持 active/recycled，收到: {status}"),
        ));
    }
    let limit = params.limit.unwrap_or(100);
    let cursor_id = match &params.cursor {
        Some(c) => match c.parse::<i64>() {
            Ok(v) => Some(v),
            Err(_) => {
                return Err(error_response(
                    moevault_core::ErrorKind::InvalidInput,
                    format!("cursor 格式错误: {c}"),
                ))
            }
        },
        None => None,
    };

    let db = state.db.clone();
    let status_for_query = status.clone();
    let filter = params.build_filter()?;
    let filter_for_total = filter.clone();
    let sort = params.parse_sort()?;
    let sort_asc = params.order.as_deref() == Some("asc");
    let (items, next_id) = tokio::task::spawn_blocking(move || {
        db.list_images_filtered(&status_for_query, &filter, sort, sort_asc, limit, cursor_id)
    })
    .await
    .map_err(join_error_response)?
    .map_err(db_error_response)?;

    let total = {
        let db = state.db.clone();
        let status2 = status.clone();
        tokio::task::spawn_blocking(move || db.count_images_filtered(&status2, &filter_for_total))
            .await
            .map_err(join_error_response)?
            .map_err(db_error_response)?
    };

    let page = Page {
        items,
        next_cursor: next_id.map(|id| id.to_string()),
        total,
    };
    Ok(Json(json!(page)))
}

async fn stats(
    State(state): State<AppState>,
) -> Result<Json<Stats>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let s = tokio::task::spawn_blocking(move || db.stats())
        .await
        .map_err(join_error_response)?
        .map_err(db_error_response)?;
    Ok(Json(s))
}

/// POST /api/v1/images/reprocess：重新解析解码失败的图片（width=0 的图），
/// 重新提取尺寸/清晰度并生成缩略图。
async fn reprocess_images(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let db = state.db.clone();
    let library = state.library_dir();
    let thumbs = state.thumbs_dir();
    let (ok, failed) = tokio::task::spawn_blocking(move || {
        moevault_ingest::reprocess_broken_images(&db, &library, &thumbs)
    })
    .await
    .map_err(join_error_response)?
    .map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "reprocessed": ok, "failed": failed })))
}
