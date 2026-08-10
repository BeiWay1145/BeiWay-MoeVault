//! API 共享状态与 WS 事件广播。

use std::time::Instant;

use moevault_db::Db;
use tokio::sync::broadcast;

/// WS 事件：JSON 字符串（结构见 docs/TECH_DETAILS.md 第 3 节）。
pub type WsEvent = String;

/// 应用共享状态。
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// WS 事件广播通道。
    pub ws_tx: broadcast::Sender<WsEvent>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        let (ws_tx, _) = broadcast::channel(256);
        Self {
            db,
            ws_tx,
            started_at: Instant::now(),
        }
    }

    /// 向所有连接的 WS 客户端广播事件（JSON 字符串）。
    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }
}
