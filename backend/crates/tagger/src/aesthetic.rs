//! 美学评分流水线：调用本地推理服务（/infer/aesthetic）为图片批量评分。
//!
//! 流程：取未评分（aesthetic_score IS NULL）的 active 图片 → 逐张调推理服务
//! → 写回 aesthetic_score。单张失败不中断，记 failed 继续。

use std::path::Path;

use moevault_db::Db;
use serde::Deserialize;
use tracing::{info, warn};

use crate::TaggerError;
use crate::pipeline::filter_eligible;

/// 美学评分进度统计。
#[derive(Debug, Clone, Default, Copy)]
pub struct AestheticProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
}

#[derive(Debug, Deserialize)]
struct AestheticResp {
    score: f64,
}

/// 为指定图片（或全部未评分 active 图）执行美学评分。
pub async fn run_aesthetic_pipeline(
    db: &Db,
    infer: &crate::InferClient,
    library_dir: &Path,
    image_ids: Option<Vec<i64>>,
    job_id: Option<i64>,
) -> Result<AestheticProgress, TaggerError> {
    let ids = match image_ids {
        Some(ids) => {
            // 批量（>1 张）过滤已有美学分的图；单张（详情页手动重评）保留强制语义
            if ids.len() > 1 {
                filter_eligible(db, "aesthetic", &ids, false)?
            } else {
                ids
            }
        }
        None => db.unscored_active_images(10000)?,
    };
    if ids.is_empty() {
        return Ok(AestheticProgress::default());
    }
    info!(count = ids.len(), "美学评分流水线：开始");

    let mut progress = AestheticProgress {
        total: ids.len(),
        ..Default::default()
    };

    for image_id in &ids {
        match score_one(db, infer, library_dir, *image_id).await {
            Ok(()) => {
                progress.done += 1;
                let _ = db.add_log("info", "aesthetic", &format!("图片 #{image_id} 美学评分成功"));
            }
            Err(e) => {
                warn!(image_id, error = %e, "美学评分失败");
                progress.failed += 1;
                let rel = db
                    .get_image_by_id(*image_id)
                    .ok()
                    .flatten()
                    .map(|i| i.rel_path)
                    .unwrap_or_default();
                let _ = db.add_log("error", "aesthetic", &format!("图片 #{image_id}（{rel}）美学评分失败：{e}"));
            }
        }
        // 实时写回 job 进度（任务中心进度条可见推进）
        if let Some(jid) = job_id {
            let _ = db.update_job(jid, "running", progress.done as i64, progress.failed as i64, None);
        }
    }
    info!(done = progress.done, failed = progress.failed, "美学评分流水线完成");
    Ok(progress)
}

async fn score_one(
    db: &Db,
    infer: &crate::InferClient,
    library_dir: &Path,
    image_id: i64,
) -> Result<(), TaggerError> {
    let img = db
        .get_image_by_id(image_id)?
        .ok_or_else(|| TaggerError::Invalid(format!("图片 {image_id} 不存在")))?;
    let file_path = library_dir.join(&img.rel_path);

    // 调用推理服务 /infer/aesthetic
    let score = infer
        .infer_aesthetic(file_path.as_path())
        .await
        .map_err(|e| {
            TaggerError::Invalid(format!(
                "美学评分失败（请确认推理服务已启动: python/run_server.bat，端口 8001）：{e}"
            ))
        })?;
    db.set_aesthetic_score(image_id, score)?;
    info!(image_id, score, "美学评分完成");
    Ok(())
}

impl crate::InferClient {
    /// 调用 /infer/aesthetic 获取美学分（1-5）。
    pub async fn infer_aesthetic(&self, path: &Path) -> Result<f64, TaggerError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            path: &'a str,
        }
        // 传绝对路径（推理服务 cwd 与后端不同，相对路径会 404）
        let abs_path = crate::pipeline::to_absolute_path(path)?;
        let resp = self
            .http()
            .post(format!("{}/infer/aesthetic", self.base_url))
            .json(&Req {
                path: &abs_path,
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let body_snippet = resp
                .text()
                .await
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>();
            return Err(TaggerError::Invalid(format!(
                "推理服务 /infer/aesthetic 返回 {status}，响应: {body_snippet}"
            )));
        }
        let body: AestheticResp = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                return Err(TaggerError::Invalid(format!(
                    "推理服务 /infer/aesthetic 响应解析失败: {e}"
                )));
            }
        };
        Ok(body.score)
    }
}
