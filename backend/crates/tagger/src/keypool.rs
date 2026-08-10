//! SauceNAO 多 API key 调度器。
//!
//! 需求（用户新功能）：
//! 1. 多个 API key 轮番调用：冷却期归零（含容错延时）的 key 才分配任务
//! 2. 追踪每个 key 的剩余配额（short_remaining：30s 窗口 / long_remaining：当日）
//! 3. 配额预警：long_remaining < 10 时当日停用该 key，直到次日重置
//! 4. 记录各 key 状态（冷却中/可用/配额耗尽），供调试与 UI 展示

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

/// 日配额预警阈值：剩余 < 此值当日停用该 key。
pub const DAILY_QUOTA_WARN: i64 = 10;
/// 冷却容错延时（秒）：冷却归零后再等待该时长，降低撞限流概率。
pub const COOLDOWN_GRACE_SECS: u64 = 2;
/// 默认 30s 窗口请求上限（免费账号经验值，由响应头校准）。
pub const DEFAULT_SHORT_LIMIT: u32 = 6;

/// 单个 key 的状态。
/// 时间字段用 epoch 秒存储（可序列化），运行时转 Instant。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyState {
    /// API key（明文，仅内部使用）。
    pub api_key: String,
    /// 30s 窗口剩余配额。
    pub short_remaining: i64,
    /// 30s 窗口上限。
    pub short_limit: i64,
    /// 当日剩余配额。
    pub long_remaining: i64,
    /// 冷却到期时刻（epoch 秒）。
    pub cooldown_until_secs: Option<u64>,
    /// 当日是否已停用（配额预警触发）。
    pub daily_paused: bool,
    /// 当日 UTC 日期（yyyyMMdd），跨日重置 daily_paused。
    pub daily_date: String,
    /// 最近一次请求时间（epoch 秒）。
    pub last_used_secs: Option<u64>,
    /// 累计请求次数。
    pub total_requests: u64,
}

impl KeyState {
    fn new(api_key: String) -> Self {
        Self {
            api_key,
            short_remaining: DEFAULT_SHORT_LIMIT as i64,
            short_limit: DEFAULT_SHORT_LIMIT as i64,
            long_remaining: 95,
            cooldown_until_secs: None,
            daily_paused: false,
            daily_date: today_utc(),
            last_used_secs: None,
            total_requests: 0,
        }
    }

    /// 是否可用（未被当日停用 + 不在冷却期）。
    pub fn available(&self) -> bool {
        if self.daily_paused {
            return false;
        }
        match self.cooldown_until_secs {
            Some(secs) => now_secs() >= secs,
            None => true,
        }
    }

    /// 剩余冷却秒数（0 = 无冷却）。
    pub fn cooldown_secs(&self) -> u64 {
        match self.cooldown_until_secs {
            Some(secs) => secs.saturating_sub(now_secs()),
            None => 0,
        }
    }

    /// 设置冷却（从现在起 N 秒）。
    pub fn set_cooldown(&mut self, seconds: u64) {
        self.cooldown_until_secs = Some(now_secs() + seconds);
    }

    /// 标记最近使用。
    pub fn mark_used(&mut self) {
        self.last_used_secs = Some(now_secs());
        self.total_requests += 1;
    }
}

/// 当前 Unix 时间戳（秒）。
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 多 key 调度器（线程安全）。
/// 支持持久化：设置 persist_path 后每次状态变更自动保存 JSON 快照，重启恢复。
pub struct ApiKeyPool {
    inner: Mutex<PoolInner>,
}

struct PoolInner {
    keys: Vec<KeyState>,
    /// 轮转游标（round-robin 起点偏移）。
    cursor: usize,
    /// 持久化路径（None = 不持久化）。
    persist: Option<std::path::PathBuf>,
}

/// 持久化快照格式（磁盘 JSON）。
#[derive(serde::Serialize, serde::Deserialize)]
struct PoolSnapshot {
    keys: Vec<KeyState>,
    cursor: usize,
}

impl ApiKeyPool {
    /// 从多个 API key 构建调度器。
    pub fn new(keys: Vec<String>) -> Self {
        let keys: Vec<KeyState> = keys
            .into_iter()
            .filter(|k| !k.trim().is_empty())
            .map(KeyState::new)
            .collect();
        Self {
            inner: Mutex::new(PoolInner { keys, cursor: 0, persist: None }),
        }
    }

    /// 设置持久化路径（状态变更后自动保存）。
    pub fn set_persist_path(&self, path: std::path::PathBuf) {
        let mut guard = match self.inner.try_lock() { Ok(g) => g, Err(_) => return };
        guard.persist = Some(path);
        drop(guard);
        let _ = self.save_blocking();
    }

