// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use regex::Regex;
use thiserror::Error;

/// Macro to generate standard error conversions for basic types
///
/// # Usage
///
/// ```rust
/// impl_basic_error_conversions!(MyError, InternalVariant);
/// ```
///
/// This generates:
/// - `impl From<String> for MyError`
/// - `impl From<&str> for MyError`
/// - `impl From<anyhow::Error> for MyError`
#[macro_export]
macro_rules! impl_basic_error_conversions {
    ($error_type:ty, $variant:ident) => {
        impl From<String> for $error_type {
            fn from(msg: String) -> Self {
                Self::$variant(msg)
            }
        }

        impl From<&str> for $error_type {
            fn from(msg: &str) -> Self {
                Self::$variant(msg.to_string())
            }
        }

        impl From<anyhow::Error> for $error_type {
            fn from(err: anyhow::Error) -> Self {
                Self::$variant(err.to_string())
            }
        }
    };
}

/// 错误消息脱敏函数
///
/// 移除错误消息中的敏感信息，包括：
/// - 数据库表名和列名
/// - 文件路径
/// - SQL 查询片段
/// - 内部服务地址
pub fn sanitize_error_message(msg: &str) -> String {
    let mut msg = msg.to_string();

    // 移除表名和列名模式 (如 "table: users", "column: email")
    let table_column_pattern =
        Regex::new(r#"(?i)(table|column|field):\s*[\w."']+"#).expect("Invalid regex pattern");
    msg = table_column_pattern
        .replace_all(&msg, "[REDACTED]")
        .to_string();

    // 移除 SQL 查询片段
    let sql_pattern =
        Regex::new(r#"(?i)(SQL|query|statement):\s*[^,\]}]+"#).expect("Invalid regex pattern");
    msg = sql_pattern.replace_all(&msg, "[SQL_REDACTED]").to_string();

    // 移除文件路径 (如 /home/dev/crawlrs/src/..., /app/...)
    let file_path_pattern =
        Regex::new(r#"/[a-zA-Z0-9_/.-]+\.(rs|toml|env|yml|json)"#).expect("Invalid regex pattern");
    msg = file_path_pattern
        .replace_all(&msg, "[FILE_PATH_REDACTED]")
        .to_string();

    // 移除行号信息 (如 "at line 42", "src/file.rs:42")
    let line_number_pattern =
        Regex::new(r#"[a-zA-Z0-9_/.-]+\.rs:\d+"#).expect("Invalid regex pattern");
    msg = line_number_pattern
        .replace_all(&msg, "[LOCATION_REDACTED]")
        .to_string();

    // 移除内部 IP 地址
    let internal_ip_pattern = Regex::new(r#"\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3})\b"#).expect("Invalid regex pattern");
    msg = internal_ip_pattern
        .replace_all(&msg, "[INTERNAL_IP_REDACTED]")
        .to_string();

    // 移除端口号（如果是内部服务）
    let internal_service_pattern =
        Regex::new(r#"(localhost|127\.0\.0\.1):\d+"#).expect("Invalid regex pattern");
    msg = internal_service_pattern
        .replace_all(&msg, "$1:[PORT_REDACTED]")
        .to_string();

    // 移除数据库连接字符串中的密码
    let db_password_pattern =
        Regex::new(r#"(postgres|mysql|mongodb)://[^:]+:[^@]+@"#).expect("Invalid regex pattern");
    msg = db_password_pattern
        .replace_all(&msg, "$1://[USER]:[PASSWORD]@")
        .to_string();

    msg
}

/// 检查是否应该显示详细错误信息
fn should_show_detailed_errors() -> bool {
    // 开发环境显示详细错误，生产环境隐藏
    std::env::var("CRAWLRS_ENV")
        .map(|v| v.eq_ignore_ascii_case("development") || v.eq_ignore_ascii_case("dev"))
        .unwrap_or(false)
}

/// 仓库层错误类型
#[derive(Error, Debug)]
pub enum RepositoryError {
    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("未找到数据")]
    NotFound,

    #[error("数据已存在")]
    AlreadyExists,

    #[error("无效参数: {0}")]
    InvalidParameter(String),

    #[error("内部错误: {0}")]
    InternalError(String),
}

/// Worker错误类型
#[derive(Error, Debug)]
pub enum WorkerError {
    #[error("仓库错误: {0}")]
    RepositoryError(String),

    #[error("限流错误: {0}")]
    RateLimitingError(String),

    #[error("内部错误: {0}")]
    InternalError(String),

    #[error("领域错误: {0}")]
    DomainError(String),

    #[error("服务错误: {0}")]
    ServiceError(String),

    #[error("未找到: {0}")]
    NotFound(String),
}

// From implementations for WorkerError using macro
impl_basic_error_conversions!(WorkerError, InternalError);

impl From<RepositoryError> for WorkerError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::DatabaseError(msg) => WorkerError::RepositoryError(msg),
            RepositoryError::NotFound => WorkerError::NotFound("Resource not found".to_string()),
            RepositoryError::AlreadyExists => {
                WorkerError::RepositoryError("Resource already exists".to_string())
            }
            RepositoryError::InvalidParameter(msg) => WorkerError::DomainError(msg),
            RepositoryError::InternalError(msg) => WorkerError::InternalError(msg),
        }
    }
}

/// Extension trait for converting repository errors to WorkerError
///
/// This eliminates repetitive `.map_err(|e| WorkerError::RepositoryError(e.to_string()))` patterns.
///
/// # Usage
///
/// ```rust
/// use crate::utils::errors::RepositoryResultExt;
///
/// async fn example() -> Result<(), WorkerError> {
///     let result = repository.find_by_id(id).await?;
///     Ok(result)
/// }
/// ```
pub trait RepositoryResultExt<T> {
    /// Convert the result to WorkerError format
    fn repo_err(self) -> Result<T, WorkerError>;
}

impl<T, E: std::fmt::Display> RepositoryResultExt<T> for Result<T, E> {
    fn repo_err(self) -> Result<T, WorkerError> {
        self.map_err(|e| WorkerError::RepositoryError(e.to_string()))
    }
}

/// 统一的应用层错误
#[derive(Error, Debug)]
pub enum AppError {
    #[error("认证失败: {0}")]
    Authentication(String),

    #[error("授权失败: {0}")]
    Authorization(String),

    #[error("验证错误: {0}")]
    Validation(String),

    #[error("未找到资源: {0}")]
    NotFound(String),

    #[error("请求过多: {0}")]
    RateLimited(String),

    #[error("内部服务器错误: {0}")]
    Internal(String),

    #[error("服务不可用: {0}")]
    ServiceUnavailable(String),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Authentication(msg) => (axum::http::StatusCode::UNAUTHORIZED, msg),
            AppError::Authorization(msg) => (axum::http::StatusCode::FORBIDDEN, msg),
            AppError::Validation(msg) => (axum::http::StatusCode::BAD_REQUEST, msg),
            AppError::NotFound(msg) => (axum::http::StatusCode::NOT_FOUND, msg),
            AppError::RateLimited(msg) => (axum::http::StatusCode::TOO_MANY_REQUESTS, msg),
            AppError::Internal(msg) => {
                // 在生产环境中脱敏内部错误消息
                let sanitized_msg = if should_show_detailed_errors() {
                    msg
                } else {
                    sanitize_error_message(&msg)
                };
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, sanitized_msg)
            }
            AppError::ServiceUnavailable(msg) => {
                let sanitized_msg = if should_show_detailed_errors() {
                    msg
                } else {
                    sanitize_error_message(&msg)
                };
                (axum::http::StatusCode::SERVICE_UNAVAILABLE, sanitized_msg)
            }
        };

        // 统一使用 { "success": false, "error": "message" } 格式
        let body = serde_json::json!({
            "success": false,
            "error": message,
        });

        (status, axum::Json(body)).into_response()
    }
}

