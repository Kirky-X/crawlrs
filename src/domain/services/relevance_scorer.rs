// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 相关性评分服务
//!
//! 提供搜索结果相关性评分功能，支持通过 DI 注入自定义日期解析器。

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use shaku::{Component, Interface};
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
pub trait DateParserTrait: Interface + Send + Sync {
    /// 从文本中提取日期
    fn extract_date(&self, text: &str) -> Option<DateTime<Utc>>;
}

/// 默认日期解析器组件
///
/// 预编译了常用日期格式的正则表达式。
#[derive(Component)]
#[shaku(interface = DateParserTrait)]
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
}
