//! API 共享状态与 WS 事件广播。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use moevault_db::Db;
use moevault_tagger::{ApiKeyPool, ExitPool};
use tokio::sync::{broadcast, RwLock};

/// WS 事件：JSON 字符串（结构见 docs/TECH_DETAILS.md 第 3 节）。
pub type WsEvent = String;

/// 应用共享状态。
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// 运行时数据目录（library/ thumbs/ 的父目录）。
    pub data_dir: PathBuf,
    /// Python 推理服务基地址（本地打标回退用）。
    pub infer_base_url: String,
    /// SauceNAO 多 key 调度器（全局单例，配额/冷却跨请求保持）。
    pub sauce_pool: Arc<RwLock<Option<Arc<ApiKeyPool>>>>,
    ///
    /// 增强2：SauceNAO 多出口 IP 池（Clash 多入站端口）。
    /// 与 sauce_pool 平行的全局单例；未配置出口时为空/直连。
    pub exit_pool: Arc<RwLock<Option<Arc<ExitPool>>>>,
    /// WS 事件广播通道。
    pub ws_tx: broadcast::Sender<WsEvent>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf, infer_base_url: String) -> Self {
        let (ws_tx, _) = broadcast::channel(256);

        // 增强2：按已保存的配置初始化 SauceNAO 多出口池。
        // 放在这里（而非 app 的 main）是因为 app crate 不直接依赖 tagger，
        // 而 api crate 已依赖 —— 内聚到此可避免为一个初始化引入新的 crate 依赖。
        let exit_pool = {
            let ports = db
                .get_setting("sauce_proxy_ports")
                .ok()
                .flatten()
                .map(|s| {
                    s.split([',', ';', ' ', '\n'])
                        .filter_map(|p| p.trim().parse::<u16>().ok())
                        .filter(|p| *p > 0)
                        .collect::<Vec<u16>>()
                })
                .unwrap_or_default();
            let enabled = db
                .get_setting("sauce_proxy_enabled")
                .ok()
                .flatten()
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if enabled && !ports.is_empty() {
                tracing::info!(?ports, "SauceNAO 多出口已启用，构建出口池");
                Some(Arc::new(ExitPool::new(&ports, true)))
            } else {
                None
            }
        };

        Self {
            db,
            data_dir,
            infer_base_url,
            sauce_pool: Arc::new(RwLock::new(None)),
            exit_pool: Arc::new(RwLock::new(exit_pool)),
            ws_tx,
            started_at: Instant::now(),
        }
    }

    /// 库目录（`data/library`）。
    pub fn library_dir(&self) -> PathBuf {
        self.data_dir.join("library")
    }

    /// 缩略图目录（`data/thumbs`）。
    pub fn thumbs_dir(&self) -> PathBuf {
        self.data_dir.join("thumbs")
    }

    /// 回收站目录（`data/recycle`）。
    pub fn recycle_dir(&self) -> PathBuf {
        self.data_dir.join("recycle")
    }

    /// 向所有连接的 WS 客户端广播事件（JSON 字符串）。
    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }
}
