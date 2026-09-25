// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Common HTML parser for search engine results
//! Provides reusable parsing logic for different search engines

use super::{ResponseItem, SearchEngineType};
use once_cell::sync::Lazy;
use scraper::{ElementRef, Html, Selector};

use super::shared_utils::{escape_html_text, safe_parse_selector};

/// Common HTML result parser for search engines
pub struct HtmlParser {
    // Compiled selectors for reuse
    title_selector: Selector,
    link_selector: Selector,
    snippet_selectors: Vec<Selector>,
    result_selectors: Vec<Selector>,
}

/// Google 风格 parser 全局单例（性能 PERF-05）
///
/// 使用 `once_cell::sync::Lazy` 在首次访问时构造一次，避免每次
/// `BaiduSearchEngine::new`/`BingSearchEngine::new` 都重复解析 4-8 个 CSS 选择器。
/// `Selector::parse` 内部涉及 CSS tokenizer + AST 构造，是已知昂贵操作。
static GOOGLE_PARSER: Lazy<HtmlParser> = Lazy::new(|| {
    HtmlParser::new(
        vec!["div.g", "div[data-hveid]", "div:has(> a > h3)"],
        "h3",
        "a[href]",
        vec!["[data-sncf]", "span.st", "div[class*='snippet']"],
    )
});

/// Bing 风格 parser 全局单例（性能 PERF-05）
static BING_PARSER: Lazy<HtmlParser> =
    Lazy::new(|| HtmlParser::new(vec!["li.b_algo"], "h2", "a[href]", vec!["p"]));

