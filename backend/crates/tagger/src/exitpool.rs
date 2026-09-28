//! SauceNAO 多出口 IP 池（Clash 多端口轮换）。
//!
//! ## 要解决的问题
//!
//! SauceNAO 免费账号的限流是**按出口 IP** 计算的：4 次 / 30 秒（另有 100 次/日 的账号上限）。
//! 多 API key 无法突破该限制（同一 IP）。用户若使用 Clash，可为多个节点各开一个**入站端口**，
//! 应用按端口建多个出口，使每个出口独立计算 4 次/30 秒 → 打标更快。
//!
//! ## 配置方式（路径 A）
//!
//! 用户在 Clash 里为不同节点开多个本地端口（如 7901/7902/7903），
//! 应用侧只填**端口号**，固定拼成 `http://127.0.0.1:{port}`：
//! - 避免用户填入任意外部代理（安全）
//! - 交互最简单（只需抄 Clash 配置里的端口数字）
//!
//! ## 调度策略：按"空闲优先"而非固定轮换
//!
//! 需求（用户明确）：**当前出口被限流时，切换到还有空闲额度的出口**。
//! 因此不使用简单的 round-robin，而是：
//! 1. 每个出口维护自己的 4 次/30 秒 窗口与冷却状态；
//! 2. 取用时优先选**窗口未满且未冷却**的出口；
//! 3. 若当前出口刚触发限流（-2/-3），把它标为冷却，下一次自动换到其它可用出口；
//! 4. 全部出口都不可用 → 等待最早恢复的那个（与现有 keypool 的全局等待语义一致）。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

/// 单出口的 30 秒窗口上限（SauceNAO 免费账号按 IP 的经验值）。
pub const EXIT_WINDOW_LIMIT: usize = 4;
/// 窗口长度（秒）。
pub const EXIT_WINDOW_SECS: u64 = 30;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 单个出口的状态。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExitState {
    /// 本地端口号（127.0.0.1:port）。
    pub port: u16,
    /// 最近探测到的出口 IP（供 UI 展示，确认各端口确实走不同节点）。
    pub exit_ip: Option<String>,
    /// 冷却截止时间（Unix 秒）；0 表示无冷却。
    pub cooldown_until: u64,
    /// 已累计请求数（调试用）。
    pub total_requests: u64,
    /// 连续失败次数（超时/连接失败）；达到阈值后临时停用。
    pub consecutive_failures: u32,
    /// 是否因连续失败被停用。
    pub disabled: bool,
}

impl ExitState {
    fn available_at(&self, now: u64) -> u64 {
        self.cooldown_until.max(now)
    }
    /// 当前是否可用（未停用、未冷却、窗口未满）。
    fn usable(&self, now: u64, recent: &VecDeque<u64>) -> bool {
        if self.disabled {
            return false;
        }
        if now < self.cooldown_until {
            return false;
        }
        recent.len() < EXIT_WINDOW_LIMIT
    }
}

struct ExitInner {
    exits: Vec<ExitState>,
    /// 每个出口的请求时间戳队列（与 exits 同序）。
    recent: Vec<VecDeque<u64>>,
}

/// 多出口池。未启用时退化为"直连"（不设代理）。
#[derive(Clone)]
pub struct ExitPool {
    inner: Arc<Mutex<ExitInner>>,
    /// 每个出口的 HTTP 客户端（绑定 127.0.0.1:port）。索引与 exits 一致。
    clients: Arc<Vec<reqwest::Client>>,
    /// 直连客户端（未配置端口，或指定不使用代理时）。
    direct: reqwest::Client,
    /// 是否启用（用户开关）。
    enabled: bool,
}