    /// 从持久化快照恢复（若文件存在且 key 匹配）。
    /// 返回是否成功恢复。
    pub fn load_from(path: &std::path::Path, keys: &[String]) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        let snap: PoolSnapshot = serde_json::from_str(&data).ok()?;
        // 校验 key 集合一致（配置变更时丢弃旧配额）
        let want: Vec<String> = keys
            .iter()
            .filter(|k| !k.trim().is_empty())
            .cloned()
            .collect();
        if snap.keys.len() != want.len() {
            return None;
        }
        for (k, w) in snap.keys.iter().zip(want.iter()) {
            if k.api_key != *w {
                return None;
            }
        }
        Some(Self {
            inner: Mutex::new(PoolInner {
                keys: snap.keys,
                cursor: snap.cursor,
                persist: Some(path.to_path_buf()),
            }),
        })
    }

    /// 保存快照（async 内调用）。
    async fn save(&self) {
        let guard = self.inner.lock().await;
        let Some(path) = guard.persist.clone() else { return };
        let snap = PoolSnapshot { keys: guard.keys.clone(), cursor: guard.cursor };
        drop(guard);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&snap) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// 保存快照（blocking 上下文）。
    fn save_blocking(&self) -> std::io::Result<()> {
        let guard = match self.inner.try_lock() { Ok(g) => g, Err(_) => return Ok(()) };
        let Some(path) = guard.persist.clone() else { return Ok(()) };
        let snap = PoolSnapshot { keys: guard.keys.clone(), cursor: guard.cursor };
        drop(guard);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&snap)
            .map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.keys.is_empty()
    }

    /// 全部 key 数量。
    pub async fn len(&self) -> usize {
        self.inner.lock().await.keys.len()
    }

    /// 等待并返回一个可用 key（阻塞直到有 key 冷却结束）。
    ///
    /// 轮转策略：从游标开始扫描，找第一个 available 的 key；若全不可用，
    /// 等待最短冷却 + 容错延时后重试。
    /// 返回 (key 状态克隆, 释放守卫所需的 index)。
    pub async fn acquire(&self) -> (String, usize) {
        loop {
            let mut inner = self.inner.lock().await;
            // 跨日重置
            let today = today_utc();
            for k in &mut inner.keys {
                if k.daily_date != today {
                    k.daily_date = today.clone();
                    k.daily_paused = false;
                    k.long_remaining = 95; // 日配额重置（经验默认，响应头会校准）
                }
            }
            if inner.keys.is_empty() {
                drop(inner);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            // 从游标找可用 key（轮转）
            let n = inner.keys.len();
            let mut found: Option<usize> = None;
            for offset in 0..n {
                let idx = (inner.cursor + offset) % n;
                if inner.keys[idx].available() {
                    found = Some(idx);
                    break;
                }
            }

            if let Some(idx) = found {
                inner.cursor = (idx + 1) % n; // 下次从下一个开始轮转
                inner.keys[idx].mark_used();
                let key = inner.keys[idx].api_key.clone();
                drop(inner);
                self.save().await;
                return (key, idx);
            }

            // 全不可用：等最短冷却 + 容错延时
            let min_cooldown = inner
                .keys
                .iter()
                .map(|k| k.cooldown_secs())
                .filter(|c| *c > 0)
                .min()
                .unwrap_or(5);
            let wait = min_cooldown + COOLDOWN_GRACE_SECS;
            drop(inner);
            tokio::time::sleep(Duration::from_secs(wait.max(1))).await;
        }
    }

    /// 请求完成后更新 key 状态（从响应头 + 返回的 index）。
    pub async fn update(
        &self,
        idx: usize,
        short_remaining: Option<i64>,
        long_remaining: Option<i64>,
    ) {
        let mut inner = self.inner.lock().await;
        if let Some(k) = inner.keys.get_mut(idx) {
            if let Some(v) = short_remaining {
                k.short_remaining = v;
                k.short_limit = k.short_limit.max(v);
            }
            if let Some(v) = long_remaining {
                k.long_remaining = v;
                // 配额预警：< 10 当日停用
                if v < DAILY_QUOTA_WARN {
                    k.daily_paused = true;
                    tracing::warn!(key_idx = idx, remaining = v, "SauceNAO 日配额预警（<{DAILY_QUOTA_WARN}），当日停用");
                }
            }
        }
        drop(inner);
        self.save().await;
    }

    /// 进入冷却（请求后，根据短窗口剩余或固定延时）。
    pub async fn start_cooldown(&self, idx: usize, seconds: u64) {
        let mut inner = self.inner.lock().await;
        if let Some(k) = inner.keys.get_mut(idx) {
            k.set_cooldown(seconds);
        }
        drop(inner);
        self.save().await;
    }

    /// 请求失败时标记：短窗口剩余可能已耗光，设置保守冷却。
    pub async fn on_failure(&self, idx: usize) {
        let mut inner = self.inner.lock().await;
        if let Some(k) = inner.keys.get_mut(idx) {
            // 保守：剩余清零 + 长冷却（30s 窗口重置）
            k.short_remaining = 0;
            k.set_cooldown(30);
        }
        drop(inner);
        self.save().await;
    }

    /// 全部 key 状态快照（供调试/UI）。
    pub async fn snapshot(&self) -> Vec<KeyState> {
        self.inner.lock().await.keys.clone()
    }

    /// 是否有任何可用 key。
    pub async fn any_available(&self) -> bool {
        self.inner.lock().await.keys.iter().any(|k| k.available())
    }
}

