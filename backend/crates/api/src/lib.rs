//! moevault-api：axum HTTP 路由、WS、错误响应封装。

pub mod routes;
pub mod state;

use axum::Router;

pub use state::AppState;

/// 议题6（BUG追踪器）：全局错误日志中间件——所有 4xx/5xx 响应自动写入 app_logs。
/// 后端错误（error sending request / 参数错误 / 404 等）不再遗漏。
async fn error_log_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().path().to_string();
    let resp = next.run(req).await;
    let status = resp.status();
    if status.as_u16() >= 400 {
        // /logs 自身的错误不记录（避免写日志失败时循环刷屏）
        let is_logs_api = uri.starts_with("/api/v1/logs");
        if !is_logs_api {
            let level = if status.is_server_error() { "error" } else { "warn" };
            let db = state.db.clone();
            let msg = format!("{method} {uri} → {status}");
            tokio::task::spawn_blocking(move || {
                let _ = db.add_log(level, "http", &msg);
            });
        }
    }
    resp
}

/// 构建应用路由（业务接口）。静态资源托管由 app 层按需叠加。
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::router())
        .merge(routes::images::router())
        .merge(routes::export::router())
        .merge(routes::replace::router())
        .merge(routes::import::router())
        .merge(routes::dedup::router())
        .merge(routes::trash::router())
        .merge(routes::tagging::router())
        .merge(routes::aesthetic::router())
        .merge(routes::tasks::router())
        .merge(routes::logs::router())
        .merge(routes::search::router())
        .merge(routes::dict::router())
        .merge(routes::settings::router())
        .merge(routes::ws::router())
        .layer(axum::middleware::from_fn_with_state(state.clone(), error_log_middleware))
        .with_state(state)
}
