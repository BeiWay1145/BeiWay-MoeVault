//! moevault-core：领域类型、配置、错误定义（零外部服务依赖的纯数据层）。

pub mod config;
pub mod error;
pub mod models;

pub use config::Config;
pub use error::{AppError, ErrorKind};
