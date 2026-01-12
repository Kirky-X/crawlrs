// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Common HTML parser for search engine results
//! Provides reusable parsing logic for different search engines

use super::{ResponseItem, SearchEngineType};
use scraper::{Html, Selector};

/// 安全解析CSS选择器，如果解析失败则返回None
fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// Common HTML result parser for search engines
pub struct HtmlParser {
    // Compiled selectors for reuse
    title_selector: Selector,
    link_selector: Selector,
    snippet_selectors: Vec<Selector>,
    result_selectors: Vec<Selector>,
}

impl HtmlParser {
    /// Create a new parser with engine-specific selectors
    pub fn new(
        result_selectors: Vec<&str>,
        title_selector: &str,
        link_selector: &str,
        snippet_selectors: Vec<&str>,
    ) -> Self {
        Self {
            title_selector: safe_parse_selector(title_selector).unwrap_or_else(|| {
                safe_parse_selector("h3").expect("Failed to parse fallback title selector")
            }),
            link_selector: safe_parse_selector(link_selector).unwrap_or_else(|| {
                safe_parse_selector("a[href]").expect("Failed to parse fallback link selector")
            }),
            snippet_selectors: snippet_selectors
                .iter()
                .map(|s| {
                    safe_parse_selector(s).unwrap_or_else(|| {
                        safe_parse_selector("p").expect("Failed to parse fallback snippet selector")
                    })
                })
                .collect(),
            result_selectors: result_selectors
                .iter()
                .map(|s| {
                    safe_parse_selector(s).unwrap_or_else(|| {
                        safe_parse_selector("div")
                            .expect("Failed to parse fallback result selector")
                    })
                })
                .collect(),
        }
    }

    /// Create parser for Google-style results
    pub fn for_google() -> Self {
        Self::new(
            vec!["div.g", "div[data-hveid]", "div:has(> a > h3)"],
            "h3",
            "a[href]",
            vec!["[data-sncf]", "span.st", "div[class*='snippet']"],
        )
    }

    /// Create parser for Bing-style results
    pub fn for_bing() -> Self {
        Self::new(vec!["li.b_algo"], "h2", "a[href]", vec!["p"])
    }

    /// Create parser for Baidu-style results
    pub fn for_baidu() -> Self {
        Self::new(
            vec![
                "div.c-container",
                "div.result",
                "div.result-op",
                ".c-container",
            ],
            "h3",
            "a",
            vec!["div.c-abstract", ".c-abstract", ".c-span18"],
        )
    }

    /// Create parser for Sogou-style results
    pub fn for_sogou() -> Self {
        Self::new(vec![".vrwrap", ".rb"], "h3", "h3 > a", vec![])
    }

    /// Parse HTML content and extract search results
    pub fn parse(&self, html: &str, engine: SearchEngineType) -> Vec<ResponseItem> {
        let document = Html::parse_document(html);
        let mut results = Vec::new();

        // Try each result selector until we find matches
        let result_elements = self
            .result_selectors
            .iter()
            .find_map(|s| {
                let elements: Vec<_> = document.select(s).collect();
                if !elements.is_empty() {
                    Some(elements)
                } else {
                    None
                }
            })
            .unwrap_or_default();

        for element in result_elements {
            // Extract title
            let title = element
                .select(&self.title_selector)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if title.is_empty() {
                continue;
            }

            // Extract URL
            let url = {
                let mut found_url = String::new();
                if let Some(a) = element.select(&self.link_selector).next() {
                    if let Some(href) = a.value().attr("href") {
                        if !href.is_empty() {
                            found_url = Self::clean_url(href);
                        }
                    }
                }
                found_url
            };

            if url.is_empty() || !url.starts_with("http") {
                continue;
            }

            // Extract snippet
            let description = self
                .snippet_selectors
                .iter()
                .find_map(|s| {
                    element
                        .select(s)
                        .next()
                        .map(|e| e.text().collect::<String>().trim().to_string())
                        .filter(|t| !t.is_empty())
                })
                .unwrap_or_default();

            // Deduplicate by URL
            if results.iter().any(|r: &ResponseItem| r.url == url) {
                continue;
            }

            results.push(ResponseItem {
                title: Self::escape_html(&title),
                url,
                description: Self::escape_html(&description),
                engine,
            });
        }

        results
    }

    /// Clean and normalize URLs
    pub fn clean_url(url: &str) -> String {
        if url.starts_with("/url?q=") {
            url.trim_start_matches("/url?q=")
                .split('&')
                .next()
                .unwrap_or(url)
                .to_string()
        } else if url.starts_with("/") && !url.starts_with("//") {
            format!("https://www.google.com{}", url)
        } else {
            url.to_string()
        }
    }

    /// Escape HTML entities to prevent XSS
    pub fn escape_html(text: &str) -> String {
        html_escape::encode_text(text).trim().to_string()
    }
}

/// User agent manager for rotating user agents
pub struct UserAgentManager {
    // Cache for user agents
    user_agents: Vec<String>,
    current_index: usize,
}

impl UserAgentManager {
    /// Create a new manager with random user agents
    pub fn new() -> Self {
        let user_agents = vec![
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0".to_string(),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15".to_string(),
        ];

        Self {
            user_agents,
            current_index: 0,
        }
    }

    /// Get next user agent (round-robin)
    pub fn get(&mut self) -> &str {
        let ua = &self.user_agents[self.current_index];
        self.current_index = (self.current_index + 1) % self.user_agents.len();
        ua
    }

    /// Get random user agent
    pub fn get_random(&self) -> &str {
        use rand::Rng;
        let mut rng = rand::rng();
        let index = rng.random_range(0..self.user_agents.len());
        &self.user_agents[index]
    }
}

impl Default for UserAgentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_url_google() {
        let url = "/url?q=https://example.com&sa=t";
        assert_eq!(HtmlParser::clean_url(url), "https://example.com");
    }

    #[test]
    fn test_clean_url_normal() {
        let url = "https://example.com";
        assert_eq!(HtmlParser::clean_url(url), "https://example.com");
    }

    #[test]
    fn test_clean_url_relative() {
        let url = "/path/to/page";
        assert_eq!(
            HtmlParser::clean_url(url),
            "https://www.google.com/path/to/page"
        );
    }

    #[test]
    fn test_escape_html() {
        let text = "&lt;script&gt;alert(1)&lt;/script&gt;";
        let escaped = HtmlParser::escape_html(text);
        assert!(!escaped.contains("<script>"));
    }

    #[test]
    fn test_user_agent_manager() {
        let mut manager = UserAgentManager::new();
        let ua1 = manager.get().to_string();
        let ua2 = manager.get().to_string();
        assert!(!ua1.is_empty());
        assert!(!ua2.is_empty());
    }
}
