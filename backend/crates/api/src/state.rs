//! API 共享状态与 WS 事件广播。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use moevault_db::Db;
use moevault_tagger::ApiKeyPool;
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
    /// WS 事件广播通道。
    pub ws_tx: broadcast::Sender<WsEvent>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf, infer_base_url: String) -> Self {
        let (ws_tx, _) = broadcast::channel(256);
        Self {
            db,
            data_dir,
            infer_base_url,
            sauce_pool: Arc::new(RwLock::new(None)),
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
