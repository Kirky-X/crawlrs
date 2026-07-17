// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 错误处理辅助函数
//!
//! 提供统一的错误转换和处理辅助函数

use crate::common::error::CrawlRsError;
use std::fmt::Display;

/// 将错误转换为 CrawlRsError::Other
pub fn map_to_other_error<E: Display>(error: E) -> CrawlRsError {
    CrawlRsError::Other(error.to_string())
}

/// 将错误转换为 CrawlRsError::Database
pub fn map_to_database_error<E: Display>(error: E) -> CrawlRsError {
    CrawlRsError::Database(sea_orm::DbErr::Custom(error.to_string()))
}

/// 将错误转换为 CrawlRsError::Network
pub fn map_to_network_error<E: Display>(error: E) -> CrawlRsError {
    CrawlRsError::Network(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_to_other_error() {
        let error = map_to_other_error("test error");
        assert!(matches!(error, CrawlRsError::Other(_)));
    }

    #[test]
    fn test_map_to_database_error() {
        let error = map_to_database_error("db error");
        assert!(matches!(error, CrawlRsError::Database(_)));
    }

    #[test]
    fn test_map_to_network_error() {
        let error = map_to_network_error("network failure");
        assert!(matches!(error, CrawlRsError::Network(_)));
    }
}