impl ExitPool {
    /// 按端口列表构建。`enabled=false` 或端口为空时只会有一个直连出口。
    pub fn new(ports: &[u16], enabled: bool) -> Self {
        let direct = reqwest::Client::builder()
            .user_agent("MoeVault/0.1 (image manager)")
            .build()
            .expect("构建直连客户端失败");

        if !enabled || ports.is_empty() {
            return Self {
                inner: Arc::new(Mutex::new(ExitInner {
                    exits: vec![ExitState {
                        port: 0,
                        exit_ip: None,
                        cooldown_until: 0,
                        total_requests: 0,
                        consecutive_failures: 0,
                        disabled: false,
                    }],
                    recent: vec![VecDeque::new()],
                })),
                clients: Arc::new(vec![]),
                direct,
                enabled: false,
            };
        }

        // 去重并排序，保证顺序稳定（便于 UI 与日志对应）
        let mut uniq: Vec<u16> = ports.to_vec();
        uniq.sort_unstable();
        uniq.dedup();

        let mut clients = Vec::new();
        let mut exits = Vec::new();
        let mut recent = Vec::new();
        for p in &uniq {
            let proxy = format!("http://127.0.0.1:{p}");
            match reqwest::Proxy::all(&proxy) {
                Ok(px) => {
                    let c = reqwest::Client::builder()
                        .user_agent("MoeVault/0.1 (image manager)")
                        .proxy(px)
                        .timeout(Duration::from_secs(120))
                        .build();
                    if let Ok(c) = c {
                        clients.push(c);
                        exits.push(ExitState {
                            port: *p,
                            exit_ip: None,
                            cooldown_until: 0,
                            total_requests: 0,
                            consecutive_failures: 0,
                            disabled: false,
                        });
                        recent.push(VecDeque::new());
                    }
                }
                Err(_) => continue,
            }
        }

        if exits.is_empty() {
            // 端口都无效 → 退化为直连
            return Self::new(&[], false);
        }

        Self {
            inner: Arc::new(Mutex::new(ExitInner { exits, recent })),
            clients: Arc::new(clients),
            direct,
            enabled: true,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 出口数量（未启用时为 1，表示直连）。
    pub async fn count(&self) -> usize {
        self.inner.lock().await.exits.len()
    }

    ///
    /// 取一个可用出口，返回 (客户端, 出口索引)。
    ///
    /// 策略（按用户需求）：**优先选有空闲额度的出口**；
    /// 当前出口被限流后由 `note_rate_limited` 标记，下次自动换到其它出口。
    /// 全部不可用时等待最早恢复的那个（不空转）。
    ///
    pub async fn acquire(&self) -> (reqwest::Client, usize) {
        loop {
            let now = now_secs();
            let mut inner = self.inner.lock().await;
            // 清理过期窗口记录
            for q in inner.recent.iter_mut() {
                while let Some(front) = q.front().copied() {
                    if now.saturating_sub(front) >= EXIT_WINDOW_SECS {
                        q.pop_front();
                    } else {
                        break;
                    }
                }
            }
            // 找一个可用出口（按顺序取第一个可用的 —— 已是"有空闲优先"的语义，
            // 因为不可用的会被跳过；前一个被限流后自然落到下一个）
            let mut chosen: Option<usize> = None;
            for (i, e) in inner.exits.iter().enumerate() {
                if e.usable(now, &inner.recent[i]) {
                    chosen = Some(i);
                    break;
                }
            }
            if let Some(i) = chosen {
                inner.recent[i].push_back(now);
                inner.exits[i].total_requests += 1;
                drop(inner);
                let client = self
                    .clients
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| self.direct.clone());
                return (client, i);
            }
            // 全部不可用 → 等最早恢复的
            let earliest = inner
                .exits
                .iter()
                .filter(|e| !e.disabled)
                .map(|e| e.available_at(now))
                .min();
            let wait = match earliest {
                Some(t) => t.saturating_sub(now).max(1),
                // 全部被停用（连续失败）→ 稍等后重试，避免死循环
                None => 5,
            };
            drop(inner);
            tokio::time::sleep(Duration::from_secs(wait.min(EXIT_WINDOW_SECS))).await;
        }
    }

    /// 出口触发限流 → 标记冷却（下次自动换到其它出口）。
    pub async fn note_rate_limited(&self, idx: usize, secs: u64) {
        let mut inner = self.inner.lock().await;
        if let Some(e) = inner.exits.get_mut(idx) {
            e.cooldown_until = now_secs() + secs.max(1);
            e.consecutive_failures = 0; // 限流说明出口本身是通的
            tracing::warn!(port = e.port, secs, "出口被限流，切换其它出口");
        }
    }

    /// 出口请求失败（超时/连不上）：累计失败，达阈值则停用该出口。
    pub async fn note_failure(&self, idx: usize) {
        const MAX_FAILS: u32 = 5;
        let mut inner = self.inner.lock().await;
        if let Some(e) = inner.exits.get_mut(idx) {
            e.consecutive_failures += 1;
            if e.consecutive_failures >= MAX_FAILS {
                e.disabled = true;
                tracing::error!(port = e.port, "出口连续失败 {MAX_FAILS} 次，已停用");
            }
        }
    }

    /// 请求成功 → 清零失败计数。
    pub async fn note_success(&self, idx: usize) {
        let mut inner = self.inner.lock().await;
        if let Some(e) = inner.exits.get_mut(idx) {
            e.consecutive_failures = 0;
        }
    }

    /// 状态快照（供 UI 展示各出口的可用性与出口 IP）。
    pub async fn snapshot(&self) -> Vec<ExitState> {
        self.inner.lock().await.exits.clone()
    }

    /// 探测各出口的出口 IP 与可用性（设置页「测试全部」）。
    ///
    /// 用 `https://api.ipify.org` 获取出口 IP —— 若多个端口返回相同 IP，
    /// 说明 Clash 配置未生效（轮换无意义），UI 需明确警告。
    pub async fn probe_all(&self) -> Vec<(u16, Option<String>, Option<String>)> {
        let clients = self.clients.clone();
        let direct = self.direct.clone();
        let ports: Vec<u16> = {
            let inner = self.inner.lock().await;
            inner.exits.iter().map(|e| e.port).collect()
        };
        let mut out = Vec::new();
        for (i, port) in ports.iter().enumerate() {
            let client = if *port == 0 {
                direct.clone()
            } else {
                clients.get(i).cloned().unwrap_or_else(|| direct.clone())
            };
            let t0 = std::time::Instant::now();
            let res = client
                .get("https://api.ipify.org")
                .timeout(Duration::from_secs(8))
                .send()
                .await;
            match res {
                Ok(r) => {
                    let ip = r.text().await.ok().map(|s| s.trim().to_string());
                    let ms = t0.elapsed().as_millis() as u64;
                    // 记录到状态里供 UI 显示
                    {
                        let mut inner = self.inner.lock().await;
                        if let Some(e) = inner.exits.get_mut(i) {
                            e.exit_ip = ip.clone();
                            e.disabled = false; // 探测成功即恢复
                            e.consecutive_failures = 0;
                        }
                    }
                    out.push((*port, ip, Some(format!("{ms} ms"))));
                }
                Err(e) => {
                    out.push((*port, None, Some(format!("失败: {e}"))));
                }
            }
        }
        out
    }
}