// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

type DateParser = fn(&str) -> Option<DateTime<Utc>>;

static DATE_REGEXES: Lazy<Vec<(Regex, DateParser)>> = Lazy::new(|| {
    vec![
        // ISO 8601 format: 2024-01-15T10:30:00Z
        (Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{3})?Z").unwrap(), |s| {
            DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
        }),
        // Date format: 2024-01-15
        (Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap(), |s| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
                .map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc())
        }),
        // Relative time: 2 hours ago, 3 days ago, 1 week ago
        (Regex::new(r"(\d+)\s+(hour|hours|day|days|week|weeks|month|months|year|years)\s+ago").unwrap(), |s| {
            let captures = Regex::new(r"(\d+)\s+(hour|hours|day|days|week|weeks|month|months|year|years)").unwrap()
                .captures(s)?;
            let num: i64 = captures.get(1)?.as_str().parse().ok()?;
            let unit = captures.get(2)?.as_str();

            let duration = match unit {
                "hour" | "hours" => Duration::hours(num),
                "day" | "days" => Duration::days(num),
                "week" | "weeks" => Duration::weeks(num),
                "month" | "months" => Duration::days(num * 30), // Approximation
                "year" | "years" => Duration::days(num * 365), // Approximation
                _ => return None,
            };

            Some(Utc::now() - duration)
        }),
        // Common formats: Jan 15, 2024, January 15, 2024
        (Regex::new(r"(Jan|January|Feb|February|Mar|March|Apr|April|May|Jun|June|Jul|July|Aug|August|Sep|September|Oct|October|Nov|November|Dec|December)\s+(\d{1,2}),?\s+(\d{4})").unwrap(), |s| {
            chrono::NaiveDate::parse_from_str(s, "%b %d, %Y").ok()
                .or_else(|| chrono::NaiveDate::parse_from_str(s, "%B %d, %Y").ok())
                .map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc())
        }),
    ]
});

pub struct RelevanceScorer {
    query_terms: Vec<String>,
    term_weights: HashMap<String, f64>,
}

impl RelevanceScorer {
    pub fn new(query: &str) -> Self {
        let query_lower = query.to_lowercase();
        let terms: Vec<String> = query_lower
            .split_whitespace()
            .filter(|term| term.len() > 2) // Filter out very short terms
            .map(|s| s.to_string())
            .collect();

        // Calculate TF-IDF-like weights for query terms
        let mut term_weights = HashMap::new();
        let total_terms = terms.len() as f64;

        for term in &terms {
            let count = terms.iter().filter(|t| t == &term).count() as f64;
            let tf = count / total_terms;
            // Simple IDF approximation (logarithmic scale)
            let idf = (1.0 + total_terms / count).ln();
            term_weights.insert(term.clone(), tf * idf);
        }

        Self {
            query_terms: terms,
            term_weights,
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
        let pattern = format!(r"\b{}\b", regex::escape(term));
        Regex::new(&pattern).unwrap().is_match(text)
    }

    /// Check if domain is authoritative (simplified heuristic)
    fn is_authoritative_domain(&self, url: &str) -> bool {
        let authoritative_domains = vec![
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

    /// Extract publication date from text
    pub fn extract_published_date(text: &str) -> Option<DateTime<Utc>> {
        for (regex, parser) in DATE_REGEXES.iter() {
            if let Some(captures) = regex.find(text) {
                if let Some(date) = parser(captures.as_str()) {
                    return Some(date);
                }
            }
        }
        None
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
    fn test_date_extraction() {
        let text = "Published on 2024-01-15T10:30:00Z, updated 2 days ago";
        let date = RelevanceScorer::extract_published_date(text);
        assert!(date.is_some());

        let text2 = "Posted January 15, 2024";
        let date2 = RelevanceScorer::extract_published_date(text2);
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
