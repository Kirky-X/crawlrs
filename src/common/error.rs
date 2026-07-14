// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一错误类型定义
//!
//! 提供应用程序的统一错误类型，用于处理所有应用级别的错误

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use log::error;
use regex::Regex;
use serde::Serialize;

/// 应用程序错误类型
///
/// 统一所有应用级别的错误，提供清晰的错误分类和上下文信息
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    /// 网络错误
    #[error("Network error: {0}")]
    Network(String),

    /// 配置错误
    #[error("Configuration error: {0}")]
    Config(String),

    /// 验证错误
    #[error("Validation error: {0}")]
    Validation(String),

    /// 资源未找到
    #[error("Not found: {0}")]
    NotFound(String),

    /// 权限拒绝
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// 超时错误
    #[error("Timeout: {0}")]
    Timeout(String),

    /// IO 错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 错误
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// 引擎错误
    #[error("Engine error: {0}")]
    Engine(String),

    /// 缓存错误
    #[error("Cache error: {0}")]
    Cache(String),

    /// 任务错误
    #[error("Task error: {0}")]
    Task(String),

    /// 速率限制错误
    #[error("Rate limit error: {0}")]
    RateLimit(String),

    /// 其他错误
    #[error("Error: {0}")]
    Other(String),
}

impl AppError {
    /// 获取错误的 HTTP 状态码
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Network(_) => StatusCode::BAD_GATEWAY,
            AppError::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::PermissionDenied(_) => StatusCode::FORBIDDEN,
            AppError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Json(_) => StatusCode::BAD_REQUEST,
            AppError::Engine(_) => StatusCode::BAD_GATEWAY,
            AppError::Cache(_) => StatusCode::SERVICE_UNAVAILABLE,
            AppError::Task(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 获取错误的代码
    pub fn error_code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "DATABASE_ERROR",
            AppError::Network(_) => "EXTERNAL_SERVICE_ERROR",
            AppError::Config(_) => "CONFIGURATION_ERROR",
            AppError::Validation(_) => "VALIDATION_ERROR",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::PermissionDenied(_) => "FORBIDDEN",
            AppError::Timeout(_) => "TIMEOUT",
            AppError::Io(_) => "IO_ERROR",
            AppError::Json(_) => "JSON_ERROR",
            AppError::Engine(_) => "ENGINE_ERROR",
            AppError::Cache(_) => "CACHE_ERROR",
            AppError::Task(_) => "TASK_ERROR",
            AppError::RateLimit(_) => "RATE_LIMITED",
            AppError::Other(_) => "INTERNAL_ERROR",
        }
    }

    /// 获取用户可见的错误消息（脱敏后）
    ///
    /// 此方法返回适合展示给最终用户的错误消息，不包含敏感的内部实现细节。
    /// 用于生产环境中的错误响应。
    pub fn user_message(&self) -> String {
        match self {
            AppError::Database(_) => {
                "Database operation failed. Please try again later.".to_string()
            }
            AppError::Network(_) => {
                "External service unavailable. Please try again later.".to_string()
            }
            AppError::Config(_) => "Configuration error. Please contact support.".to_string(),
            AppError::Validation(msg) => format!("Validation error: {}", msg),
            AppError::NotFound(msg) => format!("Resource not found: {}", msg),
            AppError::PermissionDenied(msg) => format!("Permission denied: {}", msg),
            AppError::Timeout(_) => "Request timed out. Please try again later.".to_string(),
            AppError::Io(_) => "Internal I/O error. Please try again later.".to_string(),
            AppError::Json(_) => "Invalid JSON format. Please check your request.".to_string(),
            AppError::Engine(e) => sanitize_engine_error(e),
            AppError::Cache(_) => "Cache service unavailable. Please try again later.".to_string(),
            AppError::Task(_) => "Task processing error. Please try again later.".to_string(),
            AppError::RateLimit(_) => {
                "Rate limit exceeded. Please slow down your requests.".to_string()
            }
            AppError::Other(_) => "Internal server error. Please try again later.".to_string(),
        }
    }

    /// 获取详细错误信息（用于日志）
    ///
    /// 此方法返回完整的错误详情，包含所有内部实现细节。
    /// 仅用于服务器端日志记录，不应暴露给客户端。
    pub fn detailed_message(&self) -> String {
        self.to_string()
    }

    /// 转换为 API 错误响应格式
    pub fn to_api_error_response(&self) -> ApiErrorResponse {
        ApiErrorResponse {
            code: self.error_code().to_string(),
            message: self.to_string(),
        }
    }
}