/// 当前 UTC 日期（yyyyMMdd）。
fn today_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 用简单算法：从 epoch 计算 UTC 年月日
    let days = secs / 86400;
    let rem = secs % 86400;
    let _ = rem;
    civil_from_days(days as i64)
}

/// 天数 → yyyyMMdd（Hinnant 算法逆过程）。
fn civil_from_days(z: i64) -> String {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_available_when_no_cooldown() {
        let k = KeyState::new("key1".into());
        assert!(k.available());
        assert_eq!(k.cooldown_secs(), 0);
    }

    #[test]
    fn key_paused_when_daily_quota_low() {
        let mut k = KeyState::new("key1".into());
        k.daily_paused = true;
        assert!(!k.available());
    }

    #[tokio::test]
    async fn pool_rotates_keys_round_robin() {
        let pool = ApiKeyPool::new(vec!["k1".into(), "k2".into(), "k3".into()]);
        let snap = pool.snapshot().await;
        assert_eq!(snap.len(), 3);
        assert!(snap[0].available());
        assert!(snap[1].available());
        assert!(snap[2].available());
    }

    #[tokio::test]
    async fn update_sets_quota_and_pauses_on_warning() {
        let pool = ApiKeyPool::new(vec!["k1".into()]);
        pool.update(0, Some(3), Some(8)).await; // long_remaining=8 < 10
        let snap = pool.snapshot().await;
        assert_eq!(snap[0].short_remaining, 3);
        assert_eq!(snap[0].long_remaining, 8);
        assert!(snap[0].daily_paused, "配额 <10 应当日停用");
        assert!(!snap[0].available());
    }

    #[tokio::test]
    async fn cooldown_blocks_availability() {
        let pool = ApiKeyPool::new(vec!["k1".into()]);
        pool.start_cooldown(0, 3600).await;
        let snap = pool.snapshot().await;
        assert!(!snap[0].available());
        assert!(snap[0].cooldown_secs() > 0);
    }

    #[tokio::test]
    async fn persist_roundtrip_restores_quota() {
        let dir = std::env::temp_dir().join(format!(
            "moevault_keypool_{}_{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keys = vec!["k1".to_string(), "k2".to_string()];

        // 创建 pool，更新配额 + 冷却，保存
        let pool = ApiKeyPool::new(keys.clone());
        pool.set_persist_path(dir.clone());
        pool.update(0, Some(2), Some(9)).await; // long=9 < 10 → 预警停用
        pool.start_cooldown(1, 120).await;

        // 从快照恢复（同 key 集合）
        let restored = ApiKeyPool::load_from(&dir, &keys).expect("应恢复成功");
        let snap = restored.snapshot().await;
        assert_eq!(snap[0].short_remaining, 2);
        assert_eq!(snap[0].long_remaining, 9);
        assert!(snap[0].daily_paused, "配额 <10 停用应恢复");
        assert!(!snap[0].available());
        assert!(!snap[1].available(), "key2 冷却应恢复");
        assert!(snap[1].cooldown_secs() > 0);

        // key 集合变化 → 拒绝恢复
        let different = ApiKeyPool::load_from(&dir, &["k1".to_string()]);
        assert!(different.is_none());

        let _ = std::fs::remove_file(&dir);
    }

    #[tokio::test]
    async fn failure_sets_conservative_cooldown() {
        let pool = ApiKeyPool::new(vec!["k1".into()]);
        pool.on_failure(0).await;
        let snap = pool.snapshot().await;
        assert_eq!(snap[0].short_remaining, 0);
        assert!(!snap[0].available());
    }
}
