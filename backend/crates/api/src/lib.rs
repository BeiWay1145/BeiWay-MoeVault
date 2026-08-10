//! moevault-api：axum HTTP 路由、WS、错误响应封装。

pub mod routes;
pub mod state;

use axum::Router;

pub use state::AppState;

/// 构建应用路由（业务接口）。静态资源托管由 app 层按需叠加。
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::router())
        .merge(routes::images::router())
        .merge(routes::import::router())
        .merge(routes::ws::router())
        .with_state(state)
}
