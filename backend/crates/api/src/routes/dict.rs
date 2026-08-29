//! 中文字典导入：/api/v1/dict/import。
//!
//! 下载 ffdfkj/ffdkj-Danbooru_Tag-Chinese-English-Translation-Table 的 tag.sqlite
//! （name/cn_name/category/post_count，每日更新 317K+ 条），批量回填 tags.name_cn
//! （仅填空缺，不覆盖手工别名）。设置页「导入中文字典」按钮触发。

use std::path::PathBuf;

use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use moevault_core::ErrorKind;
use serde_json::{json, Value};

use crate::state::AppState;

use super::{db_error_response, error_response};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/dict/import", post(import_dict))
}

const DICT_URL: &str =
    "https://github.com/ffdkj/ffdkj-Danbooru_Tag-Chinese-English-Translation-Table/raw/main/tag.sqlite";
/// 下载超时与体积上限（317K 行 sqlite 约 60-80MB，上限 256MB 防异常）。
const MAX_BYTES: u64 = 256 * 1024 * 1024;

async fn import_dict(
    State(state): State<AppState>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // 1) 下载 tag.sqlite 到临时文件
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("MoeVault/0.1")
        .build()
        .map_err(|e| error_response(ErrorKind::Internal, format!("HTTP 客户端初始化失败: {e}")))?;
    let resp = client
        .get(DICT_URL)
        .send()
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("下载中文字典失败（网络？）: {e}")))?;
    if !resp.status().is_success() {
        return Err(error_response(
            ErrorKind::Internal,
            format!("下载中文字典失败: HTTP {}", resp.status()),
        ));
    }
    let tmp = std::env::temp_dir().join(format!("moevault_tag_dict_{}.sqlite", std::process::id()));
    {
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| error_response(ErrorKind::Internal, format!("读取下载内容失败: {e}")))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(error_response(
                ErrorKind::Internal,
                "字典文件超过 256MB 上限，已中止".to_string(),
            ));
        }
        std::fs::write(&tmp, &bytes)
            .map_err(|e| error_response(ErrorKind::Internal, format!("写入临时文件失败: {e}")))?;
    }

    // 2) 解析导入（db crate 打开 sqlite 并批量回填）
    let db = state.db.clone();
    let path: PathBuf = tmp.clone();
    let result = tokio::task::spawn_blocking(move || db.import_cn_dict(&path))
        .await
        .map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))?
        .map_err(db_error_response)?;

    let _ = std::fs::remove_file(&tmp);
    let (matched, updated, missing) = result;
    Ok(Json(json!({
        "matched": matched,
        "updated": updated,
        "missing": missing,
    })))
}
