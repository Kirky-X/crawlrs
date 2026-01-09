// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};

use super::types::SearchEngineType;

/// 搜索结果项
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseItem {
    pub title: String,
    pub url: String,
    pub description: String,
    pub engine: SearchEngineType,
}

/// 搜索响应
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response<T> {
    pub items: Vec<T>,
    pub total_results: Option<u64>,
    pub engine: SearchEngineType,
}
