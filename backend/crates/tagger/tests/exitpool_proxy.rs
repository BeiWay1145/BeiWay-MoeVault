//! 出口池的集成验证测试（经本地 Clash 端口访问 SauceNAO）。
//!
//! 说明：这些测试**依赖本机运行中的 Clash**（7901-7905 端口）。
//! 未检测到代理端口时自动跳过，不影响 CI 或其它开发环境。

#![cfg(test)]

use std::time::Duration;

/// 检测某个本地代理端口是否在监听。
fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// 经代理访问 SauceNAO 首页：验证 CONNECT 隧道 + TLS 可用。
///
/// 这是多出口功能的核心前置条件 —— 若此测试失败，
/// 说明 reqwest 的 TLS 特性与代理不兼容（曾因 webpki-roots 出现该问题）。
#[tokio::test]
async fn proxy_can_reach_saucenao() {
    let ports: Vec<u16> = (7901..=7905).filter(|p| port_listening(*p)).collect();
    if ports.is_empty() {
        eprintln!("跳过：本机未运行 Clash（7901-7905 无监听）");
        return;
    }
    let mut ok = 0;
    for p in &ports {
        let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{p}")).expect("构造代理失败");
        let client = reqwest::Client::builder()
            .user_agent("MoeVault/0.1 test")
            .proxy(proxy)
            .timeout(Duration::from_secs(25))
            .build()
            .expect("构建客户端失败");
        match client.get("https://saucenao.com/").send().await {
            Ok(r) => {
                eprintln!("端口 {p} -> HTTP {}", r.status());
                ok += 1;
            }
            Err(e) => eprintln!("端口 {p} -> 失败: {e}"),
        }
    }
    // 只要有任意一个出口可用即视为通过（个别节点可能临时抖动）
    assert!(ok > 0, "所有代理端口都无法访问 SauceNAO（检查 Clash 与 TLS 特性）");
}