// From implementations for AppError using macro
impl_basic_error_conversions!(AppError, Internal);

impl From<RepositoryError> for AppError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::DatabaseError(msg) => AppError::Internal(msg),
            RepositoryError::NotFound => AppError::NotFound("Resource not found".to_string()),
            RepositoryError::AlreadyExists => {
                AppError::Validation("Resource already exists".to_string())
            }
            RepositoryError::InvalidParameter(msg) => AppError::Validation(msg),
            RepositoryError::InternalError(msg) => AppError::Internal(msg),
        }
    }
}

impl From<crate::domain::repositories::task_repository::RepositoryError> for AppError {
    fn from(err: crate::domain::repositories::task_repository::RepositoryError) -> Self {
        match err {
            crate::domain::repositories::task_repository::RepositoryError::Database(db_err) => {
                AppError::Internal(db_err.to_string())
            }
            crate::domain::repositories::task_repository::RepositoryError::NotFound => {
                AppError::NotFound("Resource not found".to_string())
            }
        }
    }
}

/// From implementations for security-related errors
impl From<confers::ConfigError> for AppError {
    fn from(err: confers::ConfigError) -> Self {
        AppError::Validation(err.to_string())
    }
}

impl From<crate::utils::robots::RobotsCheckerError> for AppError {
    fn from(err: crate::utils::robots::RobotsCheckerError) -> Self {
        AppError::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use crate::common::test_support::ENV_MUTEX;
    #[test]
    fn test_sanitize_error_message_removes_table_names() {
        let msg = "Database error: table: users column: email not found";
        let sanitized = sanitize_error_message(msg);
        // Table/column patterns are replaced with [REDACTED]
        assert!(sanitized.contains("[REDACTED]"));
        // Verify original table/column names are removed
        assert!(!sanitized.contains("users"));
        assert!(!sanitized.contains("email"));
    }

    #[test]
    fn test_sanitize_error_message_removes_file_paths() {
        let msg = "Error at /home/dev/crawlrs/src/main.rs:42";
        let sanitized = sanitize_error_message(msg);
        // File path should be redacted
        assert!(!sanitized.contains("/home/dev/crawlrs/"));
        // File path is replaced with [FILE_PATH_REDACTED]
        assert!(sanitized.contains("[FILE_PATH_REDACTED]"));
    }

    #[test]
    fn test_sanitize_error_message_removes_internal_ips() {
        let msg = "Connection failed to 10.0.0.1:5432";
        let sanitized = sanitize_error_message(msg);
        assert!(!sanitized.contains("10.0.0.1"));
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
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CRAWLRS_ENV", "development");
        assert!(should_show_detailed_errors());
        std::env::remove_var("CRAWLRS_ENV");
    }

    #[test]
    fn test_should_hide_detailed_errors_in_prod() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CRAWLRS_ENV", "production");
        assert!(!should_show_detailed_errors());
        std::env::remove_var("CRAWLRS_ENV");
    }

