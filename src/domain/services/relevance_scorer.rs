// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 相关性评分服务
//!
//! 提供搜索结果相关性评分功能，支持通过 DI 注入自定义日期解析器。

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use crate::utils::regex_cache::RegexCacheTrait;

/// Relevance scorer errors
#[derive(Error, Debug)]
pub enum RelevanceScorerError {
    #[error("Regex cache lock error: {0}")]
    CacheLockError(String),

    #[error("Regex compilation error: {0}")]
    RegexError(String),
}

// From implementations for RelevanceScorerError
impl From<String> for RelevanceScorerError {
    fn from(msg: String) -> Self {
        RelevanceScorerError::RegexError(msg)
    }
}

impl From<&str> for RelevanceScorerError {
    fn from(msg: &str) -> Self {
        RelevanceScorerError::RegexError(msg.to_string())
    }
}

impl From<anyhow::Error> for RelevanceScorerError {
    fn from(err: anyhow::Error) -> Self {
        RelevanceScorerError::RegexError(err.to_string())
    }
}

type DateParser = fn(&str) -> Option<DateTime<Utc>>;

/// 日期解析器 trait（支持 DI）
///
/// 提供日期提取的抽象接口，便于测试时注入 mock 实现。
pub trait DateParserTrait: Send + Sync {
    /// 从文本中提取日期
    fn extract_date(&self, text: &str) -> Option<DateTime<Utc>>;
}

/// 默认日期解析器组件
///
/// 预编译了常用日期格式的正则表达式。
pub struct DateParserComponent {
    /// 预编译的日期正则表达式
    date_regexes: Vec<(Regex, DateParser)>,
}

impl DateParserComponent {
    /// 创建新的日期解析器组件
    pub fn new() -> Self {
        Self {
            date_regexes: Self::init_date_regexes(),
        }
    }

    /// Factory function: Create a DateParserComponent with default settings
    ///
    /// This is the recommended factory function for creating date parsers.
    pub fn with_defaults() -> Self {
        Self::new()
    }

    fn init_date_regexes() -> Vec<(Regex, DateParser)> {
        vec![
            // ISO 8601 format: 2024-01-15T10:30:00Z
            (Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{3})?Z").expect("Invalid ISO 8601 date regex"), |s| {
                DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
            }),
            // Date format: 2024-01-15
            (Regex::new(r"\d{4}-\d{2}-\d{2}").expect("Invalid date format regex"), |s| {
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0).map(|d| d.and_utc()))
            }),
            // Relative time: 2 hours ago, 3 days ago, 1 week ago
            (Regex::new(r"(\d+)\s+(hour|hours|day|days|week|weeks|month|months|year|years)\s+ago").expect("Invalid relative time regex"), |s| {
                let relative_regex = Regex::new(r"(\d+)\s+(hour|hours|day|days|week|weeks|month|months|year|years)").expect("Invalid relative time pattern regex");
                let captures = relative_regex.captures(s)?;
                let num: i64 = captures.get(1)?.as_str().parse().ok()?;
                let unit = captures.get(2)?.as_str();

                let duration = match unit {
                    "hour" | "hours" => Duration::hours(num),
                    "day" | "days" => Duration::days(num),
                    "week" | "weeks" => Duration::weeks(num),
                    "month" | "months" => Duration::days(num * 30),
                    "year" | "years" => Duration::days(num * 365),
                    _ => return None,
                };

                Some(Utc::now() - duration)
            }),
            // Common formats: Jan 15, 2024, January 15, 2024
            (Regex::new(r"(Jan|January|Feb|February|Mar|March|Apr|April|May|Jun|June|Jul|July|Aug|August|Sep|September|Oct|October|Nov|November|Dec|December)\s+(\d{1,2}),?\s+(\d{4})").expect("Invalid month format regex"), |s| {
                chrono::NaiveDate::parse_from_str(s, "%b %d, %Y").ok()
                    .or_else(|| chrono::NaiveDate::parse_from_str(s, "%B %d, %Y").ok())
                    .and_then(|date| date.and_hms_opt(0, 0, 0).map(|d| d.and_utc()))
            }),
        ]
    }
}

impl Default for DateParserComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl DateParserTrait for DateParserComponent {
    fn extract_date(&self, text: &str) -> Option<DateTime<Utc>> {
        for (regex, parser) in self.date_regexes.iter() {
            if let Some(captures) = regex.find(text) {
                if let Some(date) = parser(captures.as_str()) {
                    return Some(date);
                }
            }
        }
        None
    }
}

