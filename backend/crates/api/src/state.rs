//! API 共享状态与 WS 事件广播。

use std::path::PathBuf;
use std::time::Instant;

use moevault_db::Db;
use tokio::sync::broadcast;

/// WS 事件：JSON 字符串（结构见 docs/TECH_DETAILS.md 第 3 节）。
pub type WsEvent = String;

/// 应用共享状态。
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// 运行时数据目录（library/ thumbs/ 的父目录）。
    pub data_dir: PathBuf,
    /// WS 事件广播通道。
    pub ws_tx: broadcast::Sender<WsEvent>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf) -> Self {
        let (ws_tx, _) = broadcast::channel(256);
        Self {
            db,
            data_dir,
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

    /// 向所有连接的 WS 客户端广播事件（JSON 字符串）。
    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }
}
