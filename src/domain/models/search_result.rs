// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub engine: String,
    pub score: f64,
    pub published_time: Option<DateTime<Utc>>,
}

impl Default for SearchResult {
    fn default() -> Self {
        Self {
            title: String::new(),
            url: String::new(),
            description: None,
            engine: String::new(),
            score: 0.0,
            published_time: None,
        }
    }
}

impl SearchResult {
    pub fn new(title: String, url: String, description: Option<String>, engine: String) -> Self {
        Self {
            title,
            url,
            description,
            engine,
            score: 0.0,
            published_time: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== SearchResult::default tests ==========

    #[test]
    fn test_default_produces_empty_fields() {
        let result = SearchResult::default();

        assert_eq!(result.title, "", "default title should be empty string");
        assert_eq!(result.url, "", "default url should be empty string");
        assert!(
            result.description.is_none(),
            "default description should be None"
        );
        assert_eq!(result.engine, "", "default engine should be empty string");
        assert_eq!(result.score, 0.0, "default score should be 0.0");
        assert!(
            result.published_time.is_none(),
            "default published_time should be None"
        );
    }

    #[test]
    fn test_default_impl_is_consistent() {
        // Verify Default is stable across calls
        let a = SearchResult::default();
        let b = SearchResult::default();
        assert_eq!(a, b, "default should be deterministic");
    }

    // ========== SearchResult::new tests ==========

    #[test]
    fn test_new_with_description_sets_fields_and_defaults() {
        let result = SearchResult::new(
            "Rust Programming".to_string(),
            "https://rust-lang.org".to_string(),
            Some("Official site".to_string()),
            "google".to_string(),
        );

        assert_eq!(result.title, "Rust Programming");
        assert_eq!(result.url, "https://rust-lang.org");
        assert_eq!(
            result.description,
            Some("Official site".to_string()),
            "description should match provided value"
        );
        assert_eq!(result.engine, "google");
        assert_eq!(result.score, 0.0, "new should default score to 0.0");
        assert!(
            result.published_time.is_none(),
            "new should default published_time to None"
        );
    }

    #[test]
    fn test_new_without_description_sets_none() {
        let result = SearchResult::new(
            "Example".to_string(),
            "https://example.com".to_string(),
            None,
            "bing".to_string(),
        );

        assert_eq!(result.title, "Example");
        assert_eq!(result.url, "https://example.com");
        assert!(
            result.description.is_none(),
            "None description should be preserved"
        );
        assert_eq!(result.engine, "bing");
        assert_eq!(result.score, 0.0);
        assert!(result.published_time.is_none());
    }

    #[test]
    fn test_new_with_empty_strings() {
        let result = SearchResult::new(String::new(), String::new(), None, String::new());

        assert_eq!(result.title, "");
        assert_eq!(result.url, "");
        assert_eq!(result.engine, "");
        assert!(result.description.is_none());
        assert_eq!(result.score, 0.0);
    }

    // ========== Serialization tests ==========

    #[test]
    fn test_serde_roundtrip_with_description() {
        let result = SearchResult::new(
            "Title".to_string(),
            "https://example.com".to_string(),
            Some("desc".to_string()),
            "google".to_string(),
        );

        let json = serde_json::to_string(&result).expect("serialize");
        let back: SearchResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, back, "serde roundtrip should preserve SearchResult");
    }

    #[test]
    fn test_serde_roundtrip_without_description() {
        let result = SearchResult::new(
            "No Desc".to_string(),
            "https://nodec.com".to_string(),
            None,
            "baidu".to_string(),
        );

        let json = serde_json::to_string(&result).expect("serialize");
        let back: SearchResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, back, "serde roundtrip should preserve SearchResult");
    }

    #[test]
    fn test_serde_roundtrip_with_published_time() {
        let mut result = SearchResult::new(
            "News".to_string(),
            "https://news.com".to_string(),
            Some("breaking".to_string()),
            "sogou".to_string(),
        );
        result.score = 0.95;
        result.published_time = Some(Utc::now());

        let json = serde_json::to_string(&result).expect("serialize");
        let back: SearchResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, back, "serde roundtrip should preserve all fields");
        assert!(
            back.published_time.is_some(),
            "published_time should survive roundtrip"
        );
        assert!(
            (back.score - 0.95).abs() < f64::EPSILON,
            "score should survive roundtrip"
        );
    }

    // ========== Clone / PartialEq tests ==========

    #[test]
    fn test_clone_produces_equal_result() {
        let result = SearchResult::new(
            "Clone Me".to_string(),
            "https://clone.com".to_string(),
            Some("desc".to_string()),
            "google".to_string(),
        );
        let cloned = result.clone();
        assert_eq!(result, cloned, "clone should be equal to original");
    }

    #[test]
    fn test_partial_eq_different_titles_not_equal() {
        let a = SearchResult::new(
            "Title A".to_string(),
            "https://same.com".to_string(),
            None,
            "google".to_string(),
        );
        let b = SearchResult::new(
            "Title B".to_string(),
            "https://same.com".to_string(),
            None,
            "google".to_string(),
        );
        assert_ne!(a, b, "different titles should not be equal");
    }

    #[test]
    fn test_partial_eq_different_scores_not_equal() {
        let mut a = SearchResult::new(
            "Title".to_string(),
            "https://same.com".to_string(),
            None,
            "google".to_string(),
        );
        let b = a.clone();
        a.score = 1.0;
        assert_ne!(a, b, "different scores should not be equal");
    }
}