pub struct RelevanceScorer {
    query_terms: Vec<String>,
    term_weights: HashMap<String, f64>,
    /// Regex cache for word boundary matching (optional, for DI)
    regex_cache: Option<Arc<dyn RegexCacheTrait>>,
}

impl RelevanceScorer {
    /// Create a new RelevanceScorer without regex cache
    pub fn new(query: &str) -> Self {
        Self::new_with_cache(query, None)
    }

    /// Factory function: Create a RelevanceScorer for a query
    ///
    /// This is the recommended factory function for query-based scoring.
    pub fn for_query(query: &str) -> Self {
        Self::new(query)
    }

    /// Factory function: Create a RelevanceScorer with a specific engine name
    ///
    /// Use this when scoring results from a specific search engine.
    pub fn with_engine(engine_name: &str) -> Self {
        Self::new(engine_name)
    }

    /// Create a new RelevanceScorer with regex cache for dependency injection
    pub fn new_with_cache(query: &str, regex_cache: Option<Arc<dyn RegexCacheTrait>>) -> Self {
        let query_lower = query.to_lowercase();
        let terms: Vec<String> = query_lower
            .split_whitespace()
            .filter(|term| term.len() > 2) // Filter out very short terms
            .map(|s| s.to_string())
            .collect();

        // Calculate TF-IDF-like weights for query terms
        let total_terms = terms.len();
        let unique_terms = terms.iter().collect::<std::collections::HashSet<_>>().len();
        let mut term_weights = HashMap::with_capacity(unique_terms);

        let total_terms_f64 = total_terms as f64;

        for term in &terms {
            let count = terms.iter().filter(|t| t == &term).count() as f64;
            let tf = count / total_terms_f64;
            // Simple IDF approximation (logarithmic scale)
            let idf = (1.0 + total_terms_f64 / count).ln();
            term_weights.insert(term.clone(), tf * idf);
        }

        Self {
            query_terms: terms,
            term_weights,
            regex_cache,
        }
    }

    /// Calculate relevance score for a search result
    pub fn calculate_score(&self, title: &str, description: Option<&str>, url: &str) -> f64 {
        let mut score = 0.0;
        let title_lower = title.to_lowercase();
        let desc_lower = description.map(|d| d.to_lowercase()).unwrap_or_default();
        let url_lower = url.to_lowercase();

        // Title relevance (highest weight)
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            // Exact title match
            if title_lower.contains(term) {
                score += 2.0 * weight;
            }

            // Title starts with term
            if title_lower.starts_with(term) {
                score += 1.5 * weight;
            }

            // Title word boundary match
            if self.has_word_boundary_match(&title_lower, term) {
                score += 1.2 * weight;
            }
        }

        // Description relevance (medium weight)
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            if desc_lower.contains(term) {
                score += 0.8 * weight;
            }