/// Baidu 风格 parser 全局单例（性能 PERF-05）
static BAIDU_PARSER: Lazy<HtmlParser> = Lazy::new(|| {
    HtmlParser::new(
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
});

/// Sogou 风格 parser 全局单例（性能 PERF-05）
static SOGOU_PARSER: Lazy<HtmlParser> =
    Lazy::new(|| HtmlParser::new(vec![".vrwrap", ".rb"], "h3", "h3 > a", vec![]));

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

    /// 获取 Google 风格 parser 全局单例（性能 PERF-05）
    ///
    /// 替代原 `for_google()` 构造函数，返回 `&'static Self` 避免重复构造。
    pub fn google() -> &'static Self {
        &GOOGLE_PARSER
    }

    /// 获取 Bing 风格 parser 全局单例（性能 PERF-05）
    pub fn bing() -> &'static Self {
        &BING_PARSER
    }

    /// 获取 Baidu 风格 parser 全局单例（性能 PERF-05）
    pub fn baidu() -> &'static Self {
        &BAIDU_PARSER
    }

    /// 获取 Sogou 风格 parser 全局单例（性能 PERF-05）
    pub fn sogou() -> &'static Self {
        &SOGOU_PARSER
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
            let mut url = String::new();
            if let Some(a) = element.select(&self.link_selector).next() {
                let raw_href = a.value().attr("href").unwrap_or("");
                if !raw_href.is_empty() {
                    url = if engine == SearchEngineType::Baidu {
                        self.resolve_baidu_url(raw_href, &element)
                    } else {
                        Self::clean_url(raw_href)
                    };
                }
            }

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

    /// 解析百度结果锚点的最终 URL（仅 `SearchEngineType::Baidu` 路径调用）。
    ///
    /// 百度桌面版结果 href 常为 `www.baidu.com/link?url=<id>` 中转形态，真实
    /// URL 有时由结果块内锚点的 `mu`、`data-url` 属性携带。按成本分层解包：
    /// 1. 中转形态时优先读 `mu` → `data-url` 属性，取首个 http(s) 值（零网络成本）；
    /// 2. 属性不可得时保留中转链接，不做网络跟随：`EngineClient::scrape` 是
    ///    异步全页抓取语义（跟随 302 只会拿到目标页 HTML，无法单独取得
    ///    Location 头），而 parse 路径为同步函数——逐条跟随中转意味着对每个
    ///    外部站点发起完整抓取，成本与反爬风险不成比例。保留原链接保证结果
    ///    不丢失，仅跨引擎去重时该条退化为未命中（tn=json 兜底路径返回的
    ///    是真实 URL，可部分弥补）。
    fn resolve_baidu_url(&self, raw_href: &str, element: &ElementRef) -> String {
        let is_redirect =
            raw_href.starts_with("/link?url=") || raw_href.contains("baidu.com/link?url=");
        if !is_redirect {
            return Self::clean_url(raw_href);
        }

        for attr in ["mu", "data-url"] {
            if let Some(real_url) = element
                .select(&self.link_selector)
                .filter_map(|a| a.value().attr(attr))
                .map(str::trim)
                .find(|v| v.starts_with("http"))
            {
                return real_url.to_string();
            }
        }

        if raw_href.starts_with("/link?url=") {
            // 相对中转链接补全主机，避免进入 clean_url 被误加 google 前缀
            return format!("https://www.baidu.com{}", raw_href);
        }
        raw_href.to_string()
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
    /// 委托到 `shared_utils::escape_html_text`（架构统一 XSS 防护原语）
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
        use rand::RngExt;
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
        // 测试协议相对 URL 不被添加 google 前缀
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
        // 确保原始的 "& " (后跟空格的裸 &) 不存在
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
        let parser = HtmlParser::google();
        let results = parser.parse("", SearchEngineType::Bing);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_google_valid_results() {
        // 测试从有效 Google HTML 解析结果
        let parser = HtmlParser::google();
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

        let results = parser.parse(html, SearchEngineType::Bing);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "First snippet text");
        assert_eq!(results[0].engine, SearchEngineType::Bing);
    }

    #[test]
    fn test_parse_no_matching_selectors_returns_empty() {
        // 边界情况：HTML 不包含匹配的选择器时返回空
        let parser = HtmlParser::google();
        let html = r#"<html><body><div>nothing here</div></body></html>"#;
        let results = parser.parse(html, SearchEngineType::Bing);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_skips_results_without_title() {
        // 边界情况：缺少标题的结果被跳过
        let parser = HtmlParser::google();
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

        let results = parser.parse(html, SearchEngineType::Bing);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid Result");
    }

    #[test]
    fn test_parse_skips_non_http_urls() {
        // 边界情况：非 http 开头的 URL 被跳过
        let parser = HtmlParser::google();
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

        let results = parser.parse(html, SearchEngineType::Bing);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/valid");
    }

    #[test]
    fn test_parse_deduplicates_by_url() {
        // 测试相同 URL 的结果被去重
        let parser = HtmlParser::google();
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

        let results = parser.parse(html, SearchEngineType::Bing);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Title");
        assert_eq!(results[1].title, "Unique Title");
    }

    #[test]
    fn test_parse_bing_results() {
        // 测试 Bing 解析器从有效 HTML 提取结果
        let parser = HtmlParser::bing();
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
        let parser = HtmlParser::baidu();
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

    // ========== 百度中转链接（link?url=）解包测试 ==========

    #[test]
    fn test_parse_baidu_redirect_unwraps_mu_attribute() {
        // 锚点/结果块带 mu 属性时解包出真实 URL（零网络成本路径）
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://www.baidu.com/link?url=abc123" mu="https://example.com/real">Baidu Result</a></h3>
                <div class="c-abstract">snippet</div>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1, "unwrapped result must not be dropped");
        assert_eq!(results[0].url, "https://example.com/real");
        assert_eq!(results[0].title, "Baidu Result");
    }

    #[test]
    fn test_parse_baidu_redirect_unwraps_mu_on_sibling_anchor() {
        // mu 属性不在标题锚上而在结果块内其他锚点时同样可解包（sogou data-url 先例模式）
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://www.baidu.com/link?url=abc123">Baidu Result</a></h3>
                <a href="https://www.baidu.com/link?url=abc123" mu="https://example.com/sibling">site</a>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/sibling");
    }

    #[test]
    fn test_parse_baidu_redirect_falls_back_to_data_url() {
        // 无 mu 属性时回退到 data-url 属性
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://www.baidu.com/link?url=abc123" data-url="https://example.com/from-data-url">Baidu Result</a></h3>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/from-data-url");
    }

    #[test]
    fn test_parse_baidu_redirect_kept_when_no_attributes() {
        // 无任何真实 URL 属性：保留中转链接（不跟随 302，不丢结果）
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://www.baidu.com/link?url=abc123">Baidu Result</a></h3>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1, "redirect link must not be dropped");
        assert_eq!(results[0].url, "https://www.baidu.com/link?url=abc123");
    }

    #[test]
    fn test_parse_baidu_redirect_ignores_non_http_attribute() {
        // 属性值非 http(s) 开头不可信：回退保留中转链接
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://www.baidu.com/link?url=abc123" mu="javascript:alert(1)">Baidu Result</a></h3>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.baidu.com/link?url=abc123");
    }

    #[test]
    fn test_parse_baidu_relative_redirect_gets_baidu_host() {
        // 相对形态 /link?url= 补全为百度主机，不被 clean_url 误加 google 前缀
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="/link?url=abc123">Baidu Result</a></h3>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.baidu.com/link?url=abc123");
    }

    #[test]
    fn test_parse_baidu_normal_url_untouched() {
        // 普通真实 URL 不受解包逻辑影响
        let parser = HtmlParser::baidu();
        let html = r#"
        <html><body>
            <div class="c-container">
                <h3><a href="https://example.com/plain">Baidu Result</a></h3>
            </div>
            <div class="c-container">
                <h3><a href="http://example.org/another" mu="https://ignored.example/">Baidu Result 2</a></h3>
            </div>
        </body></html>
        "#;

        let results = parser.parse(html, SearchEngineType::Baidu);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://example.com/plain");
        assert_eq!(results[1].url, "http://example.org/another");
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