/// API 错误响应结构
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    /// 错误代码
    pub code: String,
    /// 错误消息
    pub message: String,
}

/// 应用程序结果类型
///
/// 使用统一的错误类型作为 Result 的错误变体
pub type AppResult<T> = Result<T, AppError>;

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        error.to_api_error_response()
    }
}

// =============================================================================
// 错误信息脱敏函数
// =============================================================================

/// 检查是否应该显示详细错误信息
///
/// 在开发环境中显示详细错误信息便于调试，在生产环境中隐藏敏感信息。
/// 默认情况下隐藏详细错误信息（安全优先）。
fn should_show_detailed_errors() -> bool {
    // 检查多个环境变量以支持不同的配置方式
    let env = std::env::var("CRAWLRS_ENV")
        .or_else(|_| std::env::var("APP_ENVIRONMENT"))
        .or_else(|_| std::env::var("RUST_ENV"))
        .unwrap_or_else(|_| "production".to_string());

    // 只有明确设置为开发环境时才显示详细错误
    env.eq_ignore_ascii_case("development")
        || env.eq_ignore_ascii_case("dev")
        || env.eq_ignore_ascii_case("local")
}

/// 脱敏引擎错误信息
///
/// 移除引擎错误中的敏感信息，包括：
/// - 文件路径
/// - IP 地址
/// - 端口号
/// - URL 中的敏感参数
fn sanitize_engine_error(error: &str) -> String {
    let mut sanitized = error.to_string();

    // 移除文件路径 (如 /home/dev/crawlrs/src/..., /app/...)
    let file_path_pattern = Regex::new(r"/[a-zA-Z0-9_/.-]+\.(rs|toml|env|yml|json|txt)")
        .expect("Invalid regex pattern for file paths");
    sanitized = file_path_pattern
        .replace_all(&sanitized, "[FILE_PATH]")
        .to_string();

    // 移除行号信息 (如 "at line 42", "src/file.rs:42")
    let line_number_pattern =
        Regex::new(r"[a-zA-Z0-9_/.-]+\.rs:\d+").expect("Invalid regex pattern for line numbers");
    sanitized = line_number_pattern
        .replace_all(&sanitized, "[LOCATION]")
        .to_string();

    // 移除内部 IP 地址（私有 IP 段）
    let internal_ip_pattern = Regex::new(
        r"\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3})\b"
    ).expect("Invalid regex pattern for internal IPs");
    sanitized = internal_ip_pattern
        .replace_all(&sanitized, "[INTERNAL_IP]")
        .to_string();

    // 移除 localhost 和 127.0.0.1 后的端口号
    let internal_service_pattern = Regex::new(r"(localhost|127\.0\.0\.1):\d+")
        .expect("Invalid regex pattern for internal services");
    sanitized = internal_service_pattern
        .replace_all(&sanitized, "$1:[PORT]")
        .to_string();

    // 移除公网 IP 地址（保留格式但不暴露具体 IP）
    // 排除 127.x.x.x（loopback 已由 internal_service_pattern 处理端口脱敏）
    // Rust regex crate 不支持 look-ahead，用 alternation 排除首段为 127 的情况：
    //   首段匹配 0-126 或 128-255（[0-9]{1,2}=0-99, 1[01][0-9]=100-119,
    //   12[0-689]=120-126/128/129, 1[3-9][0-9]=130-199, 2[0-4][0-9]=200-249, 25[0-5]=250-255）
    let public_ip_pattern = Regex::new(r"\b(?:[0-9]{1,2}|1[01][0-9]|12[0-689]|1[3-9][0-9]|2[0-4][0-9]|25[0-5])\.(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){2}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b")
        .expect("Invalid regex pattern for public IPs");
    sanitized = public_ip_pattern
        .replace_all(&sanitized, "[IP]")
        .to_string();

    // 移除 URL 中的查询参数（可能包含敏感信息）
    let url_params_pattern =
        Regex::new(r"\?[^#\s]+").expect("Invalid regex pattern for URL parameters");
    sanitized = url_params_pattern
        .replace_all(&sanitized, "?[PARAMS_REDACTED]")
        .to_string();

    // 移除 URL 中的用户名和密码
    let url_credentials_pattern =
        Regex::new(r"(https?://)[^:]+:[^@]+@").expect("Invalid regex pattern for URL credentials");
    sanitized = url_credentials_pattern
        .replace_all(&sanitized, "$1[CREDENTIALS_REDACTED]@")
        .to_string();

    // 移除数据库连接字符串中的密码
    let db_password_pattern = Regex::new(r"(postgres|mysql|mongodb|redis)://[^:]+:[^@]+@")
        .expect("Invalid regex pattern for database passwords");
    sanitized = db_password_pattern
        .replace_all(&sanitized, "$1://[USER]:[PASSWORD]@")
        .to_string();

    // 移除 API 密钥模式（如 api_key=xxx, token=xxx）
    let api_key_pattern = Regex::new(r"(?i)(api[_-]?key|token|secret|password|auth)[=:\s]+[^\s&]+")
        .expect("Invalid regex pattern for API keys");
    sanitized = api_key_pattern
        .replace_all(&sanitized, "$1=[REDACTED]")
        .to_string();

    // 如果脱敏后消息为空或太短，返回通用错误消息
    if sanitized.trim().is_empty() || sanitized.len() < 5 {
        return "Engine error occurred".to_string();
    }

    sanitized
}

