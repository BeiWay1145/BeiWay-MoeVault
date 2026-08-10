//! GET /ws：WebSocket 事件推送。
//!
//! 连接建立后发送 `hello` 事件；后续接收服务端广播
//! （task.progress / library.updated / dedup.updated / stats.updated 等）。

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    routing::get,
    Router,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/ws", get(ws_handler))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // 握手事件
    let hello = json!({
        "type": "hello",
        "ts": now_secs(),
        "payload": { "service": "moevault" },
    })
    .to_string();
    if socket.send(Message::Text(hello.into())).await.is_err() {
        return;
    }

    let mut rx = state.ws_tx.subscribe();
    loop {
        tokio::select! {
            // 客户端消息（骨架阶段仅处理关闭）
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            // 服务端广播
            ev = rx.recv() => {
                match ev {
                    Ok(ev) => {
                        if socket.send(Message::Text(ev.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
