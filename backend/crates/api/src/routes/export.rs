//! 批量导出：POST /api/v1/images/export。
//!
//! 从库目录复制选中图片到目标目录（自动创建），可选：
//! - 同名 .txt 标签文件（无标签则不生成）
//! - 打包为 ZIP（压缩级别 0 仅存储 / 1 快速 / 2 正常 / 3 极限；7Z 暂不支持）
//! - 导出后移入回收站
//! 目标目录：`{target_dir}/{bundle_name}/`（未填 bundle_name 直接用 target_dir）。

use std::io::{Read, Write};
use std::path::PathBuf;

use axum::{extract::State, routing::post, Json, Router};
use flate2::Compression;
use moevault_core::ErrorKind;
use serde::Deserialize;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/images/export", post(export_images))
        .route("/api/v1/system/downloads-dir", axum::routing::get(default_downloads_dir))
        .route("/api/v1/system/open-explorer", post(open_explorer))
}

/// POST /api/v1/system/open-explorer：在资源管理器中打开/选中路径（导出后跳转用）。
#[derive(Debug, Deserialize)]
pub struct OpenExplorerReq {
    pub path: String,
}

async fn open_explorer(
    Json(req): Json<OpenExplorerReq>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "path 不能为空"));
    }
    // Windows：直接调 explorer.exe（不经 cmd，避免引号被 cmd 吞掉导致打开主目录）。
    // 目录 → 打开目录页；文件 → /select, 选中。
    let is_file = std::path::Path::new(path).is_file();
    let mut cmd = std::process::Command::new("explorer.exe");
    if is_file {
        cmd.arg("/select,").arg(path);
    } else {
        cmd.arg(path);
    }
    let _ = cmd.spawn();
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/v1/system/downloads-dir：返回用户真实下载目录（导出目标默认值）。
/// 优先注册表（用户可能把下载目录改到其他盘，如 D:\Downloads），回退 %USERPROFILE%\Downloads。
async fn default_downloads_dir() -> Json<Value> {
    let dir = registry_downloads_dir()
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|p| std::path::Path::new(&p).join("Downloads"))
                .filter(|p| p.is_dir())
                .map(|p| p.to_string_lossy().to_string())
        })
        .or_else(dirs_downloads)
        .unwrap_or_else(|| ".".to_string());
    Json(json!({ "dir": dir }))
}

/// 从注册表读取真实下载目录（用户重定向后仍准确）。
fn registry_downloads_dir() -> Option<String> {
    const GUID: &str = "{374DE290-123F-4565-9164-39C4925E467B}";
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders";
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", GUID])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    // 输出形如：    {374DE290-...}    REG_SZ    D:\Downloads
    let value = text
        .lines()
        .find_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            // REG_SZ 后跟路径（可能含空格 → 取最后两个有效段之后的剩余）
            let idx = parts.iter().position(|p| *p == "REG_SZ")?;
            let rest = l.splitn(idx + 3, ' ').nth(idx + 2)?.trim();
            if rest.is_empty() { None } else { Some(rest.to_string()) }
        })?;
    if std::path::Path::new(&value).is_dir() {
        Some(value)
    } else {
        None
    }
}

/// 下载目录回退：已知常见位置（不引入额外依赖）。
fn dirs_downloads() -> Option<String> {
    // Windows: %USERPROFILE%\Downloads 已在上面处理；这里再尝试 OneDrive/显式路径
    let p = std::env::var("USERPROFILE").ok()?;
    for cand in [format!("{p}\\Downloads"), format!("{p}\\OneDrive\\Downloads")] {
        if std::path::Path::new(&cand).is_dir() {
            return Some(cand);
        }
    }
    None
}

#[derive(Debug, Deserialize)]
pub struct ExportRequest {
    pub ids: Vec<i64>,
    /// 导出目标根目录（不存在则创建）。
    pub target_dir: String,
    /// 子文件夹/压缩包名（空 = 不创建子目录）。
    #[serde(default)]
    pub bundle_name: Option<String>,
    /// 打包为 ZIP（默认 false：散文件 + 子文件夹）。
    #[serde(default)]
    pub pack: bool,
    /// zip / 7z（7z 暂不支持，本机无 7z 二进制）。
    #[serde(default = "default_format")]
    pub pack_format: String,
    /// 0 仅存储 / 1 快速 / 2 正常 / 3 极限。
    #[serde(default)]
    pub pack_level: u8,
    /// 导出同名 .txt 标签文件（无标签不生成）。
    #[serde(default)]
    pub with_tags: bool,
    /// 导出后移入回收站。
    #[serde(default)]
    pub recycle_after: bool,
}

fn default_format() -> String {
    "zip".to_string()
}