/// 脱敏通用错误信息
///
/// 对所有错误消息进行脱敏处理，移除敏感信息
#[allow(dead_code)]
fn sanitize_error_message(msg: &str) -> String {
    let mut sanitized = msg.to_string();

    // 移除表名和列名模式 (如 "table: users", "column: email")
    let table_column_pattern = Regex::new(r#"(?i)(table|column|field):\s*[\w."']+"#)
        .expect("Invalid regex pattern for table/column names");
    sanitized = table_column_pattern
        .replace_all(&sanitized, "[REDACTED]")
        .to_string();

    // 移除 SQL 查询片段
    let sql_pattern = Regex::new(r#"(?i)(SQL|query|statement):\s*[^,\]\}]+"#)
        .expect("Invalid regex pattern for SQL queries");
    sanitized = sql_pattern
        .replace_all(&sanitized, "[SQL_REDACTED]")
        .to_string();

    // 移除文件路径
    let file_path_pattern = Regex::new(r"/[a-zA-Z0-9_/.-]+\.(rs|toml|env|yml|json)")
        .expect("Invalid regex pattern for file paths");
    sanitized = file_path_pattern
        .replace_all(&sanitized, "[FILE_PATH_REDACTED]")
        .to_string();

    // 移除内部 IP 地址
    let internal_ip_pattern = Regex::new(
        r"\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3})\b"
    ).expect("Invalid regex pattern for internal IPs");
    sanitized = internal_ip_pattern
        .replace_all(&sanitized, "[INTERNAL_IP_REDACTED]")
        .to_string();

    // 移除数据库连接字符串中的密码
    let db_password_pattern = Regex::new(r"(postgres|mysql|mongodb)://[^:]+:[^@]+@")
        .expect("Invalid regex pattern for database passwords");
    sanitized = db_password_pattern
        .replace_all(&sanitized, "$1://[USER]:[PASSWORD]@")
        .to_string();

    sanitized
}

// =============================================================================
// IntoResponse 实现
// =============================================================================

/// 为 AppError 实现 IntoResponse trait
///
/// 根据环境自动选择返回详细错误信息或脱敏后的错误信息：
/// - 开发环境：返回详细错误信息，便于调试
/// - 生产环境：返回脱敏后的用户友好错误信息，详细错误记录到日志
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_code = self.error_code();
        let detailed_msg = self.detailed_message();

        // 记录详细错误到服务器日志
        error!(
            "Request error occurred error_code={:?} status_code={} error_details={}",
            error_code,
            status.as_u16(),
            detailed_msg
        );

        // 根据环境决定返回给客户端的错误信息
        let error_response = if should_show_detailed_errors() {
            // 开发环境：返回详细错误信息
            serde_json::json!({
                "success": false,
                "error": {
                    "code": error_code,
                    "message": detailed_msg,
                    "status": status.as_u16(),
                }
            })
        } else {
            // 生产环境：返回脱敏后的用户友好错误信息
            let user_msg = self.user_message();
            serde_json::json!({
                "success": false,
                "error": {
                    "code": error_code,
                    "message": user_msg,
                    "status": status.as_u16(),
                }
            })
        };

        (status, Json(error_response)).into_response()
    }
}

