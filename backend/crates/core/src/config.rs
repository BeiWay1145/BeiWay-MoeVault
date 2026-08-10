//! 运行时配置。全部支持环境变量覆盖，便于开发/部署注入。

use std::path::PathBuf;

use crate::error::{AppError, ErrorKind};

/// 主服务配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// 监听地址，默认 127.0.0.1（本地应用，不对外暴露）。
    pub host: String,
    /// 监听端口，默认 9178。
    pub port: u16,
    /// 运行时数据目录（library/ thumbs/ recycle/ 的父目录），默认 ./data。
    pub data_dir: PathBuf,
    /// SQLite 数据库文件路径。
    pub db_path: PathBuf,
    /// 前端静态资源目录（生产模式托管），可选。
    pub static_dir: Option<PathBuf>,
    /// Python 推理服务基地址。
    pub infer_base_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 9178,
            data_dir: PathBuf::from("data"),
            db_path: PathBuf::from("data/app.db"),
            static_dir: None,
            infer_base_url: "http://127.0.0.1:8001".into(),
        }
    }
}

impl Config {
    /// 从环境变量构造配置，未设置项取默认值。
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("MOEVAULT_HOST") {
            cfg.host = v;
        }
        if let Ok(v) = std::env::var("MOEVAULT_PORT") {
            if let Ok(p) = v.parse() {
                cfg.port = p;
            }
        }
        if let Ok(v) = std::env::var("MOEVAULT_DATA_DIR") {
            cfg.data_dir = PathBuf::from(v);
            cfg.db_path = cfg.data_dir.join("app.db");
        }
        if let Ok(v) = std::env::var("MOEVAULT_DB_PATH") {
            cfg.db_path = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("MOEVAULT_STATIC_DIR") {
            cfg.static_dir = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("MOEVAULT_INFER_BASE") {
            cfg.infer_base_url = v;
        }
        cfg
    }

    /// 校验配置并准备目录（data_dir 不存在则创建）。
    pub fn validate(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.data_dir).map_err(|e| {
            AppError::new(
                ErrorKind::Config,
                format!("无法创建数据目录 {}: {e}", self.data_dir.display()),
            )
        })?;
        if let Some(dir) = &self.static_dir {
            if !dir.is_dir() {
                return Err(AppError::new(
                    ErrorKind::Config,
                    format!("静态资源目录不存在: {}", dir.display()),
                ));
            }
        }
        Ok(())
    }
}
