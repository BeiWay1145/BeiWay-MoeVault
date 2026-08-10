//! 统一错误类型。

use std::fmt;

/// 错误分类，便于上层映射为 HTTP 状态码或日志级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Db,
    NotFound,
    InvalidInput,
    External,
    Internal,
}

impl ErrorKind {
    /// 映射到 HTTP 状态码（api 层使用）。
    pub fn status_code(&self) -> u16 {
        match self {
            ErrorKind::NotFound => 404,
            ErrorKind::InvalidInput => 422,
            ErrorKind::Config | ErrorKind::Internal => 500,
            ErrorKind::Db | ErrorKind::External => 502,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::Config => "config",
            ErrorKind::Db => "db",
            ErrorKind::NotFound => "not_found",
            ErrorKind::InvalidInput => "invalid_input",
            ErrorKind::External => "external",
            ErrorKind::Internal => "internal",
        }
    }
}

/// 应用统一错误。
#[derive(Debug, Clone)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}