/// 从 reqwest::Error 转换为 AppError::Network
///
/// 保留 `?` 操作符对 reqwest 错误的自动转换能力。
/// 变体 `Network(String)` 不再使用 `#[from]`，因此需要手动实现。
impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Network(err.to_string())
    }
}

/// 从 EngineError 转换为 AppError
///
/// 统一引擎层的错误到应用层错误处理
impl From<crate::engines::engine_client::EngineError> for AppError {
    fn from(err: crate::engines::engine_client::EngineError) -> Self {
        match err {
            crate::engines::engine_client::EngineError::RequestFailed(msg) => AppError::Engine(msg),
            crate::engines::engine_client::EngineError::Timeout(duration) => {
                AppError::Timeout(format!("Request timed out after {:?}", duration))
            }
            crate::engines::engine_client::EngineError::NoEnginesAvailable => {
                AppError::Engine("No scraping engines available".to_string())
            }
            crate::engines::engine_client::EngineError::InvalidUrl(msg) => {
                AppError::Validation(msg)
            }
            crate::engines::engine_client::EngineError::SsrfProtection(msg) => {
                AppError::PermissionDenied(format!("SSRF protection triggered: {}", msg))
            }
            crate::engines::engine_client::EngineError::BrowserError(msg) => {
                AppError::Engine(format!("Browser error: {}", msg))
            }
            crate::engines::engine_client::EngineError::Expired => {
                AppError::Timeout("Request expired".to_string())
            }
            crate::engines::engine_client::EngineError::AllEnginesFailed(msg) => {
                AppError::Engine(format!("All engines failed: {}", msg))
            }
            crate::engines::engine_client::EngineError::Other(msg) => AppError::Engine(msg),
            crate::engines::engine_client::EngineError::Internal(msg) => AppError::Engine(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::ENV_MUTEX;
    use axum::http::StatusCode;
    use std::time::Duration;

    #[test]
    fn test_error_display() {
        let err = AppError::NotFound("User".to_string());
        assert_eq!(err.to_string(), "Not found: User");
    }

    #[test]
    fn test_error_debug() {
        let err = AppError::Validation("Invalid email".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Validation"));
    }

    #[test]
    fn test_app_result() {
        let result: AppResult<i32> = Ok(42);
        assert!(result.is_ok());

        let result: AppResult<i32> = Err(AppError::Config("Missing key".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_status_code_mapping() {
        assert_eq!(
            AppError::NotFound("test".to_string()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AppError::Validation("test".to_string()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::PermissionDenied("test".to_string()).status_code(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            AppError::RateLimit("test".to_string()).status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn test_error_code_mapping() {
        // Use a Custom error instead of the specific Conn variant
        let db_err = sea_orm::DbErr::Custom("test connection error".to_string());
        assert_eq!(AppError::Database(db_err).error_code(), "DATABASE_ERROR");
        assert_eq!(
            AppError::NotFound("test".to_string()).error_code(),
            "NOT_FOUND"
        );
        assert_eq!(
            AppError::Validation("test".to_string()).error_code(),
            "VALIDATION_ERROR"
        );
    }

    #[test]
    fn test_to_api_error_response() {
        let err = AppError::NotFound("User not found".to_string());
        let response = err.to_api_error_response();
        assert_eq!(response.code, "NOT_FOUND");
        assert_eq!(response.message, "Not found: User not found");
    }

    // =============================================================================
    // 错误信息脱敏测试
    // =============================================================================

    #[test]
    fn test_user_message_sanitization() {
        // 数据库错误应该返回通用消息
        let db_err = AppError::Database(sea_orm::DbErr::Custom(
            "Connection failed to postgres://user:password123@localhost:5432".to_string(),
        ));
        let user_msg = db_err.user_message();
        assert!(!user_msg.contains("password123"));
        assert!(!user_msg.contains("localhost:5432"));
        assert!(user_msg.contains("Database operation failed"));

        // 网络错误应该返回通用消息
        let net_err = crate::utils::error_helpers::map_to_network_error(
            "Connection refused to 10.0.0.1:8080",
        );
        let user_msg = net_err.user_message();
        assert!(!user_msg.contains("10.0.0.1"));
        assert!(user_msg.contains("External service unavailable"));

        // 配置错误应该返回通用消息
        let config_err =
            AppError::Config("Missing database URL in /home/dev/crawlrs/.env".to_string());
        let user_msg = config_err.user_message();
        assert!(!user_msg.contains("/home/dev/"));
        assert!(user_msg.contains("Configuration error"));

        // 验证错误应该保留用户输入的错误信息
        let validation_err = AppError::Validation("Email format is invalid".to_string());
        let user_msg = validation_err.user_message();
        assert!(user_msg.contains("Email format is invalid"));
    }

    #[test]
    fn test_detailed_message_preserves_info() {
        let err = AppError::Database(sea_orm::DbErr::Custom(
            "Connection failed to postgres://user:password123@localhost:5432".to_string(),
        ));
        let detailed = err.detailed_message();
        // 详细消息应该包含所有信息
        assert!(detailed.contains("postgres://"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_file_paths() {
        let error = "Failed to load config from /home/dev/crawlrs/src/config.rs:42";
        let sanitized = sanitize_engine_error(error);
        assert!(!sanitized.contains("/home/dev/crawlrs/"));
        assert!(sanitized.contains("[FILE_PATH]") || sanitized.contains("[LOCATION]"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_internal_ips() {
        let error = "Connection failed to 10.0.0.1:5432 and 192.168.1.100:8080";
        let sanitized = sanitize_engine_error(error);
        assert!(!sanitized.contains("10.0.0.1"));
        assert!(!sanitized.contains("192.168.1.100"));
        assert!(sanitized.contains("[INTERNAL_IP]"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_ports() {
        let error = "Service at localhost:8080 and 127.0.0.1:5432 failed";
        let sanitized = sanitize_engine_error(error);
        assert!(sanitized.contains("localhost:[PORT]"));
        assert!(sanitized.contains("127.0.0.1:[PORT]"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_credentials() {
        let error = "Failed to connect to postgres://admin:secret123@db.example.com:5432/mydb";
        let sanitized = sanitize_engine_error(error);
        assert!(!sanitized.contains("admin:secret123"));
        assert!(sanitized.contains("[USER]:[PASSWORD]"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_api_keys() {
        let error = "Request failed with api_key=sk-1234567890abcdef and token=xyz789";
        let sanitized = sanitize_engine_error(error);
        assert!(!sanitized.contains("sk-1234567890abcdef"));
        assert!(!sanitized.contains("xyz789"));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitize_engine_error_removes_url_params() {
        let error = "Failed to fetch https://api.example.com/data?secret=abc123&token=xyz";
        let sanitized = sanitize_engine_error(error);
        assert!(!sanitized.contains("secret=abc123"));
        assert!(!sanitized.contains("token=xyz"));
        assert!(sanitized.contains("[PARAMS_REDACTED]"));
    }

    #[test]
    fn test_sanitize_error_message_removes_table_names() {
        let msg = "Database error: table: users column: email not found";
        let sanitized = sanitize_error_message(msg);
        assert!(sanitized.contains("[REDACTED]"));
        assert!(!sanitized.contains("users"));
        assert!(!sanitized.contains("email"));
    }

    #[test]
    fn test_sanitize_error_message_removes_sql_queries() {
        let msg = "Error: SQL: SELECT * FROM users WHERE password = 'secret'";
        let sanitized = sanitize_error_message(msg);
        assert!(sanitized.contains("[SQL_REDACTED]"));
        assert!(!sanitized.contains("SELECT"));
        assert!(!sanitized.contains("password"));
    }

    #[test]
    fn test_sanitize_error_message_removes_db_passwords() {
        let msg = "Connection failed: postgres://user:password123@localhost:5432";
        let sanitized = sanitize_error_message(msg);
        assert!(!sanitized.contains("password123"));
        assert!(sanitized.contains("[PASSWORD]"));
    }

    #[test]
    fn test_sanitize_error_message_preserves_safe_content() {
        let msg = "Invalid input: email format is incorrect";
        let sanitized = sanitize_error_message(msg);
        assert!(sanitized.contains("Invalid input"));
        assert!(sanitized.contains("email format"));
    }

    #[test]
    fn test_should_show_detailed_errors_in_dev() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("CRAWLRS_ENV", "development");
        assert!(should_show_detailed_errors());
        std::env::remove_var("CRAWLRS_ENV");
    }

    #[test]
    fn test_should_show_detailed_errors_in_local() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("APP_ENVIRONMENT", "local");
        assert!(should_show_detailed_errors());
        std::env::remove_var("APP_ENVIRONMENT");
    }

    #[test]
    fn test_should_hide_detailed_errors_in_prod() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("CRAWLRS_ENV", "production");
        assert!(!should_show_detailed_errors());
        std::env::remove_var("CRAWLRS_ENV");
    }

    #[test]
    fn test_should_hide_detailed_errors_by_default() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // 默认应该是生产环境模式（安全优先）
        std::env::remove_var("CRAWLRS_ENV");
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("RUST_ENV");
        assert!(!should_show_detailed_errors());
    }

    #[test]
    fn test_sanitize_engine_error_empty_input() {
        let msg = "";
        let sanitized = sanitize_engine_error(msg);
        assert_eq!(sanitized, "Engine error occurred");
    }

    #[test]
    fn test_sanitize_engine_error_very_short_input() {
        let msg = "abc";
        let sanitized = sanitize_engine_error(msg);
        assert_eq!(sanitized, "Engine error occurred");
    }

    #[test]
    fn test_sanitize_engine_error_multiple_patterns() {
        let error =
            "Error at /home/dev/crawlrs/src/main.rs:42 with IP 10.0.0.1 and api_key=secret123";
        let sanitized = sanitize_engine_error(error);
        assert!(sanitized.contains("[FILE_PATH]") || sanitized.contains("[LOCATION]"));
        assert!(sanitized.contains("[INTERNAL_IP]"));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitize_error_message_empty_input() {
        let msg = "";
        let sanitized = sanitize_error_message(msg);
        assert_eq!(sanitized, "");
    }

    #[test]
    fn test_sanitize_error_message_very_long_input() {
        let msg = "A".repeat(10000);
        let sanitized = sanitize_error_message(&msg);
        // 应该仍然处理长输入
        assert!(sanitized.contains('A'));
    }

    // =============================================================================
    // 完整的状态码映射测试 - 覆盖所有变体
    // =============================================================================

    #[test]
    fn test_status_code_all_variants() {
        // 测试所有错误变体的状态码映射
        assert_eq!(
            AppError::Network("conn refused".to_string()).status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            AppError::Config("missing key".to_string()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::Timeout("timed out".to_string()).status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "io fail")).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::Json(serde_json::from_str::<serde_json::Value>("bad").unwrap_err())
                .status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::Engine("engine fail".to_string()).status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            AppError::Cache("cache down".to_string()).status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            AppError::Task("task error".to_string()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::Other("unknown".to_string()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        // Database also maps to INTERNAL_SERVER_ERROR
        assert_eq!(
            AppError::Database(sea_orm::DbErr::Custom("db down".to_string())).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    // =============================================================================
    // 完整的错误代码映射测试 - 覆盖所有变体
    // =============================================================================

    #[test]
    fn test_error_code_all_variants() {
        assert_eq!(
            AppError::Network("test".to_string()).error_code(),
            "EXTERNAL_SERVICE_ERROR"
        );
        assert_eq!(
            AppError::Config("test".to_string()).error_code(),
            "CONFIGURATION_ERROR"
        );
        assert_eq!(
            AppError::PermissionDenied("test".to_string()).error_code(),
            "FORBIDDEN"
        );
        assert_eq!(
            AppError::Timeout("test".to_string()).error_code(),
            "TIMEOUT"
        );
        assert_eq!(
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")).error_code(),
            "IO_ERROR"
        );
        assert_eq!(
            AppError::Json(serde_json::from_str::<serde_json::Value>("x").unwrap_err())
                .error_code(),
            "JSON_ERROR"
        );
        assert_eq!(
            AppError::Engine("test".to_string()).error_code(),
            "ENGINE_ERROR"
        );
        assert_eq!(
            AppError::Cache("test".to_string()).error_code(),
            "CACHE_ERROR"
        );
        assert_eq!(
            AppError::Task("test".to_string()).error_code(),
            "TASK_ERROR"
        );
        assert_eq!(
            AppError::Other("test".to_string()).error_code(),
            "INTERNAL_ERROR"
        );
        assert_eq!(
            AppError::RateLimit("test".to_string()).error_code(),
            "RATE_LIMITED"
        );
    }

    // =============================================================================
    // 完整的 user_message 测试 - 覆盖所有变体
    // =============================================================================

    #[test]
    fn test_user_message_all_variants() {
        // 数据库、网络、配置等通用错误返回固定消息
        assert_eq!(
            AppError::Network("http://10.0.0.1".to_string()).user_message(),
            "External service unavailable. Please try again later."
        );
        assert_eq!(
            AppError::Config("/etc/crawlrs/config.toml".to_string()).user_message(),
            "Configuration error. Please contact support."
        );
        assert_eq!(
            AppError::Timeout("30s".to_string()).user_message(),
            "Request timed out. Please try again later."
        );
        assert_eq!(
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")).user_message(),
            "Internal I/O error. Please try again later."
        );
        assert_eq!(
            AppError::Json(serde_json::from_str::<serde_json::Value>("x").unwrap_err())
                .user_message(),
            "Invalid JSON format. Please check your request."
        );
        assert_eq!(
            AppError::Cache("redis down".to_string()).user_message(),
            "Cache service unavailable. Please try again later."
        );
        assert_eq!(
            AppError::Task("task failed".to_string()).user_message(),
            "Task processing error. Please try again later."
        );
        assert_eq!(
            AppError::RateLimit("too fast".to_string()).user_message(),
            "Rate limit exceeded. Please slow down your requests."
        );
        assert_eq!(
            AppError::Other("unknown".to_string()).user_message(),
            "Internal server error. Please try again later."
        );

        // 携带上下文的错误保留原始信息
        let not_found = AppError::NotFound("user 42".to_string());
        assert!(not_found.user_message().contains("user 42"));

        let perm = AppError::PermissionDenied("insufficient scope".to_string());
        assert!(perm.user_message().contains("insufficient scope"));

        // 引擎错误经过脱敏处理
        let engine_err =
            AppError::Engine("Failed at /home/dev/src/main.rs:42 with api_key=secret".to_string());
        let msg = engine_err.user_message();
        assert!(!msg.contains("secret"));
        assert!(!msg.contains("/home/dev/"));
    }

    // =============================================================================
    // From<AppError> for ApiErrorResponse 测试
    // =============================================================================

    #[test]
    fn test_from_app_error_to_api_error_response() {
        let err = AppError::Validation("field required".to_string());
        let response: ApiErrorResponse = err.into();
        assert_eq!(response.code, "VALIDATION_ERROR");
        assert_eq!(response.message, "Validation error: field required");
    }

    #[test]
    fn test_from_app_error_network_to_api_error_response() {
        let err = AppError::Network("timeout".to_string());
        let response: ApiErrorResponse = err.into();
        assert_eq!(response.code, "EXTERNAL_SERVICE_ERROR");
        assert!(response.message.contains("timeout"));
    }

    // =============================================================================
    // IntoResponse 测试 - 生产环境和开发环境
    // =============================================================================

    #[tokio::test]
    async fn test_into_response_production_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("CRAWLRS_ENV", "production");
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("RUST_ENV");

        let err = AppError::NotFound("resource 123".to_string());
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert_eq!(json["error"]["code"], "NOT_FOUND");
        assert_eq!(json["error"]["status"], 404);
        // 生产环境返回脱敏后的用户消息
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Resource not found"));
    }

    #[tokio::test]
    async fn test_into_response_development_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("CRAWLRS_ENV", "development");

        let err = AppError::Validation("bad input".to_string());
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
        assert_eq!(json["error"]["status"], 400);
        // 开发环境返回详细错误信息
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("bad input"));
    }

    #[tokio::test]
    async fn test_into_response_rate_limit() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("CRAWLRS_ENV", "production");
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("RUST_ENV");

        let err = AppError::RateLimit("too many requests".to_string());
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "RATE_LIMITED");
        assert_eq!(json["error"]["status"], 429);
    }

    // =============================================================================
    // From<EngineError> 测试 - 覆盖所有 EngineError 变体
    // =============================================================================

    #[test]
    fn test_from_engine_error_request_failed() {
        let err: AppError =
            crate::engines::engine_client::EngineError::RequestFailed("conn refused".to_string())
                .into();
        match err {
            AppError::Engine(msg) => assert!(msg.contains("conn refused")),
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_timeout() {
        let err: AppError =
            crate::engines::engine_client::EngineError::Timeout(Duration::from_secs(30)).into();
        match err {
            AppError::Timeout(msg) => assert!(msg.contains("30")),
            other => panic!("expected Timeout variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_no_engines_available() {
        let err: AppError = crate::engines::engine_client::EngineError::NoEnginesAvailable.into();
        match err {
            AppError::Engine(msg) => assert!(msg.contains("No scraping engines available")),
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_invalid_url() {
        let err: AppError =
            crate::engines::engine_client::EngineError::InvalidUrl("bad url".to_string()).into();
        match err {
            AppError::Validation(msg) => assert_eq!(msg, "bad url"),
            other => panic!("expected Validation variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_ssrf_protection() {
        let err: AppError =
            crate::engines::engine_client::EngineError::SsrfProtection("127.0.0.1".to_string())
                .into();
        match err {
            AppError::PermissionDenied(msg) => {
                assert!(msg.contains("SSRF protection"));
                assert!(msg.contains("127.0.0.1"));
            }
            other => panic!("expected PermissionDenied variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_browser_error() {
        let err: AppError =
            crate::engines::engine_client::EngineError::BrowserError("crashed".to_string()).into();
        match err {
            AppError::Engine(msg) => {
                assert!(msg.contains("Browser error"));
                assert!(msg.contains("crashed"));
            }
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_expired() {
        let err: AppError = crate::engines::engine_client::EngineError::Expired.into();
        match err {
            AppError::Timeout(msg) => assert_eq!(msg, "Request expired"),
            other => panic!("expected Timeout variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_all_engines_failed() {
        let err: AppError =
            crate::engines::engine_client::EngineError::AllEnginesFailed("all down".to_string())
                .into();
        match err {
            AppError::Engine(msg) => {
                assert!(msg.contains("All engines failed"));
                assert!(msg.contains("all down"));
            }
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_other() {
        let err: AppError =
            crate::engines::engine_client::EngineError::Other("misc".to_string()).into();
        match err {
            AppError::Engine(msg) => assert_eq!(msg, "misc"),
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_engine_error_internal() {
        let err: AppError =
            crate::engines::engine_client::EngineError::Internal("internal bug".to_string()).into();
        match err {
            AppError::Engine(msg) => assert_eq!(msg, "internal bug"),
            other => panic!("expected Engine variant, got {:?}", other),
        }
    }

    // =============================================================================
    // From<reqwest::Error> 测试
    // =============================================================================

    #[test]
    fn test_from_reqwest_error_conversion() {
        // reqwest 0.13 + rustls 后端下 Certificate::from_pem 不再立即验证 PEM，
        // 改用无效代理 URL 触发 reqwest::Error。
        let result = reqwest::Proxy::all("not a valid url");
        assert!(result.is_err(), "Proxy::all should reject invalid URL");
        let reqwest_err = result.unwrap_err();
        let app_err: AppError = reqwest_err.into();
        match app_err {
            AppError::Network(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Network variant, got {:?}", other),
        }
    }
}
