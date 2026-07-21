// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{EngineClient, ScrapeRequest as EngineScrapeRequest};
use crate::search::{
    engine_trait::SearchEngine,
    error::SearchError,
    response::{Response, ResponseItem},
    types::{EngineHealth, SearchEngineType},
    SearchRequest,
};
use async_trait::async_trait;
use chrono::Utc;
use log::{info, warn};
use once_cell::sync::Lazy;
use rand::RngExt;
use scraper::Html;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::shared_utils::{build_query_string, escape_html_text, safe_parse_selector};

/// Google CONSENT cookie 值（绕过 EU 同意重定向）
///
/// 参考: temp/searxng/searx/engines/google.py line 274
/// （架构 MEDIUM-4：提取为常量，避免硬编码在请求构造中）
const GOOGLE_CONSENT_COOKIE: &str = "CONSENT=YES+";

/// Google 搜索请求的静态 HTTP 头
///
/// 这些头在所有请求中都是相同的，使用 `Lazy` 只构造一次 HashMap 结构（capacity + buckets）。
///
/// 性能 LOW-1（注释修正）：`headers()` 方法接受 `HashMap` 所有权，每次调用必须 clone。
/// `HashMap::clone` 会克隆所有 entries，3 个 (String, String) 对等于 6 个 String 分配，
/// 与每次新构造 `HashMap::with_capacity(3)` + 3 次 insert 等价。
/// 之前的注释声称 "避免每次构造 6 个 String" 是错误的 — clone 不省分配，只省 capacity 计算。
/// 真正的优化需要修改 `ScrapeOptions::headers` 接口为 `&HashMap` 或共享 `Arc<HashMap>`。
static GOOGLE_STATIC_HEADERS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    let mut map = HashMap::with_capacity(3);
    map.insert("Accept".to_string(), "*/*".to_string());
    map.insert("Accept-Language".to_string(), "en-US,en;q=0.9".to_string());
    map.insert("Cookie".to_string(), GOOGLE_CONSENT_COOKIE.to_string());
    map
});

/// Google bot-protection 检测的 OR 模式
///
/// 任一模式命中即判定为 CAPTCHA/sorry 页面，返回 RateLimited。
/// 与下方 NOSCRIPT_AND_PATTERNS 分离是因为 AND/OR 语义不同：
/// - 这里是 OR（任一匹配即 RateLimited）
/// - noscript 是 AND（两个都存在才判定）
///
/// 性能 LOW-2（注释修正 + 回退）：之前用 AhoCorasick 一次扫描替代 2 次 contains，
/// 但 AhoCorasick 对 2 个短模式有状态机常数开销，无 benchmark 证据证明更优。
/// 架构 LOW-3（注释修正）：删除"通常更快"的无证据断言 — `str::contains` 在 memchr
/// SIMD 优化下对小模式快速，但本仓库未做 benchmark，不能断言哪种更快。
/// 2 次 `str::contains` 代码更直观，且无证据表明 AhoCorasick 对 2 个短模式更优。
/// 已删除 aho-corasick 直接依赖（regex 仍间接引入）。
const GOOGLE_BOT_PROTECTION_OR_PATTERNS: [&str; 2] = ["/sorry/", "sorry.google.com"];

/// Google Search Engine implementation
struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

/// Google Search Engine implementation
pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
    engine_client: Arc<EngineClient>,
}

