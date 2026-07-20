// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Common HTML parser for search engine results
//! Provides reusable parsing logic for different search engines

use super::{ResponseItem, SearchEngineType};
use scraper::{Html, Selector};

use super::shared_utils::{escape_html_text, safe_parse_selector};

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
        let mut results = Vec::with_capacity(20);

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
                title: escape_html_text(&title),
                url,
                description: escape_html_text(&description),
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
    ///
    /// 委托到 `shared_utils::escape_html_text`（架构 MEDIUM 4：统一 XSS 防护原语）
    pub fn escape_html(text: &str) -> String {
        escape_html_text(text)
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

    // ========== clean_url 边界情况补充 ==========

    #[test]
    fn test_clean_url_google_url_no_extra_params() {
        // 测试 /url?q= 后没有额外参数的情况
        let url = "/url?q=https://example.com";
        assert_eq!(HtmlParser::clean_url(url), "https://example.com");
    }

    #[test]
    fn test_clean_url_protocol_relative_not_prefixed() {
        // 测试协议相对 URL (//) 不被添加 google 前缀
        let url = "//example.com/path";
        assert_eq!(HtmlParser::clean_url(url), "//example.com/path");
    }

    #[test]
    fn test_clean_url_empty_string() {
        // 边界情况：空字符串返回空字符串
        assert_eq!(HtmlParser::clean_url(""), "");
    }

    #[test]
    fn test_clean_url_google_url_with_multiple_params() {
        // 测试 /url?q= 后跟多个参数，只提取第一个
        let url = "/url?q=https://example.com&sa=t&ved=123";
        assert_eq!(HtmlParser::clean_url(url), "https://example.com");
    }

    // ========== escape_html 补充测试 ==========

    #[test]
    fn test_escape_html_plain_text_unchanged() {
        // 测试普通文本不被修改
        let text = "Hello World Rust Programming";
        assert_eq!(
            HtmlParser::escape_html(text),
            "Hello World Rust Programming"
        );
    }

    #[test]
    fn test_escape_html_special_chars_encoded() {
        // 测试 HTML 特殊字符 & < > 被编码（encode_text 不编码引号）
        let text = "<div>a & b</div>";
        let escaped = HtmlParser::escape_html(text);
        assert!(!escaped.contains('<'), "should not contain raw <");
        assert!(!escaped.contains('>'), "should not contain raw >");
        assert!(escaped.contains("&lt;"), "should contain &lt;");
        assert!(escaped.contains("&gt;"), "should contain &gt;");
        assert!(escaped.contains("&amp;"), "should contain &amp;");
        // 确保原始的 "& " (后跟空格的裸 & ) 不存在
        assert!(
            !escaped.contains("& "),
            "should not contain raw & followed by space"
        );
    }

    #[test]
    fn test_escape_html_empty_string() {
        // 边界情况：空字符串返回空字符串
        assert_eq!(HtmlParser::escape_html(""), "");
    }

    #[test]
    fn test_escape_html_trims_whitespace() {
        // 测试首尾空白被 trim
        let text = "  content with spaces  ";
        assert_eq!(HtmlParser::escape_html(text), "content with spaces");
    }

    // ========== parse 补充测试 ==========

    #[test]
    fn test_parse_empty_html_returns_empty() {
        // 边界情况：空 HTML 返回空结果
        let parser = HtmlParser::for_google();
        let results = parser.parse("", SearchEngineType::Google);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_google_valid_results() {
        // 测试从有效 Google HTML 解析结果
        let parser = HtmlParser::for_google();
        let html = r#"
        <html><body>
            <div class="g">
                <a href="https://example.com/1"><h3>First Result</h3></a>
                <span class="st">First snippet text</span>
            </div>
            <div class="g">
                <a href="https://example.com/2"><h3>Second Result</h3></a>
                <span class="st">Second snippet text</span>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Google);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "First snippet text");
        assert_eq!(results[0].engine, SearchEngineType::Google);
    }

    #[test]
    fn test_parse_no_matching_selectors_returns_empty() {
        // 边界情况：HTML 不包含匹配的选择器时返回空
        let parser = HtmlParser::for_google();
        let html = r#"<html><body><div>nothing here</div></body></html>"#;
        let results = parser.parse(html, SearchEngineType::Google);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_skips_results_without_title() {
        // 边界情况：缺少标题的结果被跳过
        let parser = HtmlParser::for_google();
        let html = r#"
        <html><body>
            <div class="g">
                <a href="https://example.com/1"></a>
            </div>
            <div class="g">
                <a href="https://example.com/2"><h3>Valid Result</h3></a>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Google);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid Result");
    }

    #[test]
    fn test_parse_skips_non_http_urls() {
        // 边界情况：非 http 开头的 URL 被跳过
        let parser = HtmlParser::for_google();
        let html = r#"
        <html><body>
            <div class="g">
                <a href="javascript:void(0)"><h3>JS Link</h3></a>
            </div>
            <div class="g">
                <a href="mailto:test@example.com"><h3>Mail Link</h3></a>
            </div>
            <div class="g">
                <a href="https://example.com/valid"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Google);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/valid");
    }

    #[test]
    fn test_parse_deduplicates_by_url() {
        // 测试相同 URL 的结果被去重
        let parser = HtmlParser::for_google();
        let html = r#"
        <html><body>
            <div class="g">
                <a href="https://example.com/dup"><h3>First Title</h3></a>
            </div>
            <div class="g">
                <a href="https://example.com/dup"><h3>Duplicate Title</h3></a>
            </div>
            <div class="g">
                <a href="https://example.com/unique"><h3>Unique Title</h3></a>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Google);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Title");
        assert_eq!(results[1].title, "Unique Title");
    }

    #[test]
    fn test_parse_bing_results() {
        // 测试 Bing 解析器从有效 HTML 提取结果
        let parser = HtmlParser::for_bing();
        let html = r#"
        <html><body>
            <li class="b_algo">
                <h2><a href="https://example.com/1">Bing Result</a></h2>
                <p>Bing snippet</p>
            </li>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Bing);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Bing Result");
        assert_eq!(results[0].engine, SearchEngineType::Bing);
    }

    #[test]
    fn test_parse_baidu_results() {
        // 测试百度解析器从有效 HTML 提取结果
        let parser = HtmlParser::for_baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://example.com/1">Baidu Result</a></h3>
                <div class="c-abstract">Baidu snippet</div>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Baidu Result");
        assert_eq!(results[0].engine, SearchEngineType::Baidu);
    }

    // ========== UserAgentManager 补充测试 ==========

    #[test]
    fn test_user_agent_manager_round_robin_wraps_around() {
        // 测试轮询模式在遍历完所有 UA 后从头开始
        let mut manager = UserAgentManager::new();
        let ua_count = 5; // UserAgentManager::new 内置 5 个 UA

        // 收集 ua_count + 1 次调用，第 ua_count+1 次应等于第一次
        let mut uas = Vec::new();
        for _ in 0..(ua_count + 1) {
            uas.push(manager.get().to_string());
        }

        // 所有 UA 非空
        for ua in &uas {
            assert!(!ua.is_empty());
        }
        // 第 ua_count+1 次应等于第一次（轮询回绕）
        assert_eq!(uas[ua_count], uas[0], "round-robin should wrap around");
    }

    #[test]
    fn test_user_agent_manager_get_random_returns_valid_ua() {
        // 测试 get_random 返回非空的 UA
        let manager = UserAgentManager::new();
        let ua = manager.get_random();
        assert!(!ua.is_empty());
        assert!(ua.contains("Mozilla"), "UA should look like a browser UA");
    }

    #[test]
    fn test_user_agent_manager_default_equals_new() {
        // 测试 Default trait 等价于 new
        let mut m1 = UserAgentManager::new();
        let mut m2 = UserAgentManager::default();
        // 两者应返回相同的第一个 UA
        assert_eq!(m1.get(), m2.get());
    }
}
