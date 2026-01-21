// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 错误处理辅助函数
//!
//! 提供统一的错误转换和处理辅助函数

use crate::common::error::{AppError, AppResult};
use std::fmt::Display;

/// 将错误转换为 AppError::Other
pub fn map_to_other_error<E: Display>(error: E) -> AppError {
    AppError::Other(error.to_string())
}

/// 将错误转换为 AppError::Database
pub fn map_to_database_error<E: Display>(error: E) -> AppError {
    AppError::Database(sea_orm::DbErr::Custom(error.to_string()))
}

/// 将错误转换为 AppError::Network
pub fn map_to_network_error<E: Display>(error: E) -> AppError {
    // 由于 reqwest::Error 不能直接从 StatusCode 创建，我们使用其他方式
    AppError::Other(format!("Network error: {}", error))
}

/// 将错误转换为 AppError::Engine
pub fn map_to_engine_error<E: Display>(error: E) -> AppError {
    AppError::Engine(error.to_string())
}

/// 将错误转换为 AppError::Cache
pub fn map_to_cache_error<E: Display>(error: E) -> AppError {
    AppError::Cache(error.to_string())
}

/// 将错误转换为 AppError::Task
pub fn map_to_task_error<E: Display>(error: E) -> AppError {
    AppError::Task(error.to_string())
}

/// 将错误转换为 AppError::RateLimit
pub fn map_to_rate_limit_error<E: Display>(error: E) -> AppError {
    AppError::RateLimit(error.to_string())
}

/// 为 Result 类型提供便捷的错误映射方法
pub trait ResultExt<T, E> {
    /// 将错误映射为 AppError::Other
    fn map_err_to_other(self) -> AppResult<T>;

    /// 将错误映射为 AppError::Database
    fn map_err_to_database(self) -> AppResult<T>;

    /// 将错误映射为 AppError::Network
    fn map_err_to_network(self) -> AppResult<T>;

    /// 将错误映射为 AppError::Engine
    fn map_err_to_engine(self) -> AppResult<T>;

    /// 将错误映射为 AppError::Cache
    fn map_err_to_cache(self) -> AppResult<T>;

    /// 将错误映射为 AppError::Task
    fn map_err_to_task(self) -> AppResult<T>;

    /// 将错误映射为 AppError::RateLimit
    fn map_err_to_rate_limit(self) -> AppResult<T>;
}

impl<T, E: Display> ResultExt<T, E> for Result<T, E> {
    fn map_err_to_other(self) -> AppResult<T> {
        self.map_err(map_to_other_error)
    }

    fn map_err_to_database(self) -> AppResult<T> {
        self.map_err(map_to_database_error)
    }

    fn map_err_to_network(self) -> AppResult<T> {
        self.map_err(map_to_network_error)
    }

    fn map_err_to_engine(self) -> AppResult<T> {
        self.map_err(map_to_engine_error)
    }

    fn map_err_to_cache(self) -> AppResult<T> {
        self.map_err(map_to_cache_error)
    }

    fn map_err_to_task(self) -> AppResult<T> {
        self.map_err(map_to_task_error)
    }

    fn map_err_to_rate_limit(self) -> AppResult<T> {
        self.map_err(map_to_rate_limit_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_to_other_error() {
        let error = map_to_other_error("test error");
        assert!(matches!(error, AppError::Other(_)));
    }

    #[test]
    fn test_map_to_database_error() {
        let error = map_to_database_error("db error");
        assert!(matches!(error, AppError::Database(_)));
    }

    #[test]
    fn test_result_ext() {
        let result: Result<i32, &str> = Err("test");
        let app_result: AppResult<i32> = result.map_err_to_other();
        assert!(matches!(app_result, Err(AppError::Other(_))));
    }
}