impl GoogleSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>) -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
            engine_client,
        }
    }

    /// Generate 23-character random ARC_ID
    fn generate_random_id() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
        let mut rng = rand::rng();
        (0..23)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// Get ARC_ID (auto-refreshes every hour)
    ///
    /// 性能 MEDIUM-2：使用读锁优先 + 双重检查模式，避免每次请求都获取写锁。
    /// 快速路径（缓存未过期）：仅获取读锁 → 无竞争 → 直接返回。
    /// 慢速路径（缓存过期）：先释放读锁，再获取写锁，双重检查防止重复刷新。
    pub async fn get_arc_id(&self, start_offset: usize) -> String {
        // 快速路径：尝试读锁（缓存未过期时直接返回，无写锁竞争）
        {
            let cache = self.arc_id_cache.read().await;
            let now = Utc::now().timestamp();
            if now - cache.generated_at <= 3600 {
                return format!(
                    "arc_id:srp_{}_1{:02},use_ac:true,_fmt:prog",
                    cache.arc_id, start_offset
                );
            }
        }

        // 慢速路径：缓存已过期，获取写锁刷新
        let mut cache = self.arc_id_cache.write().await;
        let now = Utc::now().timestamp();
        // 双重检查：在等待写锁期间，其他线程可能已刷新缓存
        if now - cache.generated_at > 3600 {
            cache.arc_id = Self::generate_random_id();
            cache.generated_at = now;
            info!("Google ARC_ID refreshed: {}", cache.arc_id);
        }

        format!(
            "arc_id:srp_{}_1{:02},use_ac:true,_fmt:prog",
            cache.arc_id, start_offset
        )
    }

    pub async fn force_refresh_arc_id(&self) {
        let mut cache = self.arc_id_cache.write().await;
        cache.arc_id = Self::generate_random_id();
        cache.generated_at = Utc::now().timestamp();
        info!("Google ARC_ID forcefully refreshed: {}", cache.arc_id);
    }

    /// Parse Google HTML results with XSS protection
    pub fn parse_results(&self, html: &str) -> Result<Vec<ResponseItem>, SearchError> {
        let document = Html::parse_document(html);
        let mut results = Vec::with_capacity(20);
        // 用 HashSet 跟踪已见 URL，O(1) 查找替代 O(n) 线性扫描（性能 MEDIUM：去重 O(n²) → O(n)）
        let mut seen_urls: HashSet<String> = HashSet::new();

        info!("Parsing Google search results...");

        // 根据 temp/search.md 中的逆向工程结果
        // Google 结果包裹在 div[jscontroller*="SC7lYd"] 中
        let result_selector = safe_parse_selector("div[jscontroller*='SC7lYd']")
            .expect("Failed to parse Google selector: div[jscontroller*='SC7lYd']");

        // 标题在 a > h3 中
        let title_selector =
            safe_parse_selector("a h3, h3").expect("Failed to parse Google title selector");

        // URL 从 a 的 href 属性提取
        let link_selector =
            safe_parse_selector("a[href]").expect("Failed to parse Google link selector");

        // 摘要从 div[data-sncf="1"] 中提取
        let snippet_selector = safe_parse_selector("div[data-sncf='1'], div[data-snc]")
            .expect("Failed to parse Google snippet selector");

        // 提取搜索结果块
        for result_element in document.select(&result_selector) {
            // 提取标题
            let title_node = result_element.select(&title_selector).next();
            if title_node.is_none() {
                continue;
            }
            let title = title_node
                .expect("title_node should not be None after is_none() check")
                .text()
                .collect::<String>()
                .trim()
                .to_string();

            if title.is_empty() {
                continue;
            }

            // 提取链接
            let url_node = result_element.select(&link_selector).next();
            if url_node.is_none() {
                continue;
            }
            let mut url = url_node
                .expect("url_node should not be None after is_none() check")
                .value()
                .attr("href")
                .unwrap_or("")
                .to_string();

            if url.is_empty() {
                continue;
            }

            // 清理 URL - 处理 /url?q= 格式
            if url.starts_with("/url?q=") {
                url = url
                    .trim_start_matches("/url?q=")
                    .split('&')
                    .next()
                    .unwrap_or(&url)
                    .to_string();
            } else if url.starts_with("/") && !url.starts_with("//") {
                url = format!("https://www.google.com{}", url);
            }

            if !url.starts_with("http") {
                continue;
            }

            // 提取摘要 - data-sncf="1" 通常包含摘要文本
            let content_nodes = result_element.select(&snippet_selector).next();
            let description = content_nodes
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            // 去重：HashSet::insert 返回 false 表示 URL 已存在
            // O(1) 查找替代之前的 O(n) 线性扫描（results.iter().any）
            if !seen_urls.insert(url.clone()) {
                continue;
            }

            results.push(ResponseItem {
                title: escape_html_text(&title),
                url,
                description: escape_html_text(&description),
                engine: SearchEngineType::Google,
            });

            if results.len() >= 20 {
                break;
            }
        }

        info!(
            "Successfully parsed {} Google search results",
            results.len()
        );
        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    fn name(&self) -> &'static str {
        "Google"
    }
    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Google
    }
    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // Test fallback mode - only enabled in development/test environments
        if std::env::var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS").unwrap_or_default() == "true" {
            // 使用配置服务获取环境，如果不可用则回退到环境变量
            let env = std::env::var("CRAWLRS_ENV")
                .or_else(|_| std::env::var("APP_ENVIRONMENT"))
                .unwrap_or_else(|_| "development".to_string());
            let is_dev_env = matches!(
                env.as_str(),
                "development" | "dev" | "test" | "testing" | ""
            );

            if !is_dev_env {
                log::warn!(
                    "GOOGLE_HTTP_FALLBACK_TEST_RESULTS ignored in production (CRAWLRS_ENV={})",
                    env
                );
            } else {
                log::info!("Using test fallback results for Google search");
                let escaped_query = html_escape::encode_text(&request.query);
                return Ok(Response {
                    items: vec![
                        ResponseItem {
                            title: format!("Test Result 1 for {}", escaped_query),
                            url: "https://google.com/1".to_string(),
                            description: "Test description 1".to_string(),
                            engine: SearchEngineType::Google,
                        },
                        ResponseItem {
                            title: format!("Test Result 2 for {}", escaped_query),
                            url: "https://google.com/2".to_string(),
                            description: "Test description 2".to_string(),
                            engine: SearchEngineType::Google,
                        },
                    ],
                    total_results: Some(2),
                    engine: SearchEngineType::Google,
                });
            }
        }

        let page = 1;
        let start = (page - 1) * request.limit;

        // Build query parameters
        let query_params: Vec<(&str, String)> = vec![
            ("q", request.query.clone()),
            ("ie", "utf8".to_string()),
            ("oe", "utf8".to_string()),
            ("start", start.to_string()),
            ("num", request.limit.to_string()),
            ("asearch", "arc".to_string()),
            ("async", self.get_arc_id(start as usize).await),
        ];

        info!(
            "Google search request: query={}, limit={}",
            request.query, request.limit
        );

        // Build Google search URL
        let google_url = format!(
            "https://www.google.com/search?{}",
            build_query_string(&query_params)
        );
        info!(
            "Constructed Google Search URL (length: {})",
            google_url.len()
        );

        // Use EngineClient to scrape the search result page.
        //
        // Google's search results page can be scraped via plain HTTP without JS
        // rendering when the request comes from a residential IP. SearXNG (the
        // reference implementation in temp/searxng) uses the same approach:
        // HTTP request + lxml/CSS selector parsing, no browser engine.
        //
        // In datacenter environments, Google may return a noscript/captcha page
        // requiring JS rendering. In that case, deploy FlareSolverr
        // (http://localhost:8191) and set needs_js=true below to route the
        // request through FlareSolverrGoogleEngine.
        //
        // Setting needs_js=false ensures ReqwestEngine (support_score=100) is
        // selected by the router instead of being filtered out by feature_filter
        // (which excludes engines with support_score < 50 for needs_js=true).
        //
        // CONSENT cookie: SearXNG sets CONSENT=YES+ to bypass Google's EU
        // consent redirect (see temp/searxng/searx/engines/google.py line 274).
        let engine_request = EngineScrapeRequest::new(&google_url).with_options(
            crate::engines::engine_client::ScrapeOptions::builder()
                .needs_js(false)
                .timeout(Duration::from_secs(60))
                // 复用静态 headers（Lazy<HashMap>）— 见 GOOGLE_STATIC_HEADERS 注释
                // 性能 LOW-1：clone 仍分配 6 个 String，与每次新构造等价；
                // 真正优化需要 ScrapeOptions::headers 接口改造（暂未做）。
                .headers(GOOGLE_STATIC_HEADERS.clone())
                .build(),
        );

        let scrape_response = self
            .engine_client
            .scrape(&engine_request)
            .await
            .map_err(|e| SearchError::EngineClient("Google".to_string(), e.to_string()))?;

        // Handle non-200 status codes
        if !scrape_response.is_success() {
            if scrape_response.status_code == 429 {
                warn!("Google rate limit exceeded (429)");
                return Err(SearchError::RateLimited("Google".to_string()));
            }
            warn!(
                "Google returned status code: {}",
                scrape_response.status_code
            );
            // We might still try to parse if content is present, but usually error page
        }

        // Detect Google's bot-protection responses (CAPTCHA / sorry / noscript pages)
        // See temp/searxng/searx/engines/google.py detect_google_sorry()
        let content = &scrape_response.content;
        // 性能 LOW-2：2 次 contains 替代 AhoCorasick（2 个短模式，状态机常数开销不划算）
        if GOOGLE_BOT_PROTECTION_OR_PATTERNS
            .iter()
            .any(|p| content.contains(p))
        {
            warn!("Google returned CAPTCHA/sorry page — IP likely flagged as bot");
            return Err(SearchError::RateLimited("Google".to_string()));
        }
        // noscript 检测保持 2 次 contains（AND 逻辑：两个子串都存在才判定）
        if content.contains("Please click") && content.contains("enablejs") {
            warn!(
                "Google returned noscript page — JS rendering required (deploy FlareSolverr at localhost:8191)"
            );
            return Err(SearchError::EngineClient(
                "Google".to_string(),
                "noscript page returned — JS rendering required (FlareSolverr not available)"
                    .to_string(),
            ));
        }

        let results = self.parse_results(content)?;

        // If no results and status was OK, it might be a different layout or captcha
        if results.is_empty() {
            warn!(
                "No results found on Google page. Content length: {}",
                content.len()
            );
        }

        Ok(Response {
            items: results,
            total_results: None,
            engine: SearchEngineType::Google,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_id() {
        let id1 = GoogleSearchEngine::generate_random_id();
        let id2 = GoogleSearchEngine::generate_random_id();
        assert_eq!(id1.len(), 23);
        assert_eq!(id2.len(), 23);
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_google_search_engine_creation() {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        let engine = GoogleSearchEngine::new(engine_client);
        assert_eq!(engine.name(), "Google");
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        let engine = GoogleSearchEngine::new(engine_client);
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        tokio::time::sleep(Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2);
    }

    // ========== generate_random_id 补充测试 ==========

    #[test]
    fn test_generate_random_id_charset() {
        // 测试生成的 ID 只包含合法字符（A-Z a-z 0-9 _ -）
        let id = GoogleSearchEngine::generate_random_id();
        let valid_chars: std::collections::HashSet<char> =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"
                .chars()
                .collect();
        for c in id.chars() {
            assert!(
                valid_chars.contains(&c),
                "invalid character '{}' in ID: {}",
                c,
                id
            );
        }
    }

    #[test]
    fn test_generate_random_id_multiple_calls_all_correct_length() {
        // 测试多次调用都返回 23 字符长度的 ID
        for _ in 0..10 {
            let id = GoogleSearchEngine::generate_random_id();
            assert_eq!(id.len(), 23, "every ID should be 23 characters");
        }
    }

    // ========== escape_html 测试 ==========
    // escape_html 方法已迁移到 shared_utils::escape_html_text（架构 MEDIUM 4：消除重复实现）
    // 相关测试在 shared_utils.rs 的 tests 模块中

    // ========== parse_results 测试 ==========

    fn create_engine() -> GoogleSearchEngine {
        use crate::engines::engine_client::EngineClient;
        let engine_client = Arc::new(EngineClient::new());
        GoogleSearchEngine::new(engine_client)
    }

    #[test]
    fn test_parse_results_empty_html() {
        // 边界情况：空 HTML 返回 Ok 但结果为空
        let engine = create_engine();
        let results = engine.parse_results("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_valid_html() {
        // 测试从有效 Google HTML 解析结果
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>First Result</h3></a>
                <div data-sncf="1">First snippet</div>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/2"><h3>Second Result</h3></a>
                <div data-sncf="1">Second snippet</div>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Result");
        assert_eq!(results[0].url, "https://example.com/1");
        assert_eq!(results[0].description, "First snippet");
        assert_eq!(results[0].engine, SearchEngineType::Google);
        assert_eq!(results[1].title, "Second Result");
    }

    #[test]
    fn test_parse_results_no_matching_selectors() {
        // 边界情况：HTML 不包含 Google 结果选择器时返回空
        let engine = create_engine();
        let html = r#"<html><body><div>no results here</div></body></html>"#;
        let results = engine.parse_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_results_url_q_format_cleaned() {
        // 测试 /url?q= 格式的链接被正确清理
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/url?q=https://example.com/real&sa=t"><h3>Cleaned URL</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/real");
    }

    #[test]
    fn test_parse_results_relative_url_prefixed() {
        // 测试相对 URL 被添加 google.com 前缀
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/relative/path"><h3>Relative URL</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.google.com/relative/path");
    }

    #[test]
    fn test_parse_results_skips_non_http_urls() {
        // 边界情况：非 http 开头的 URL 被跳过
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="javascript:void(0)"><h3>JS Link</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/valid"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/valid");
    }

    #[test]
    fn test_parse_results_deduplicates_by_url() {
        // 测试相同 URL 的结果被去重
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/dup"><h3>First Title</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/dup"><h3>Duplicate Title</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/unique"><h3>Unique Title</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Title");
        assert_eq!(results[1].title, "Unique Title");
    }

    #[test]
    fn test_parse_results_max_20_limit() {
        // 测试结果数量上限为 20
        let engine = create_engine();
        let mut html = String::from("<html><body>");
        for i in 0..25 {
            html.push_str(&format!(
                r#"<div jscontroller="SC7lYd"><a href="https://example.com/{}"><h3>Result {}</h3></a></div>"#,
                i, i
            ));
        }
        html.push_str("</body></html>");

        let results = engine.parse_results(&html).unwrap();
        assert_eq!(results.len(), 20, "should cap at 20 results");
    }

    #[test]
    fn test_parse_results_skips_missing_title() {
        // 边界情况：缺少 h3 标题的块被跳过
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/2"><h3>Valid Result</h3></a>
            </div>
        </body></html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid Result");
    }

    // ========== search() test fallback path ==========

    /// Mutex to serialize tests that mutate process-level environment
    /// variables (std::env::set_var is not thread-safe across tests).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_search_request(query: &str) -> SearchRequest {
        SearchRequest::new(query)
    }

    // ENV_LOCK must be held across .await because engine.search() reads
    // GOOGLE_HTTP_FALLBACK_TEST_RESULTS / CRAWLRS_ENV during execution;
    // releasing it would let other tests race-modify the env var.
    // Single-threaded tokio runtime => no deadlock risk.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_search_fallback_returns_hardcoded_results_in_dev_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
        std::env::set_var("CRAWLRS_ENV", "development");

        let engine = create_engine();
        let request = make_search_request("rust programming");

        let response = engine.search(&request).await;

        // Clean up env vars ASAP to minimize cross-test interference
        std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
        std::env::remove_var("CRAWLRS_ENV");

        let response = response.expect("fallback should return Ok in dev env");
        assert_eq!(response.items.len(), 2, "fallback should return 2 items");
        assert_eq!(response.engine, SearchEngineType::Google);
        assert_eq!(response.total_results, Some(2));
        assert!(response.items[0].title.contains("rust programming"));
        assert_eq!(response.items[0].url, "https://google.com/1");
        assert_eq!(response.items[1].url, "https://google.com/2");
        assert_eq!(response.items[0].engine, SearchEngineType::Google);
    }

    #[allow(clippy::await_holding_lock)] // ENV_LOCK serializes env var access; see above
    #[tokio::test]
    async fn test_search_fallback_returns_hardcoded_results_in_test_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
        std::env::set_var("APP_ENVIRONMENT", "test");

        let engine = create_engine();
        let request = make_search_request("test query");

        let response = engine.search(&request).await;

        std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
        std::env::remove_var("APP_ENVIRONMENT");

        let response = response.expect("fallback should return Ok in test env");
        assert_eq!(response.items.len(), 2);
        assert!(response.items[0].title.contains("test query"));
    }

    #[allow(clippy::await_holding_lock)] // ENV_LOCK serializes env var access; see above
    #[tokio::test]
    async fn test_search_fallback_returns_hardcoded_results_with_empty_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
        // No CRAWLRS_ENV or APP_ENVIRONMENT set — should default to "development"
        std::env::remove_var("CRAWLRS_ENV");
        std::env::remove_var("APP_ENVIRONMENT");

        let engine = create_engine();
        let request = make_search_request("default env test");

        let response = engine.search(&request).await;

        std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");

        let response = response
            .expect("fallback should return Ok when env vars unset (defaults to development)");
        assert_eq!(response.items.len(), 2);
        assert!(response.items[0].title.contains("default env test"));
    }

    #[allow(clippy::await_holding_lock)] // ENV_LOCK serializes env var access; see above
    #[tokio::test]
    async fn test_search_fallback_escapes_query_in_title() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
        std::env::set_var("CRAWLRS_ENV", "dev");

        let engine = create_engine();
        // Query with HTML special characters that should be escaped
        let request = make_search_request("<script>alert(1)</script>");

        let response = engine.search(&request).await;

        std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
        std::env::remove_var("CRAWLRS_ENV");

        let response = response.expect("fallback should return Ok");
        // The title should contain the escaped query, not raw HTML
        assert!(response.items[0].title.contains("&lt;script&gt;"));
        assert!(!response.items[0].title.contains("<script>"));
    }

    #[allow(clippy::await_holding_lock)] // ENV_LOCK serializes env var access; see above
    #[tokio::test]
    async fn test_search_fallback_with_dev_alias_env_values() {
        // Test all accepted dev environment aliases: "development", "dev", "test", "testing", ""
        let aliases = ["dev", "testing"];

        for alias in aliases {
            let _lock = ENV_LOCK.lock().unwrap();
            std::env::set_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS", "true");
            std::env::set_var("CRAWLRS_ENV", alias);

            let engine = create_engine();
            let request = make_search_request("alias test");

            let response = engine.search(&request).await;

            std::env::remove_var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS");
            std::env::remove_var("CRAWLRS_ENV");

            assert!(
                response.is_ok(),
                "fallback should succeed for CRAWLRS_ENV={}",
                alias
            );
            assert_eq!(response.unwrap().items.len(), 2);
        }
    }

    // ========== force_refresh_arc_id tests ==========

    #[tokio::test]
    async fn test_force_refresh_arc_id_changes_id() {
        let engine = create_engine();
        let arc_id_before = engine.get_arc_id(0).await;
        engine.force_refresh_arc_id().await;
        let arc_id_after = engine.get_arc_id(0).await;
        assert_ne!(
            arc_id_before, arc_id_after,
            "ARC_ID should change after force_refresh_arc_id"
        );
    }

    #[tokio::test]
    async fn test_force_refresh_arc_id_preserves_format() {
        let engine = create_engine();
        engine.force_refresh_arc_id().await;
        let arc_id = engine.get_arc_id(5).await;
        assert!(arc_id.contains("arc_id:srp_"));
        assert!(arc_id.contains("_1"));
        assert!(arc_id.contains("use_ac:true"));
        assert!(arc_id.contains("_fmt:prog"));
    }

    // ========== safe_parse_selector tests ==========

    #[test]
    fn test_safe_parse_selector_valid_simple() {
        assert!(safe_parse_selector("div").is_some());
        assert!(safe_parse_selector("a").is_some());
        assert!(safe_parse_selector("h3").is_some());
    }

    #[test]
    fn test_safe_parse_selector_valid_complex() {
        assert!(safe_parse_selector("a[href]").is_some());
        assert!(safe_parse_selector("div[class='test']").is_some());
        assert!(safe_parse_selector("h3, a h3").is_some());
        assert!(safe_parse_selector("div[jscontroller*='SC7lYd']").is_some());
        assert!(safe_parse_selector("div[data-sncf='1'], div[data-snc]").is_some());
    }

    #[test]
    fn test_safe_parse_selector_invalid() {
        assert!(safe_parse_selector("<<<").is_none());
        assert!(safe_parse_selector("").is_none());
        assert!(safe_parse_selector("div[").is_none());
        assert!(safe_parse_selector("div[").is_none());
    }

    // ========== parse_results additional edge cases ==========

    #[test]
    fn test_parse_results_empty_title_skipped() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>   </h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/2"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid");
    }

    #[test]
    fn test_parse_results_missing_link_node_skipped() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <h3>Title Without Link</h3>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/valid"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Valid");
    }

    #[test]
    fn test_parse_results_empty_href_skipped() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href=""><h3>Empty Href</h3></a>
            </div>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/valid"><h3>Valid</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/valid");
    }

    #[test]
    fn test_parse_results_url_q_without_ampersand() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/url?q=https://example.com/clean"><h3>Clean</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/clean");
    }

    #[test]
    fn test_parse_results_protocol_relative_url_skipped() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="//example.com/protocol-relative"><h3>Protocol Relative</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert!(
            results.is_empty(),
            "protocol-relative URL should be skipped (not http prefix)"
        );
    }

    #[test]
    fn test_parse_results_data_snc_selector() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>With SNC</h3></a>
                <div data-snc="1">SNC snippet</div>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "SNC snippet");
    }

    #[test]
    fn test_parse_results_no_snippet_defaults_empty() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>No Snippet</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "");
    }

    #[test]
    fn test_parse_results_html_entities_in_title_escaped() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com/1"><h3>A &amp; B</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("&amp;"));
        assert!(!results[0].title.contains(" & "));
    }

    #[test]
    fn test_parse_results_relative_url_with_path() {
        let engine = create_engine();
        let html = r#"
        <html><body>
            <div jscontroller="SC7lYd">
                <a href="/search?q=test"><h3>Relative Path</h3></a>
            </div>
        </body></html>
        "#;
        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.google.com/search?q=test");
    }
}