            if self.has_word_boundary_match(&desc_lower, term) {
                score += 0.6 * weight;
            }
        }

        // URL relevance (lower weight)
        for term in &self.query_terms {
            let weight = self.term_weights.get(term).unwrap_or(&1.0);

            if url_lower.contains(term) {
                score += 0.4 * weight;
            }
        }

        // Domain authority bonus (simplified)
        if self.is_authoritative_domain(url) {
            score += 0.5;
        }

        // Length penalty for very short titles (potential spam)
        if title.len() < 10 {
            score *= 0.8;
        }

        // Freshness bonus (if we have publication date)
        score
    }

    /// Check if term appears at word boundaries
    fn has_word_boundary_match(&self, text: &str, term: &str) -> bool {
        // Use injected regex cache if available, otherwise use simple contains
        if let Some(cache) = &self.regex_cache {
            match cache.get_or_insert_escaped(term) {
                Ok(regex) => return regex.is_match(text),
                Err(_) => return text.contains(term),
            }
        }
        text.contains(term) // Fallback to simple contains when no cache
    }

    /// Check if domain is authoritative (simplified heuristic)
    fn is_authoritative_domain(&self, url: &str) -> bool {
        let authoritative_domains = [
            "wikipedia.org",
            "github.com",
            "stackoverflow.com",
            "medium.com",
            "reddit.com",
            "quora.com",
            "gov",
            "edu",
            "org", // TLD patterns
        ];

        authoritative_domains
            .iter()
            .any(|domain| url.contains(domain))
    }

    /// Extract publication date from text using injected DateParser
    ///
    /// This is the recommended method for DI-based usage.
    /// Inject `Arc<dyn DateParserTrait>` via the constructor or use this method directly.
    pub fn extract_published_date_with_parser(
        text: &str,
        date_parser: &dyn DateParserTrait,
    ) -> Option<DateTime<Utc>> {
        date_parser.extract_date(text)
    }

    /// Calculate freshness score based on publication date
    pub fn calculate_freshness_score(published_date: DateTime<Utc>) -> f64 {
        let now = Utc::now();
        let age = now - published_date;

        // Recent content gets higher scores
        match age {
            age if age < Duration::days(1) => 1.0,
            age if age < Duration::days(7) => 0.8,
            age if age < Duration::days(30) => 0.6,
            age if age < Duration::days(180) => 0.4,
            age if age < Duration::days(365) => 0.2,
            _ => 0.1, // Very old content
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relevance_scoring() {
        let scorer = RelevanceScorer::new("rust programming");

        let score1 = scorer.calculate_score(
            "Rust Programming Language Tutorial",
            Some("Learn Rust programming with comprehensive tutorials"),
            "https://rust-lang.org",
        );

        let score2 = scorer.calculate_score(
            "Java Programming Guide",
            Some("Java programming concepts and examples"),
            "https://java.com",
        );

        assert!(score1 > score2, "Rust-related content should score higher");
    }

    #[test]
    fn test_date_extraction_with_parser() {
        let text = "Published on 2024-01-15T10:30:00Z, updated 2 days ago";
        let parser = DateParserComponent::new();
        let date = RelevanceScorer::extract_published_date_with_parser(text, &parser);
        assert!(date.is_some());

        let text2 = "Posted January 15, 2024";
        let date2 = RelevanceScorer::extract_published_date_with_parser(text2, &parser);
        assert!(date2.is_some());
    }

    #[test]
    fn test_freshness_score() {
        let recent = Utc::now() - Duration::days(1);
        let old = Utc::now() - Duration::days(400);

        assert!(
            RelevanceScorer::calculate_freshness_score(recent)
                > RelevanceScorer::calculate_freshness_score(old)
        );
    }

    // ---- Factory methods ----

    #[test]
    fn test_new_filters_short_terms() {
        let scorer = RelevanceScorer::new("a be the rust programming");
        // filter is term.len() > 2, so "a" (1) and "be" (2) are filtered,
        // but "the" (3) passes. Result: "the", "rust", "programming"
        assert_eq!(scorer.query_terms.len(), 3);
        assert!(scorer.query_terms.contains(&"the".to_string()));
        assert!(scorer.query_terms.contains(&"rust".to_string()));
        assert!(scorer.query_terms.contains(&"programming".to_string()));
    }

    #[test]
    fn test_for_query_factory() {
        let scorer = RelevanceScorer::for_query("rust async");
        assert_eq!(scorer.query_terms, vec!["rust", "async"]);
    }

    #[test]
    fn test_with_engine_factory() {
        let scorer = RelevanceScorer::with_engine("google");
        assert_eq!(scorer.query_terms, vec!["google"]);
    }

    #[test]
    fn test_new_with_cache_none() {
        let scorer = RelevanceScorer::new_with_cache("rust lang", None);
        assert!(scorer.regex_cache.is_none());
        assert_eq!(scorer.query_terms, vec!["rust", "lang"]);
    }

    #[test]
    fn test_term_weights_tf_idf() {
        // query with repeated term should yield a non-zero weight
        let scorer = RelevanceScorer::new("rust rust programming");
        let rust_weight = *scorer.term_weights.get("rust").expect("rust weight exists");
        let prog_weight = *scorer
            .term_weights
            .get("programming")
            .expect("programming weight exists");
        assert!(rust_weight > 0.0);
        assert!(prog_weight > 0.0);
        // rust appears twice, so its tf is higher; with idf approximation it should differ
        assert_ne!(rust_weight, prog_weight);
    }

    // ---- calculate_score ----

    #[test]
    fn test_calculate_score_empty_query_returns_zero_baseline() {
        // All terms filtered out -> no term contributions, only length penalty / authority bonus apply
        let scorer = RelevanceScorer::new("a"); // single short term filtered
        let score = scorer.calculate_score("Some Title", Some("desc"), "https://example.com");
        // No query terms means no per-term additions; only length penalty if title < 10 chars
        // "Some Title" is 10 chars so no penalty; example.com not authoritative
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_calculate_score_title_starts_with_bonus() {
        let scorer = RelevanceScorer::new("rust");
        // Title starts with "rust" -> gets starts_with + contains + word boundary bonuses
        let score_starts = scorer.calculate_score("rust language guide", None, "https://x.io");
        // Title only contains "rust" in the middle -> no starts_with bonus
        let score_contains = scorer.calculate_score("learn rust guide", None, "https://x.io");
        assert!(
            score_starts > score_contains,
            "title starting with term should score higher"
        );
    }

    #[test]
    fn test_calculate_score_description_contribution() {
        let scorer = RelevanceScorer::new("rust");
        let with_desc = scorer.calculate_score("Title", Some("rust tutorial here"), "https://x.io");
        let without_desc = scorer.calculate_score("Title", None, "https://x.io");
        assert!(
            with_desc > without_desc,
            "description match should add to score"
        );
    }

    #[test]
    fn test_calculate_score_url_contribution() {
        let scorer = RelevanceScorer::new("rust");
        let with_url = scorer.calculate_score("Title", None, "https://rust.example.org");
        let without_url = scorer.calculate_score("Title", None, "https://example.org");
        assert!(with_url > without_url, "URL match should add to score");
    }

    #[test]
    fn test_calculate_score_authoritative_domain_bonus() {
        let scorer = RelevanceScorer::new("rust");
        // Use a title >= 10 chars to avoid the short-title penalty (0.8 multiplier)
        let authoritative =
            scorer.calculate_score("Long Title Here", None, "https://github.com/rust");
        let plain = scorer.calculate_score("Long Title Here", None, "https://example.com/rust");
        // Both URLs contain "rust" so url bonus is equal; difference is authority bonus
        assert!(
            authoritative > plain,
            "authoritative domain (github.com) should add bonus"
        );
        let diff = authoritative - plain;
        // The authority bonus is exactly 0.5 (no length penalty since title >= 10 chars)
        assert!(
            (diff - 0.5).abs() < 1e-9,
            "authority bonus should be 0.5, got diff {}",
            diff
        );
    }

    #[test]
    fn test_calculate_score_short_title_penalty() {
        let scorer = RelevanceScorer::new("rust");
        // Title < 10 chars triggers 0.8 multiplier
        let short = scorer.calculate_score("rust", None, "https://x.io");
        let long = scorer.calculate_score("rust language tutorial", None, "https://x.io");
        // Both contain "rust"; long title also contains it and is longer.
        // The short title receives 0.8 penalty multiplier on the total.
        assert!(
            long > short,
            "long title should score higher due to no penalty"
        );
    }

    #[test]
    fn test_calculate_score_none_description_uses_empty() {
        let scorer = RelevanceScorer::new("rust");
        // Should not panic with None description
        let score = scorer.calculate_score("rust guide", None, "https://x.io");
        assert!(score > 0.0, "title match alone should yield positive score");
    }

    // ---- has_word_boundary_match (private but exercised via calculate_score) ----

    #[test]
    fn test_word_boundary_match_with_cache() {
        // Mock cache that successfully compiles a word-boundary regex
        struct OkRegexCache;
        impl RegexCacheTrait for OkRegexCache {
            fn get_or_insert(&self, pattern: &str) -> Result<Regex, String> {
                Regex::new(pattern).map_err(|e| e.to_string())
            }
            fn get_or_insert_escaped(&self, literal: &str) -> Result<Regex, String> {
                Regex::new(&format!(r"\b{}\b", regex::escape(literal))).map_err(|e| e.to_string())
            }
            fn get_or_compile(&self, pattern: &str) -> Result<Arc<Regex>, String> {
                Ok(Arc::new(Regex::new(pattern).map_err(|e| e.to_string())?))
            }
        }

        let cache: Arc<dyn RegexCacheTrait> = Arc::new(OkRegexCache);
        // Exercise get_or_insert directly (not used by calculate_score)
        assert!(cache.get_or_insert(r"\btest\b").is_ok());
        assert!(cache.get_or_insert(r"[invalid").is_err());
        // Exercise get_or_compile directly (not used by calculate_score)
        assert!(cache.get_or_compile(r"\b\w+\b").is_ok());
        assert!(cache.get_or_compile(r"(unclosed").is_err());

        let scorer = RelevanceScorer::new_with_cache("rust", Some(cache.clone()));
        // "rust" appears as a word boundary in "learn rust now"
        let score_word = scorer.calculate_score("learn rust now", None, "https://x.io");
        // "rust" appears only inside "rusting" - no word boundary match
        let score_substring = scorer.calculate_score("learn rusting now", None, "https://x.io");
        assert!(
            score_word > score_substring,
            "word boundary match should score higher than substring-only match"
        );
    }

    #[test]
    fn test_word_boundary_match_cache_error_falls_back_to_contains() {
        // Mock cache that always errors - should fall back to contains()
        struct ErrorRegexCache;
        impl RegexCacheTrait for ErrorRegexCache {
            fn get_or_insert(&self, _pattern: &str) -> Result<Regex, String> {
                Err("cache unavailable".to_string())
            }
            fn get_or_insert_escaped(&self, _literal: &str) -> Result<Regex, String> {
                Err("cache unavailable".to_string())
            }
            fn get_or_compile(&self, _pattern: &str) -> Result<Arc<Regex>, String> {
                Err("cache unavailable".to_string())
            }
        }

        let cache: Arc<dyn RegexCacheTrait> = Arc::new(ErrorRegexCache);
        // Exercise get_or_insert and get_or_compile directly (not used by calculate_score)
        assert!(cache.get_or_insert(r"\btest\b").is_err());
        assert!(cache.get_or_compile(r"\btest\b").is_err());

        let scorer = RelevanceScorer::new_with_cache("rust", Some(cache));
        // Should not panic; falls back to contains() which matches "rusting" too
        let score = scorer.calculate_score("rusting away", None, "https://x.io");
        // With fallback, "rust" is contained in "rusting" so we still get word_boundary bonus
        assert!(score > 0.0, "fallback contains() should still match");
    }

    // ---- is_authoritative_domain (exercised via calculate_score) ----

    #[test]
    fn test_authoritative_domain_various() {
        let scorer = RelevanceScorer::new("rust");
        // Use a title >= 10 chars to avoid the short-title penalty (0.8 multiplier)
        // Each authoritative domain should yield 0.5 more than a plain domain
        let plain = scorer.calculate_score("Long Title Here", None, "https://example.com/rust");
        for url in [
            "https://wikipedia.org/rust",
            "https://github.com/rust",
            "https://stackoverflow.com/rust",
            "https://medium.com/rust",
            "https://reddit.com/rust",
            "https://quora.com/rust",
            "https://example.gov/rust",
            "https://example.edu/rust",
            "https://example.org/rust",
        ] {
            let score = scorer.calculate_score("Long Title Here", None, url);
            assert!(
                (score - plain - 0.5).abs() < 1e-9,
                "expected authority bonus 0.5 for {} (score={}, plain={}, diff={})",
                url,
                score,
                plain,
                score - plain
            );
        }
    }

    // ---- DateParserComponent ----

    #[test]
    fn test_date_parser_default_equals_new() {
        let default = DateParserComponent::default();
        let new = DateParserComponent::new();
        // Both should parse the same input identically
        let input = "2024-01-15T10:30:00Z";
        assert_eq!(
            default.extract_date(input).map(|d| d.timestamp()),
            new.extract_date(input).map(|d| d.timestamp())
        );
    }

    #[test]
    fn test_date_parser_with_defaults_factory() {
        let parser = DateParserComponent::with_defaults();
        assert!(parser.extract_date("2024-01-15").is_some());
    }

    #[test]
    fn test_extract_date_iso8601_with_millis() {
        let parser = DateParserComponent::new();
        assert!(parser.extract_date("2024-01-15T10:30:00.123Z").is_some());
    }

    #[test]
    fn test_extract_date_simple_date() {
        let parser = DateParserComponent::new();
        let date = parser
            .extract_date("event on 2024-03-20 happened")
            .expect("date should parse");
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2024-03-20");
    }

    #[test]
    fn test_extract_date_relative_time_hours() {
        let parser = DateParserComponent::new();
        let before = Utc::now();
        let date = parser
            .extract_date("posted 5 hours ago")
            .expect("relative hours should parse");
        let after = Utc::now();
        // Should be roughly 5 hours before now
        let approx = Utc::now() - Duration::hours(5);
        assert!((date.timestamp() - approx.timestamp()).abs() < 10);
        assert!(date >= before - Duration::hours(5) - Duration::seconds(10));
        assert!(date <= after - Duration::hours(5) + Duration::seconds(10));
    }

    #[test]
    fn test_extract_date_relative_time_days() {
        let parser = DateParserComponent::new();
        let date = parser
            .extract_date("3 days ago")
            .expect("relative days should parse");
        let approx = Utc::now() - Duration::days(3);
        assert!((date.timestamp() - approx.timestamp()).abs() < 10);
    }

    #[test]
    fn test_extract_date_relative_time_weeks_months_years() {
        let parser = DateParserComponent::new();
        assert!(parser.extract_date("1 week ago").is_some());
        assert!(parser.extract_date("2 months ago").is_some());
        assert!(parser.extract_date("1 year ago").is_some());
    }

    #[test]
    fn test_extract_date_month_name_format() {
        let parser = DateParserComponent::new();
        let date_short = parser
            .extract_date("Jan 15, 2024")
            .expect("short month name should parse");
        let date_long = parser
            .extract_date("January 15, 2024")
            .expect("long month name should parse");
        assert_eq!(date_short, date_long);
        assert_eq!(date_short.format("%Y-%m-%d").to_string(), "2024-01-15");
    }

    #[test]
    fn test_extract_date_no_match() {
        let parser = DateParserComponent::new();
        assert!(parser.extract_date("no date here at all").is_none());
        assert!(parser.extract_date("").is_none());
    }

    #[test]
    fn test_extract_date_first_matching_regex_wins() {
        // ISO 8601 regex comes first; ensure it's preferred when both could match
        let parser = DateParserComponent::new();
        let text = "2024-01-15T10:30:00Z and also 2024-02-20";
        let date = parser.extract_date(text).expect("should parse");
        // Should be the ISO date, not the bare date
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2024-01-15");
    }

    // ---- extract_published_date_with_parser ----

    #[test]
    fn test_extract_published_date_with_parser_no_date() {
        let parser = DateParserComponent::new();
        let date = RelevanceScorer::extract_published_date_with_parser("no dates here", &parser);
        assert!(date.is_none());
    }

    // ---- calculate_freshness_score buckets ----

    #[test]
    fn test_freshness_score_within_one_day() {
        let date = Utc::now() - Duration::hours(12);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_within_one_week() {
        let date = Utc::now() - Duration::days(3);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 0.8).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_within_one_month() {
        let date = Utc::now() - Duration::days(15);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_within_six_months() {
        let date = Utc::now() - Duration::days(90);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 0.4).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_within_one_year() {
        let date = Utc::now() - Duration::days(200);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_very_old() {
        let date = Utc::now() - Duration::days(500);
        assert!((RelevanceScorer::calculate_freshness_score(date) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_now_is_one() {
        let date = Utc::now();
        assert!((RelevanceScorer::calculate_freshness_score(date) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_freshness_score_monotonic_decreasing() {
        let now = Utc::now();
        let scores = [
            RelevanceScorer::calculate_freshness_score(now - Duration::hours(1)),
            RelevanceScorer::calculate_freshness_score(now - Duration::days(3)),
            RelevanceScorer::calculate_freshness_score(now - Duration::days(15)),
            RelevanceScorer::calculate_freshness_score(now - Duration::days(90)),
            RelevanceScorer::calculate_freshness_score(now - Duration::days(200)),
            RelevanceScorer::calculate_freshness_score(now - Duration::days(500)),
        ];
        for w in scores.windows(2) {
            assert!(
                w[0] >= w[1],
                "freshness should be non-increasing as content ages"
            );
        }
    }

    // ---- RelevanceScorerError From impls ----

    #[test]
    fn test_relevance_scorer_error_from_string() {
        let err: RelevanceScorerError = "bad regex".to_string().into();
        match err {
            RelevanceScorerError::RegexError(msg) => assert_eq!(msg, "bad regex"),
            other => panic!("expected RegexError, got {:?}", other),
        }
    }

    #[test]
    fn test_relevance_scorer_error_from_str() {
        let err: RelevanceScorerError = "bad regex".into();
        match err {
            RelevanceScorerError::RegexError(msg) => assert_eq!(msg, "bad regex"),
            other => panic!("expected RegexError, got {:?}", other),
        }
    }

    #[test]
    fn test_relevance_scorer_error_from_anyhow() {
        let err: RelevanceScorerError = anyhow::anyhow!("anyhow failure").into();
        match err {
            RelevanceScorerError::RegexError(msg) => assert!(msg.contains("anyhow failure")),
            other => panic!("expected RegexError, got {:?}", other),
        }
    }

    #[test]
    fn test_relevance_scorer_error_cache_lock_variant() {
        let err = RelevanceScorerError::CacheLockError("locked".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Regex cache lock error"));
        assert!(msg.contains("locked"));
    }

    #[test]
    fn test_relevance_scorer_error_display_regex() {
        let err = RelevanceScorerError::RegexError("compile failed".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Regex compilation error"));
        assert!(msg.contains("compile failed"));
    }
}