async fn export_images(
    State(state): State<AppState>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    if req.ids.is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "ids 不能为空"));
    }
    if req.pack && req.pack_format == "7z" {
        return Err(error_response(
            ErrorKind::InvalidInput,
            "7z 打包暂不支持（本机未检测到 7z），请选择 ZIP",
        ));
    }
    let target_root = PathBuf::from(req.target_dir.trim());
    if target_root.as_os_str().is_empty() {
        return Err(error_response(ErrorKind::InvalidInput, "目标目录不能为空"));
    }
    let bundle = req
        .bundle_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // 打包 → 目标根下的 zip 文件；散件 → 根/bundle 子目录
    let out_dir: PathBuf = if req.pack {
        target_root.clone()
    } else if let Some(b) = bundle {
        target_root.join(b)
    } else {
        target_root.clone()
    };
    let zip_path = if req.pack {
        Some(target_root.join(format!(
            "{}.zip",
            bundle.unwrap_or("export")
        )))
    } else {
        None
    };

    let level = match req.pack_level {
        0 => Compression::none(),
        1 => Compression::fast(),
        3 => Compression::best(),
        _ => Compression::default(),
    };

    let st = state.clone();
    let ids = req.ids.clone();
    let with_tags = req.with_tags;
    let pack = req.pack;
    let zip_path_clone = zip_path.clone();
    let out_dir_clone = out_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_export(
            &st,
            &ids,
            &out_dir_clone,
            zip_path_clone.as_deref(),
            pack,
            level,
            with_tags,
        )
    })
    .await
    .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
    .map_err(|e| error_response(ErrorKind::Internal, e))?;

    // 导出后回收
    let mut recycled = 0i64;
    if req.recycle_after {
        let db = state.db.clone();
        let library = state.library_dir();
        let recycle = state.recycle_dir();
        for &id in &req.ids {
            let db2 = db.clone();
            let lib2 = library.clone();
            let rec2 = recycle.clone();
            let r = tokio::task::spawn_blocking(move || {
                moevault_dedup::recycle_image(&db2, id, "exported", &lib2, &rec2)
            })
            .await;
            if r.is_ok() {
                recycled += 1;
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "count": result.exported,
        "failed": result.failed,
        "tag_files": result.tag_files,
        "target": out_dir.to_string_lossy(),
        "zip": zip_path.map(|p| p.to_string_lossy().to_string()),
        "recycled": recycled,
    })))
}

struct ExportOutcome {
    exported: i64,
    failed: i64,
    tag_files: i64,
}

fn run_export(
    st: &AppState,
    ids: &[i64],
    out_dir: &PathBuf,
    zip_path: Option<&std::path::Path>,
    pack: bool,
    level: Compression,
    with_tags: bool,
) -> Result<ExportOutcome, String> {
    let db = st.db.clone();
    let library = st.library_dir();
    std::fs::create_dir_all(out_dir).map_err(|e| format!("创建导出目录失败: {e}"))?;

    // 打包模式：预先创建 zip 文件（错误提前传播）
    let mut zip_writer: Option<zip::ZipWriter<std::fs::File>> = if let Some(p) = zip_path {
        let file = std::fs::File::create(p).map_err(|e| format!("创建压缩包失败: {e}"))?;
        Some(zip::ZipWriter::new(file))
    } else {
        None
    };

    let mut exported = 0i64;
    let mut failed = 0i64;
    let mut tag_files = 0i64;

    for id in ids {
        let img = match db.get_image_by_id(*id) {
            Ok(Some(img)) => img,
            _ => {
                failed += 1;
                continue;
            }
        };
        let src = library.join(&img.rel_path);
        let name = img
            .rel_path
            .split(['\\', '/'])
            .next_back()
            .unwrap_or("image")
            .to_string();
        let data = match std::fs::File::open(&src).and_then(|mut f| {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            Ok(buf)
        }) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(image_id = id, error = %e, "导出：读取库文件失败");
                failed += 1;
                continue;
            }
        };

        if pack {
            let zw = zip_writer.as_mut().ok_or("zip 写入器未初始化")?;
            let method = if level == Compression::none() {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            let options = SimpleFileOptions::default().compression_method(method);
            zw.start_file(name.clone(), options)
                .map_err(|e| format!("写入压缩包失败: {e}"))?;
            zw.write_all(&data)
                .map_err(|e| format!("写入压缩包失败: {e}"))?;
        } else {
            let dst = out_dir.join(&name);
            if let Err(e) = std::fs::write(&dst, &data) {
                tracing::warn!(image_id = id, error = %e, "导出：写入目标失败");
                failed += 1;
                continue;
            }
        }
        exported += 1;

        // 标签 txt
        if with_tags {
            let tags = match db.image_tags(*id) {
                Ok(t) => t,
                Err(_) => Vec::new(),
            };
            if !tags.is_empty() {
                let tag_text = tags
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let txt_name = format!(
                    "{}.txt",
                    name.rsplit_once('.')
                        .map(|(s, _)| s.to_string())
                        .unwrap_or(name.clone())
                );
                let txt_content = format!("{tag_text},");
                if pack {
                    let zw = zip_writer.as_mut().ok_or("zip 写入器未初始化")?;
                    let options = SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated);
                    zw.start_file(txt_name, options)
                        .map_err(|e| format!("写入压缩包失败: {e}"))?;
                    zw.write_all(txt_content.as_bytes())
                        .map_err(|e| format!("写入压缩包失败: {e}"))?;
                    tag_files += 1;
                } else {
                    let dst = out_dir.join(&txt_name);
                    if std::fs::write(&dst, txt_content).is_ok() {
                        tag_files += 1;
                    }
                }
            }
        }
    }

    if pack {
        if let Some(zw) = zip_writer.take() {
            zw.finish().map_err(|e| format!("完成压缩包失败: {e}"))?;
        }
    }
    Ok(ExportOutcome { exported, failed, tag_files })
}
