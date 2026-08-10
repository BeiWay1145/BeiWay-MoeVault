//! moevault-app：BeiWay-MoeVault 主服务入口。
//!
//! 启动流程：加载配置 → 打开 SQLite（含迁移）→ 构建路由 →
//! 可选托管前端静态资源 → 监听端口（优雅退出）。
//!
//! release 模式隐藏控制台窗口（桌面壳 spawn 时不弹黑框）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use moevault_api::{build_router, AppState};
use moevault_core::Config;
use moevault_db::Db;
use tower_http::services::{ServeDir, ServeFile};

fn init_tracing(data_dir: &std::path::Path) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,moevault=debug"));
    // 日志写文件（data/logs/app.log），便于查看问题
    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("app.log"));
    match file {
        Ok(f) => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(true)
                .with_writer(std::sync::Mutex::new(f))
                .try_init();
        }
        Err(e) => {
            eprintln!("[MoeVault] 日志文件打开失败，回退 stdout: {e}");
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(true)
                .try_init();
        }
    }
}

#[tokio::main]
async fn main() {
    let config = Config::from_env();
    init_tracing(&config.data_dir);

    if let Err(e) = config.validate() {
        tracing::error!("配置校验失败: {e}");
        std::process::exit(1);
    }
    tracing::info!("数据目录: {}", config.data_dir.display());

    let db = match Db::open(&config.db_path) {
        Ok(db) => db,
        Err(e) => {
            tracing::error!("数据库打开失败: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("数据库就绪: {}", config.db_path.display());

    let state = AppState::new(db, config.data_dir.clone(), config.infer_base_url.clone());
    let mut app = build_router(state);

    // 缩略图静态托管：/thumbs/<thumb_rel>
    let thumbs_dir = config.data_dir.join("thumbs");
    tracing::info!("托管缩略图目录: {}", thumbs_dir.display());
    app = app.nest_service("/thumbs", ServeDir::new(&thumbs_dir));

    // 生产模式：托管前端构建产物（SPA fallback 到 index.html）
    if let Some(dir) = &config.static_dir {
        let index = dir.join("index.html");
        tracing::info!("托管前端静态资源: {}", dir.display());
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    let addr = format!("{}:{}", config.host, config.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("监听 {addr} 失败: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("BeiWay-MoeVault 服务已启动: http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("服务运行失败");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("收到退出信号，正在优雅关闭...");
}
