//! tracing 层：把 WARN/ERROR 级别日志自动写入 app_logs 表（BUG 追踪器后端接入）。
//!
//! 设计：
//! - `DbLogLayer` 实现 `tracing_subscriber::Layer`，在 `on_event` 中过滤 warn/error
//! - 事件字段格式化为 message + 其他字段拼接
//! - 通过 `OnceLock<Db>` 在数据库就绪后启用（之前的日志仍只写文件）
//! - 递归保护：写入失败仅忽略，绝不打 tracing 日志（避免递归）

use std::sync::OnceLock;

use tracing::Level;
use tracing_subscriber::layer::Context as LayerContext;
use tracing_subscriber::Layer;

static DB_SLOT: OnceLock<moevault_db::Db> = OnceLock::new();

/// 数据库就绪后调用（之后 WARN/ERROR 自动进 app_logs）。
pub fn enable_db_logging(db: moevault_db::Db) {
    let _ = DB_SLOT.set(db);
}

/// 构造 BUG 追踪器日志层（挂在 fmt 层之后）。
pub fn db_log_layer() -> DbLogLayer {
    DbLogLayer
}

pub struct DbLogLayer;

impl<S> Layer<S> for DbLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
        // 仅捕获 warn/error
        let level = *event.metadata().level();
        if level != Level::WARN && level != Level::ERROR {
            return;
        }
        let db = match DB_SLOT.get() {
            Some(d) => d,
            None => return,
        };
        // 格式化事件字段
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let msg = visitor.message.trim();
        if msg.is_empty() {
            return;
        }
        let target = event.metadata().target();
        let category = target.split("::").next().unwrap_or("system").replace("moevault_", "");
        let level_str = if level == Level::ERROR { "error" } else { "warn" };
        let mut line = format!("[{target}] {msg}");
        if !visitor.others.is_empty() {
            line.push_str(&format!(" {}", visitor.others.join(" ")));
        }
        // 静默失败：绝不递归打日志
        let _ = db.add_log(level_str, &category, &line);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    others: Vec<String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            _ => self.others.push(format!("{}={:?}", field.name(), value)),
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            _ => self.others.push(format!("{}={}", field.name(), value)),
        }
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.others.push(format!("{}={}", field.name(), value));
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.others.push(format!("{}={}", field.name(), value));
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.others.push(format!("{}={}", field.name(), value));
    }
}
