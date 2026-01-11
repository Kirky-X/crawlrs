// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};

/// 搜索引擎类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchEngineType {
    Google,
    Bing,
    Baidu,
    Sogou,
    Auto,
    Smart,
    ABTest,
}

impl SearchEngineType {
    pub fn name(&self) -> &'static str {
        match self {
            SearchEngineType::Google => "Google",
            SearchEngineType::Bing => "Bing",
            SearchEngineType::Baidu => "Baidu",
            SearchEngineType::Sogou => "Sogou",
            SearchEngineType::Auto => "Auto",
            SearchEngineType::Smart => "Smart",
            SearchEngineType::ABTest => "ABTest",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Google" | "google" => Some(SearchEngineType::Google),
            "Bing" | "bing" => Some(SearchEngineType::Bing),
            "Baidu" | "baidu" => Some(SearchEngineType::Baidu),
            "Sogou" | "sogou" => Some(SearchEngineType::Sogou),
            "Auto" | "auto" => Some(SearchEngineType::Auto),
            "Smart" | "smart" => Some(SearchEngineType::Smart),
            "ABTest" | "abtest" | "ab_test" => Some(SearchEngineType::ABTest),
            _ => None,
        }
    }
}

/// 引擎健康状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
    Isolated,
}

impl Default for EngineHealth {
    fn default() -> Self {
        Self::Healthy
    }
}

impl EngineHealth {
    pub fn is_available(&self) -> bool {
        matches!(self, EngineHealth::Healthy | EngineHealth::Degraded)
    }
}
