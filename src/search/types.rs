// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineHealth {
    #[default]
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
    Isolated,
}

impl EngineHealth {
    pub fn is_available(&self) -> bool {
        matches!(self, EngineHealth::Healthy | EngineHealth::Degraded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== SearchEngineType::name tests ==========

    #[test]
    fn test_name_all_variants() {
        assert_eq!(SearchEngineType::Google.name(), "Google");
        assert_eq!(SearchEngineType::Bing.name(), "Bing");
        assert_eq!(SearchEngineType::Baidu.name(), "Baidu");
        assert_eq!(SearchEngineType::Sogou.name(), "Sogou");
        assert_eq!(SearchEngineType::Auto.name(), "Auto");
        assert_eq!(SearchEngineType::Smart.name(), "Smart");
        assert_eq!(SearchEngineType::ABTest.name(), "ABTest");
    }

    // ========== SearchEngineType::from_name tests ==========

    #[test]
    fn test_from_name_canonical() {
        assert_eq!(
            SearchEngineType::from_name("Google"),
            Some(SearchEngineType::Google)
        );
        assert_eq!(
            SearchEngineType::from_name("Bing"),
            Some(SearchEngineType::Bing)
        );
        assert_eq!(
            SearchEngineType::from_name("Baidu"),
            Some(SearchEngineType::Baidu)
        );
        assert_eq!(
            SearchEngineType::from_name("Sogou"),
            Some(SearchEngineType::Sogou)
        );
        assert_eq!(
            SearchEngineType::from_name("Auto"),
            Some(SearchEngineType::Auto)
        );
        assert_eq!(
            SearchEngineType::from_name("Smart"),
            Some(SearchEngineType::Smart)
        );
        assert_eq!(
            SearchEngineType::from_name("ABTest"),
            Some(SearchEngineType::ABTest)
        );
    }

    #[test]
    fn test_from_name_lowercase() {
        assert_eq!(
            SearchEngineType::from_name("google"),
            Some(SearchEngineType::Google)
        );
        assert_eq!(
            SearchEngineType::from_name("bing"),
            Some(SearchEngineType::Bing)
        );
        assert_eq!(
            SearchEngineType::from_name("baidu"),
            Some(SearchEngineType::Baidu)
        );
        assert_eq!(
            SearchEngineType::from_name("sogou"),
            Some(SearchEngineType::Sogou)
        );
        assert_eq!(
            SearchEngineType::from_name("auto"),
            Some(SearchEngineType::Auto)
        );
        assert_eq!(
            SearchEngineType::from_name("smart"),
            Some(SearchEngineType::Smart)
        );
        assert_eq!(
            SearchEngineType::from_name("abtest"),
            Some(SearchEngineType::ABTest)
        );
    }

    #[test]
    fn test_from_name_ab_test_alias() {
        assert_eq!(
            SearchEngineType::from_name("ab_test"),
            Some(SearchEngineType::ABTest)
        );
    }

    #[test]
    fn test_from_name_invalid_returns_none() {
        assert_eq!(SearchEngineType::from_name("DuckDuckGo"), None);
        assert_eq!(SearchEngineType::from_name(""), None);
        assert_eq!(SearchEngineType::from_name("GOOGLE"), None);
        assert_eq!(SearchEngineType::from_name("yahoo"), None);
    }

    #[test]
    fn test_name_from_name_roundtrip() {
        for variant in [
            SearchEngineType::Google,
            SearchEngineType::Bing,
            SearchEngineType::Baidu,
            SearchEngineType::Sogou,
            SearchEngineType::Auto,
            SearchEngineType::Smart,
            SearchEngineType::ABTest,
        ] {
            let name = variant.name();
            assert_eq!(
                SearchEngineType::from_name(name),
                Some(variant),
                "roundtrip failed for {}",
                name
            );
        }
    }

    // ========== SearchEngineType trait derivations ==========

    #[test]
    fn test_search_engine_type_clone_copy() {
        let a = SearchEngineType::Google;
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn test_search_engine_type_equality() {
        assert_eq!(SearchEngineType::Google, SearchEngineType::Google);
        assert_ne!(SearchEngineType::Google, SearchEngineType::Bing);
    }

    #[test]
    fn test_search_engine_type_serde_roundtrip() {
        for variant in [
            SearchEngineType::Google,
            SearchEngineType::Bing,
            SearchEngineType::Baidu,
            SearchEngineType::Sogou,
            SearchEngineType::Auto,
            SearchEngineType::Smart,
            SearchEngineType::ABTest,
        ] {
            let json = serde_json::to_string(&variant).expect("serialize");
            let back: SearchEngineType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(variant, back, "serde roundtrip failed for {}", json);
        }
    }

    // ========== EngineHealth tests ==========

    #[test]
    fn test_engine_health_default_is_healthy() {
        let health = EngineHealth::default();
        assert_eq!(health, EngineHealth::Healthy);
    }

    #[test]
    fn test_is_available_healthy() {
        assert!(EngineHealth::Healthy.is_available());
    }

    #[test]
    fn test_is_available_degraded() {
        assert!(EngineHealth::Degraded.is_available());
    }

    #[test]
    fn test_is_available_unhealthy() {
        assert!(!EngineHealth::Unhealthy.is_available());
    }

    #[test]
    fn test_is_available_unknown() {
        assert!(!EngineHealth::Unknown.is_available());
    }

    #[test]
    fn test_is_available_isolated() {
        assert!(!EngineHealth::Isolated.is_available());
    }

    #[test]
    fn test_engine_health_clone_copy() {
        let a = EngineHealth::Degraded;
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn test_engine_health_equality() {
        assert_eq!(EngineHealth::Healthy, EngineHealth::Healthy);
        assert_ne!(EngineHealth::Healthy, EngineHealth::Degraded);
        assert_ne!(EngineHealth::Unhealthy, EngineHealth::Isolated);
        assert_ne!(EngineHealth::Unknown, EngineHealth::Isolated);
    }
}
