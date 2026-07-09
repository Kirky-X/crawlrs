// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 错误处理辅助函数
//!
//! 提供统一的错误转换和处理辅助函数

use crate::common::error::AppError;
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
    AppError::Network(format!("Network error: {}", error))
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
    fn test_map_to_network_error() {
        let error = map_to_network_error("network failure");
        assert!(matches!(error, AppError::Network(_)));
    }
}
