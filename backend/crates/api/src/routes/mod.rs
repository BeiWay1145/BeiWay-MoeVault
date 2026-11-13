//! 路由模块组织。

pub mod aesthetic;
pub mod dedup;
pub mod dict;
pub mod export;
pub mod health;
pub mod images;
pub mod import;
pub mod logs;
pub mod replace;
pub mod search;
pub mod settings;
pub mod tagging;
pub mod tasks;
pub mod trash;
pub mod ws;

use axum::Json;
use moevault_core::ErrorKind;
use serde_json::{json, Value};

/// HTTP 错误响应的统一类型（状态码 + JSON 错误体）。
pub type ApiError = (axum::http::StatusCode, Json<Value>);

/// handler 的统一返回类型。
///
/// 之前 77 个 handler 各自手写 `Result<Json<Value>, (axum::http::StatusCode, Json<Value>)>`，
/// 既冗长又让"换个错误表示法"变成 77 处批量改。统一别名后只需改这一行。
pub type ApiResult<T> = Result<Json<T>, ApiError>;

/// 统一错误响应体（docs/TECH_DETAILS.md 第 2 节约定）。
pub fn error_response(kind: ErrorKind, message: impl Into<String>) -> (axum::http::StatusCode, Json<Value>) {
    (
        axum::http::StatusCode::from_u16(kind.status_code()).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        Json(json!({
            "error": { "code": kind.as_str(), "message": message.into() }
        })),
    )
}

/// 把任意 AppError 映射为 HTTP 错误响应。
pub fn app_error_response(e: moevault_core::AppError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(e.kind, e.message)
}

/// 把 DbError 映射为 HTTP 错误响应。
pub fn db_error_response(e: moevault_db::DbError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(moevault_core::ErrorKind::Db, e.to_string())
}

/// 把 tokio JoinError 映射为 HTTP 错误响应（内部错误）。
pub fn join_error_response(e: tokio::task::JoinError) -> (axum::http::StatusCode, Json<Value>) {
    error_response(moevault_core::ErrorKind::Internal, format!("后台任务失败: {e}"))
}

/// 任务收尾状态判定。
///
/// worker 跑完后要把 job 状态写成 `done`/`failed`，但如果用户在这期间点了「中断」，
/// `cancel_task` 已经把该 job 置为 `cancelled`。此时若直接写 `done`，
/// 任务中心里那条任务会「自己变成已完成」——用户视角是中断失败。
///
/// 所以所有任务收尾都走这里：先回读一次当前状态，已是 `cancelled` 就保持不变。
pub fn final_job_status(db: &moevault_db::Db, job_id: i64, status: &'static str) -> &'static str {
    match db.get_job(job_id) {
        // 只有「已取消」需要保护：用户点了中断，任务就不能再被写成 done/failed。
        Ok(Some(job)) if job.status == "cancelled" => "cancelled",
        _ => status,
    }
}

/// 构造任务收尾的 WS 事件。
///
/// 这里统一处理三件事，避免每个任务入口各抄一份 json! 模板（曾散落 10 段）：
/// 1. 被中断（final_status == "cancelled"）时发 `task.cancelled`，而不是 task.done——
///    前端据此显示"已中断"，不会误报成功。
/// 2. 成功/失败/取消三种情况都带 `status` 字段，前端不必再猜。
/// 3. kind 与 job_id 恒定在 payload 里，便于前端精确定位到某条任务。
pub fn task_end_event<T: ProgressLike, E: std::fmt::Display>(
    final_status: &'static str,
    kind: &'static str,
    job_id: i64,
    result: &Result<T, E>,
) -> Value {
    let done = result.as_ref().map(|p| p.done()).unwrap_or(0);
    let failed = result.as_ref().map(|p| p.failed()).unwrap_or(0);
    let total = result.as_ref().map(|p| p.total()).unwrap_or(0);
    if final_status == "cancelled" {
        return json!({
            "type": "task.cancelled",
            "payload": { "job_id": job_id, "kind": kind, "status": "cancelled", "done": done, "failed": failed, "total": total },
        });
    }
    match result {
        Ok(_) => json!({
            "type": "task.done",
            "payload": { "job_id": job_id, "kind": kind, "status": final_status, "done": done, "failed": failed, "total": total },
        }),
        Err(e) => json!({
            "type": "task.failed",
            "payload": { "job_id": job_id, "kind": kind, "status": final_status, "error": e.to_string() },
        }),
    }
}

/// 任务进度类型的统一访问接口（只读三项计数）。
///
/// 存在的理由：打标（TagProgress/SauceProgress）与美学（AestheticProgress）
/// 是不同 crate 的独立结构体，收尾事件只需要 done/failed/total，
/// 用 trait 统一比在 api 层写三个 match 分支更省。
pub trait ProgressLike {
    fn done(&self) -> i64;
    fn failed(&self) -> i64;
    fn total(&self) -> i64;
}

impl ProgressLike for moevault_tagger::TagProgress {
    fn done(&self) -> i64 { self.done as i64 }
    fn failed(&self) -> i64 { self.failed as i64 }
    fn total(&self) -> i64 { self.total as i64 }
}

impl ProgressLike for moevault_tagger::SauceProgress {
    fn done(&self) -> i64 { self.done as i64 }
    fn failed(&self) -> i64 { self.failed as i64 }
    fn total(&self) -> i64 { self.total as i64 }
}

impl ProgressLike for moevault_tagger::AestheticProgress {
    fn done(&self) -> i64 { self.done as i64 }
    fn failed(&self) -> i64 { self.failed as i64 }
    fn total(&self) -> i64 { self.total as i64 }
}