    #[test]
    fn test_should_hide_detailed_errors_by_default() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!should_show_detailed_errors());
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
        // Should still sanitize if patterns exist
        assert!(sanitized.contains('A') || sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitize_error_message_multiple_patterns() {
        let msg = "Error: table: users column: name SQL: SELECT * FROM users";
        let sanitized = sanitize_error_message(msg);
        assert!(sanitized.contains("[REDACTED]"));
        assert!(sanitized.contains("[SQL_REDACTED]"));
    }

    // ========== WorkerError From conversions ==========

    #[test]
    fn test_worker_error_from_string() {
        let err: WorkerError = "something failed".to_string().into();
        assert!(matches!(err, WorkerError::InternalError(msg) if msg == "something failed"));
    }

    #[test]
    fn test_worker_error_from_str() {
        let err: WorkerError = "disk full".into();
        assert!(matches!(err, WorkerError::InternalError(msg) if msg == "disk full"));
    }

    #[test]
    fn test_worker_error_from_anyhow() {
        let err: WorkerError = anyhow::anyhow!("network down").into();
        assert!(matches!(err, WorkerError::InternalError(msg) if msg.contains("network down")));
    }

    // ========== AppError From conversions ==========

    #[test]
    fn test_app_error_from_string() {
        let err: AppError = "boom".to_string().into();
        assert!(matches!(err, AppError::Internal(msg) if msg == "boom"));
    }

    #[test]
    fn test_app_error_from_str() {
        let err: AppError = "crash".into();
        assert!(matches!(err, AppError::Internal(msg) if msg == "crash"));
    }

    #[test]
    fn test_app_error_from_anyhow() {
        let err: AppError = anyhow::anyhow!("db timeout").into();
        assert!(matches!(err, AppError::Internal(msg) if msg.contains("db timeout")));
    }

    // ========== From<RepositoryError> for WorkerError ==========

    #[test]
    fn test_worker_error_from_repository_database_error() {
        let err: WorkerError = RepositoryError::DatabaseError("conn refused".to_string()).into();
        assert!(matches!(err, WorkerError::RepositoryError(msg) if msg == "conn refused"));
    }

    #[test]
    fn test_worker_error_from_repository_not_found() {
        let err: WorkerError = RepositoryError::NotFound.into();
        assert!(matches!(err, WorkerError::NotFound(msg) if msg.contains("not found")));
    }

    #[test]
    fn test_worker_error_from_repository_already_exists() {
        let err: WorkerError = RepositoryError::AlreadyExists.into();
        assert!(matches!(err, WorkerError::RepositoryError(msg) if msg.contains("already exists")));
    }

    #[test]
    fn test_worker_error_from_repository_invalid_parameter() {
        let err: WorkerError = RepositoryError::InvalidParameter("bad uuid".to_string()).into();
        assert!(matches!(err, WorkerError::DomainError(msg) if msg == "bad uuid"));
    }

    #[test]
    fn test_worker_error_from_repository_internal_error() {
        let err: WorkerError = RepositoryError::InternalError("oops".to_string()).into();
        assert!(matches!(err, WorkerError::InternalError(msg) if msg == "oops"));
    }

    // ========== From<RepositoryError> for AppError ==========

    #[test]
    fn test_app_error_from_repository_database_error() {
        let err: AppError = RepositoryError::DatabaseError("conn refused".to_string()).into();
        assert!(matches!(err, AppError::Internal(msg) if msg == "conn refused"));
    }

    #[test]
    fn test_app_error_from_repository_not_found() {
        let err: AppError = RepositoryError::NotFound.into();
        assert!(matches!(err, AppError::NotFound(msg) if msg.contains("not found")));
    }

    #[test]
    fn test_app_error_from_repository_already_exists() {
        let err: AppError = RepositoryError::AlreadyExists.into();
        assert!(matches!(err, AppError::Validation(msg) if msg.contains("already exists")));
    }

    #[test]
    fn test_app_error_from_repository_invalid_parameter() {
        let err: AppError = RepositoryError::InvalidParameter("bad input".to_string()).into();
        assert!(matches!(err, AppError::Validation(msg) if msg == "bad input"));
    }

    #[test]
    fn test_app_error_from_repository_internal_error() {
        let err: AppError = RepositoryError::InternalError("oops".to_string()).into();
        assert!(matches!(err, AppError::Internal(msg) if msg == "oops"));
    }

    // ========== From<task_repository::RepositoryError> for AppError ==========

    #[test]
    fn test_app_error_from_task_repo_database_error() {
        use crate::domain::repositories::task_repository::RepositoryError as TaskRepoError;
        let err: AppError = TaskRepoError::Database(anyhow::anyhow!("pool exhausted")).into();
        assert!(matches!(err, AppError::Internal(msg) if msg.contains("pool exhausted")));
    }

    #[test]
    fn test_app_error_from_task_repo_not_found() {
        use crate::domain::repositories::task_repository::RepositoryError as TaskRepoError;
        let err: AppError = TaskRepoError::NotFound.into();
        assert!(matches!(err, AppError::NotFound(msg) if msg.contains("not found")));
    }

    // ========== From<RobotsCheckerError> for AppError ==========

    #[test]
    fn test_app_error_from_robots_checker_error() {
        let robots_err = crate::utils::robots::RobotsCheckerError::ValidationError("blocked".to_string());
        let err: AppError = robots_err.into();
        assert!(matches!(err, AppError::Internal(msg) if msg.contains("blocked")));
    }

    // ========== RepositoryResultExt ==========

    #[test]
    fn test_repository_result_ext_ok() {
        let result: Result<i32, &str> = Ok(42);
        assert_eq!(result.repo_err().expect("ok should pass through"), 42);
    }

    #[test]
    fn test_repository_result_ext_err_maps_to_worker_error() {
        let result: Result<i32, &str> = Err("disk failure");
        let err = result.repo_err().unwrap_err();
        assert!(matches!(err, WorkerError::RepositoryError(msg) if msg.contains("disk failure")));
    }

    // ========== AppError::into_response status codes ==========

    #[test]
    fn test_app_error_authentication_returns_401() {
        let response = AppError::Authentication("bad token".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_app_error_authorization_returns_403() {
        let response = AppError::Authorization("no scope".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_app_error_validation_returns_400() {
        let response = AppError::Validation("missing field".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_app_error_not_found_returns_404() {
        let response = AppError::NotFound("no such task".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_app_error_rate_limited_returns_429() {
        let response = AppError::RateLimited("too fast".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_app_error_internal_returns_500() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("CRAWLRS_ENV");
        let response = AppError::Internal("table: users not found".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_app_error_service_unavailable_returns_503() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("CRAWLRS_ENV");
        let response = AppError::ServiceUnavailable("redis down".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    // ========== AppError Internal sanitization in production ==========

    #[tokio::test]
    async fn test_app_error_internal_sanitizes_in_production() {
        {
            let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var("CRAWLRS_ENV", "production");
        }
        let response = AppError::Internal(
            "Error: table: users column: email at /home/app/src/main.rs:42".to_string(),
        )
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        // The response body should not contain the raw sensitive data.
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = std::str::from_utf8(&body).expect("body should be utf8");
        assert!(!body_str.contains("/home/app/src/main.rs"), "file path should be redacted");
        {
            let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            std::env::remove_var("CRAWLRS_ENV");
        }
    }

    #[tokio::test]
    async fn test_app_error_internal_keeps_detail_in_dev() {
        {
            let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var("CRAWLRS_ENV", "development");
        }
        let response = AppError::Internal("table: users".to_string()).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = std::str::from_utf8(&body).expect("body should be utf8");
        // In dev mode, the raw message is preserved (not sanitized).
        assert!(body_str.contains("table: users"), "raw message should be preserved in dev");
        {
            let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            std::env::remove_var("CRAWLRS_ENV");
        }
    }

    // ========== AppError IntoResponse body format ==========

    #[tokio::test]
    async fn test_app_error_response_body_format() {
        let response = AppError::Validation("invalid email".to_string()).into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");
        assert_eq!(json["success"], serde_json::Value::Bool(false));
        assert_eq!(json["error"], serde_json::Value::String("invalid email".to_string()));
    }
}
