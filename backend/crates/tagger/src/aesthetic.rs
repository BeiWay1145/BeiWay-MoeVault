//! 美学评分流水线：调用本地推理服务（/infer/aesthetic）为图片批量评分。
//!
//! 流程：取未评分（aesthetic_score IS NULL）的 active 图片 → 逐张调推理服务
//! → 写回 aesthetic_score。单张失败不中断，记 failed 继续。

use std::path::Path;

use moevault_db::Db;
use serde::Deserialize;
use tracing::{info, warn};

use crate::TaggerError;

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
) -> Result<AestheticProgress, TaggerError> {
    let ids = match image_ids {
        Some(ids) => ids,
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
            Ok(()) => progress.done += 1,
            Err(e) => {
                warn!(image_id, error = %e, "美学评分失败");
                progress.failed += 1;
            }
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
    let score = infer.infer_aesthetic(file_path.as_path()).await?;
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
        let resp = self
            .http()
            .post(format!("{}/infer/aesthetic", self.base_url))
            .json(&Req {
                path: &path.to_string_lossy(),
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(TaggerError::Invalid(format!(
                "推理服务返回 {}",
                resp.status()
            )));
        }
        let body: AestheticResp = resp.json().await?;
        Ok(body.score)
    }
}
