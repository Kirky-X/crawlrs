// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use thiserror::Error;
use tokio::time::error::Elapsed;

/// 搜索错误类型
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("搜索超时: {0}")]
    Timeout(#[from] Elapsed),
    #[error("解析错误: {0}")]
    Parse(String),
    #[error("内容解析错误: {0}")]
    ContentParsing(String),
    #[error("搜索引擎错误: {0}")]
    Engine(String),
    #[error("熔断器打开: {0}")]
    CircuitOpen(String),
    #[error("没有可用的搜索引擎")]
    NoEngineAvailable,
}
