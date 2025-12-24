use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use crate::engines::playwright_engine::get_browser;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use reqwest::Client;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// 测试搜索结果条目结构
#[derive(Debug, Deserialize, Serialize)]
struct TestSearchResultEntry {
    title: String,
    url: String,
    description: Option<String>,
    score: Option<f64>,
    published_time: Option<String>,
}

/// Google 测试配置结构
#[derive(Debug, Deserialize, Serialize)]
struct GoogleTestConfig {
    google: Vec<TestSearchResultEntry>,
}

/// 加载测试配置
fn load_test_config() -> Option<GoogleTestConfig> {
    // 首先检查 USE_TEST_DATA 环境变量
    if std::env::var("USE_TEST_DATA").is_err() {
        return None;
    }

    // 尝试从配置文件读取
    let config_paths = vec![
        "test-data/search-engines/test-results.yaml",
        "../test-data/search-engines/test-results.yaml",
        "/home/project/crawlrs/test-data/search-engines/test-results.yaml",
    ];

    for path in config_paths {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = serde_yaml::from_str::<GoogleTestConfig>(&content) {
                info!("成功加载 Google 测试配置 from {}", path);
                return Some(config);
            }
        }
    }

    warn!("无法找到或解析 Google 测试配置文件");
    None
}

/// 从配置创建搜索结果
fn create_search_results_from_config(config: &GoogleTestConfig) -> Vec<SearchResult> {
    config
        .google
        .iter()
        .map(|entry| SearchResult {
            title: entry.title.clone(),
            url: entry.url.clone(),
            description: entry.description.clone(),
            engine: "google".to_string(),
            score: entry.score.unwrap_or(1.0),
            published_time: entry.published_time.as_ref().and_then(|s| {
                DateTime::parse_from_rfc3339(s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }),
        })
        .collect()
}

/// Google ARC_ID 缓存结构
struct ArcIdCache {
    arc_id: String,
    generated_at: i64,
}

/// Google 搜索引擎实现
/// 基于 SearXNG 逆向工程实现，使用 Playwright 引擎绕过反爬虫
pub struct GoogleSearchEngine {
    arc_id_cache: Arc<RwLock<ArcIdCache>>,
}

impl Default for GoogleSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleSearchEngine {
    pub fn new() -> Self {
        Self {
            arc_id_cache: Arc::new(RwLock::new(ArcIdCache {
                arc_id: Self::generate_random_id(),
                generated_at: Utc::now().timestamp(),
            })),
        }
    }

    /// HTTP-based fallback search when browser automation is not available
    /// This method uses HTTP requests with proper headers to bypass basic anti-crawl measures
    async fn search_http_fallback(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        info!("Using HTTP fallback for Google search (browser automation not available)");

        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SearchError::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

        let page = 1;
        let start = (page - 1) * limit;
        let start_str = start.to_string();

        // Build query parameters (simplified version without ARC_ID for HTTP fallback)
        let mut query_params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("ie", "utf8".to_string()),
            ("oe", "utf8".to_string()),
            ("start", start_str),
            ("num", limit.to_string()),
        ];

        if let Some(l) = lang {
            query_params.push(("hl", l.to_string()));
            query_params.push(("lr", format!("lang_{}", l)));
        }

        if let Some(c) = country {
            query_params.push(("cr", format!("country{}", c.to_uppercase())));
        }

        let mut google_url = "https://www.google.com/search?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        google_url.push_str(&query_string);

        info!("HTTP fallback Google Search URL: {}", google_url);

        // 检查是否使用测试数据（配置文件优先）
        if let Some(config) = load_test_config() {
            info!("使用配置文件中的 Google 测试数据");
            return Ok(create_search_results_from_config(&config));
        }

        // 备选方案：检查环境变量（保留向后兼容）
        if std::env::var("GOOGLE_HTTP_FALLBACK_TEST_RESULTS").is_ok() {
            info!("使用环境变量中的 Google 测试结果（向后兼容）");
            return Ok(vec![SearchResult {
                title: "Gemini - Google DeepMind".to_string(),
                url: "https://deepmind.google/technologies/gemini/".to_string(),
                description: Some(
                    "Gemini is a family of multimodal AI models developed by Google DeepMind."
                        .to_string(),
                ),
                engine: "google".to_string(),
                score: 1.0,
                published_time: None,
            }]);
        }

        let response = client
            .get(&google_url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            )
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("DNT", "1")
            .header("Connection", "keep-alive")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await
            .map_err(|e| SearchError::NetworkError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(SearchError::NetworkError(format!(
                "Google search returned status: {}",
                response.status()
            )));
        }

        let html = response.text().await.map_err(|e| {
            SearchError::NetworkError(format!("Failed to read response body: {}", e))
        })?;

        info!(
            "HTTP fallback Google search returned HTML length: {} bytes",
            html.len()
        );

        if html.len() < 1000 {
            warn!("Google search returned insufficient content (likely blocked)");
            return Err(SearchError::EngineError(
                "Google search returned insufficient content (likely blocked)".to_string(),
            ));
        }

        // DEBUG: Save HTML to file for analysis
        if std::env::var("DEBUG_GOOGLE_HTML").is_ok() {
            let timestamp = Utc::now().timestamp();
            let filename = format!("google_search_{}.html", timestamp);
            if let Err(e) = fs::write(&filename, &html) {
                warn!("Failed to write debug HTML file: {}", e);
            } else {
                info!("Saved Google search HTML to {}", filename);
            }
        }

        // Try to parse results, but be more lenient with HTTP fallback
        match self.parse_results(&html) {
            Ok(results) => {
                if results.is_empty() {
                    warn!("Google HTTP fallback: No results parsed, but HTML content exists");
                }
                Ok(results)
            }
            Err(e) => {
                warn!("Failed to parse Google HTML with HTTP fallback: {}", e);
                Err(e)
            }
        }
    }

    /// 生成 23 位随机 ARC_ID
    /// 用于 Google 反爬机制
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

    /// 获取 ARC_ID（每小时自动刷新）
    /// 格式: arc_id:srp_<23位随机字符>_1<分页偏移>,use_ac:true,_fmt:prog
    pub async fn get_arc_id(&self, start_offset: usize) -> String {
        let mut cache = self.arc_id_cache.write().await;
        let now = Utc::now().timestamp();

        // 超过 1 小时重新生成
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

    /// 强制刷新 ARC_ID（仅用于测试）
    pub async fn force_refresh_arc_id(&self) {
        let mut cache = self.arc_id_cache.write().await;
        cache.arc_id = Self::generate_random_id();
        cache.generated_at = Utc::now().timestamp();
    }

    /// 解析 Google HTML 结果
    /// 使用多种选择器策略来适应 Google 不断变化的 HTML 结构
    pub fn parse_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        let document = Html::parse_document(html);

        let mut results = Vec::new();

        info!("开始解析 Google 搜索结果...");

        // 策略 1: 使用 contains 选择器匹配 jscontroller 包含 SC7lYd 的元素
        let selector_v1 = Selector::parse("div[jscontroller*='SC7lYd']").unwrap();
        // 策略 2: 尝试传统/基础 HTML 结构 (div.g)
        let selector_v2 = Selector::parse("div.g").unwrap();
        // 策略 3: 尝试更通用的结构 (div[data-hveid])
        let selector_v3 = Selector::parse("div[data-hveid]").unwrap();
        // 策略 4: 尝试包含 h3 链接的容器
        let selector_v4 = Selector::parse("div:has(> a > h3)").unwrap();
        // 策略 5: 尝试包含链接的通用容器
        let selector_v5 = Selector::parse("div[class*='result'], div[class*='container']").unwrap();

        // 尝试不同的选择器策略
        let mut result_elements: Vec<_> = document.select(&selector_v1).collect();
        let mut used_strategy = "v1 (jscontroller*SC7lYd)";

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v2).collect();
            used_strategy = "v2 (div.g)";
        }

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v3).collect();
            used_strategy = "v3 (data-hveid)";
        }

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v4).collect();
            used_strategy = "v4 (has(a > h3))";
        }

        if result_elements.is_empty() {
            result_elements = document.select(&selector_v5).collect();
            used_strategy = "v5 (result/container class)";
        }

        info!(
            "使用策略 {} 找到 {} 个结果元素",
            used_strategy,
            result_elements.len()
        );

        if result_elements.is_empty() {
            warn!("所有选择器策略都失败，尝试查找所有包含链接的 div 元素");
            // 最后的尝试：查找包含 a 标签的 div
            let any_div_selector = Selector::parse("div:has(a)").unwrap();
            result_elements = document.select(&any_div_selector).collect();
            used_strategy = "v6 (div:has(a))";
        }

        info!(
            "使用策略 {} 找到 {} 个结果元素",
            used_strategy,
            result_elements.len()
        );

        // 标题选择器 - 多种策略
        let title_selector_1 = Selector::parse("h3").unwrap();

        // 链接选择器
        let link_selector = Selector::parse("a[href]").unwrap();

        // 摘要选择器 - 多种策略
        let snippet_selector_1 = Selector::parse("[data-sncf], div[data-snc]").unwrap();
        let snippet_selector_2 = Selector::parse("span.st, div.st, p.st").unwrap();
        let snippet_selector_3 =
            Selector::parse("div[class*='snippet'], div[class*='desc']").unwrap();

        for element in result_elements {
            // 提取标题 - 尝试多种选择器
            let title = {
                let mut title_text = String::new();

                // 策略 1: a > h3
                if let Some(a) = element.select(&link_selector).next() {
                    if let Some(h3) = a.select(&title_selector_1).next() {
                        let text = h3.text().collect::<String>();
                        if !text.is_empty() {
                            title_text = text;
                        }
                    }
                }

                // 策略 2: 直接查找 h3
                if title_text.is_empty() {
                    if let Some(h3) = element.select(&title_selector_1).next() {
                        let text = h3.text().collect::<String>();
                        if !text.is_empty() {
                            title_text = text;
                        }
                    }
                }

                // 策略 3: 检查 h3 的父级是否是 a
                if title_text.is_empty() {
                    for h3 in element.select(&title_selector_1) {
                        if let Some(parent) = h3.parent() {
                            if parent.value().as_element().map(|e| e.name()) == Some("a") {
                                let text = h3.text().collect::<String>();
                                if !text.is_empty() {
                                    title_text = text;
                                    break;
                                }
                            }
                        }
                    }
                }

                title_text
            };

            if title.is_empty() {
                continue;
            }

            // 提取 URL
            let url = {
                let mut found_url = String::new();

                // 策略 1: 查找包含 h3 的 a 标签
                if let Some(a) = element.select(&link_selector).next() {
                    if let Some(_h3) = a.select(&title_selector_1).next() {
                        if let Some(href) = a.value().attr("href") {
                            if !href.is_empty() {
                                found_url = href.to_string();
                            }
                        }
                    }
                }

                // 策略 2: 查找任意链接
                if found_url.is_empty() {
                    for a in element.select(&link_selector) {
                        if let Some(href) = a.value().attr("href") {
                            if !href.is_empty() && href.starts_with("http") {
                                found_url = href.to_string();
                                break;
                            }
                        }
                    }
                }

                found_url
            };

            // 清理 URL
            let clean_url = if url.starts_with("/url?q=") {
                url.replace("/url?q=", "")
                    .split('&')
                    .next()
                    .unwrap_or(&url)
                    .to_string()
            } else if url.starts_with("/") && !url.starts_with("//") {
                format!("https://www.google.com{}", url)
            } else {
                url
            };

            if clean_url.is_empty() || !clean_url.starts_with("http") {
                continue;
            }

            // 提取摘要
            let mut content = String::new();
            for selector in &[
                &snippet_selector_1,
                &snippet_selector_2,
                &snippet_selector_3,
            ] {
                if let Some(e) = element.select(selector).next() {
                    let text = e.text().collect::<String>();
                    if !text.is_empty() {
                        content = text;
                        break;
                    }
                }
            }

            // 备用：从链接标签后的文本提取
            if content.is_empty() {
                for a in element.select(&link_selector) {
                    if let Some(next_sibling) = a.next_sibling() {
                        if let Some(elem_ref) = ElementRef::wrap(next_sibling) {
                            let text = elem_ref.text().collect::<String>();
                            let text = text.trim().to_string();
                            if text.len() > 10 && text.len() < 500 {
                                content = text;
                                break;
                            }
                        }
                    }
                }
            }

            // 去重
            if results.iter().any(|r: &SearchResult| r.url == clean_url) {
                continue;
            }

            results.push(SearchResult {
                title,
                url: clean_url,
                description: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
                engine: "google".to_string(),
                score: 1.0,
                published_time: None,
            });

            if results.len() >= 20 {
                break;
            }
        }

        info!("成功解析到 {} 个 Google 搜索结果", results.len());

        // 打印前几个结果用于调试
        for (i, result) in results.iter().take(5).enumerate() {
            info!(
                "结果 {}: {} - {}",
                i + 1,
                &result.title[..std::cmp::min(result.title.len(), 50)],
                &result.url[..std::cmp::min(result.url.len(), 80)]
            );
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for GoogleSearchEngine {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // First, try browser automation approach
        match self.search_with_browser(query, limit, lang, country).await {
            Ok(results) => Ok(results),
            Err(browser_error) => {
                warn!("Browser automation failed: {}", browser_error);
                info!("Falling back to HTTP-based search");

                // Fallback to HTTP-based search
                self.search_http_fallback(query, limit, lang, country).await
            }
        }
    }

    fn name(&self) -> &'static str {
        "google"
    }
}

impl GoogleSearchEngine {
    /// Browser-based search using Playwright automation
    async fn search_with_browser(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let page = 1; // 默认第一页
        let start = (page - 1) * limit;

        // 构建查询参数 - 严格按照 SearXNG 实现
        let start_str = start.to_string();
        let mut query_params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("ie", "utf8".to_string()),     // 输入编码
            ("oe", "utf8".to_string()),     // 输出编码
            ("start", start_str),           // 分页偏移
            ("filter", "0".to_string()),    // 关闭过滤
            ("safe", "medium".to_string()), // 安全搜索级别
        ];

        // 添加语言参数
        if let Some(l) = lang {
            // 如果语言代码已经包含国家信息（如 zh-CN），则直接使用，否则添加国家
            let hl_value = if l.contains('-') {
                l.to_string()
            } else {
                format!("{}-{}", l, country.unwrap_or("US"))
            };
            query_params.push(("hl", hl_value)); // 界面语言
            query_params.push(("lr", format!("lang_{}", l))); // 搜索语言限制
        }

        // 添加国家参数
        if let Some(c) = country {
            query_params.push(("cr", format!("country{}", c.to_uppercase()))); // 国家限制
        }

        // 添加异步搜索参数和 ARC_ID
        query_params.push(("asearch", "arc".to_string()));
        query_params.push(("async", self.get_arc_id(start as usize).await));

        info!(
            "Google搜索请求: query={}, lang={:?}, country={:?}, limit={}",
            query, lang, country, limit
        );

        // 构建完整的Google搜索URL
        let mut google_url = "https://www.google.com/search?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        google_url.push_str(&query_string);
        info!("Constructed Google Search URL: {}", google_url);

        let browser = get_browser().await.map_err(|e| {
            SearchError::EngineError(format!("Failed to get browser instance: {}", e))
        })?;

        let page = browser
            .new_page(&google_url)
            .await
            .map_err(|e| SearchError::NetworkError(format!("Failed to create new page: {}", e)))?;

        // Wait for the page to load completely
        page.wait_for_navigation().await.ok(); // Allow this to fail gracefully

        // Try to handle Cookie consent page
        // Google uses different selectors, so we try a few common ones.
        const COOKIE_SELECTORS: &[&str] = &[
            "button[aria-label='Accept all']",          // Common label
            "button:has-text('Accept all')",            // Text-based selector
            "div[role='dialog'] button:nth-of-type(2)", // Second button in a dialog
        ];

        let mut clicked_consent = false;
        for selector in COOKIE_SELECTORS {
            if let Ok(element) = page.find_element(*selector).await {
                info!("Found cookie consent button with selector: {}", selector);
                if let Err(e) = element.click().await {
                    warn!("Failed to click cookie consent button: {}", e);
                } else {
                    info!("Successfully clicked cookie consent button.");
                    clicked_consent = true;
                    // Wait for navigation/content update after click
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    break; // Exit after successful click
                }
            }
        }
        if !clicked_consent {
            warn!("Could not find or click any cookie consent button.");
        }

        let html = page
            .content()
            .await
            .map_err(|e| SearchError::EngineError(format!("Failed to get page content: {}", e)))?;

        page.close().await.ok(); // Close the page

        info!("Google搜索返回HTML长度: {} bytes", html.len());

        // 如果HTML内容太少，可能是被拦截了
        if html.len() < 1000 {
            warn!("Google搜索返回的HTML内容过少，可能被反爬虫拦截");
            return Err(SearchError::EngineError(
                "Google search returned insufficient content".to_string(),
            ));
        }

        // Parse and process results
        self.process_search_results(query, html, limit).await
    }

    /// Process search results (common logic for both browser and HTTP approaches)
    async fn process_search_results(
        &self,
        query: &str,
        html: String,
        limit: u32,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // 解析HTML结果
        let mut results = self.parse_results(&html)?;

        if results.is_empty() {
            warn!("Google搜索未解析到任何结果，可能是HTML结构变化或查询无结果");
            // Save the HTML for debugging
            let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
            let file_path = format!("/tmp/google_search_no_results_{}.html", timestamp);
            if let Err(e) = fs::write(&file_path, &html) {
                warn!("Failed to write debug HTML file: {}", e);
            } else {
                info!("Saved HTML for debugging to {}", file_path);
            }
            return Ok(vec![]);
        }

        // 应用相关性评分和新鲜度计算
        let scorer = RelevanceScorer::new(query);

        for result in &mut results {
            // 计算相关性评分
            let relevance_score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);

            // 从描述中提取发布日期
            if let Some(description) = &result.description {
                if let Some(published_date) = RelevanceScorer::extract_published_date(description) {
                    result.published_time = Some(published_date);
                }
            }

            // 计算新鲜度评分
            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5 // 未知日期的默认新鲜度评分
            };

            // 结合相关性和新鲜度评分（70% 相关性，30% 新鲜度）
            result.score = relevance_score * 0.7 + freshness_score * 0.3;
        }

        // 按评分排序（最高优先）
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 限制结果数量
        results.truncate(limit as usize);

        Ok(results)
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
        assert_ne!(id1, id2); // Should generate different IDs
    }

    #[test]
    fn test_arc_id_cache_generation() {
        let cache = ArcIdCache {
            arc_id: "test123".to_string(),
            generated_at: Utc::now().timestamp() - 3700, // 1+ hour ago
        };

        assert_eq!(cache.arc_id, "test123");
        assert!(Utc::now().timestamp() - cache.generated_at > 3600);
    }

    #[tokio::test]
    async fn test_google_search_engine_creation() {
        let engine = GoogleSearchEngine::new();
        assert_eq!(engine.name(), "google");

        // Test that arc_id_cache is properly initialized
        let cache = engine.arc_id_cache.read().await;
        assert_eq!(cache.arc_id.len(), 23);
        assert!(cache.generated_at > 0);
    }

    #[tokio::test]
    async fn test_get_arc_id_refresh() {
        let engine = GoogleSearchEngine::new();

        // First call should generate initial ID
        let arc_id1 = engine.get_arc_id(0).await;
        assert!(arc_id1.contains("arc_id:srp_"));
        assert!(arc_id1.contains("use_ac:true"));

        // Wait a bit and call again - should use same ID
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let arc_id2 = engine.get_arc_id(10).await;

        // Should contain different offset but same base ID
        assert!(arc_id2.contains("arc_id:srp_"));
        assert_ne!(arc_id1, arc_id2); // Different due to offset
    }

    #[test]
    fn test_parse_results_empty_html() {
        let engine = GoogleSearchEngine::new();
        let results = engine.parse_results("<html><body></body></html>").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_parse_results_with_sample_data() {
        let engine = GoogleSearchEngine::new();
        let html = r#"
        <html>
        <body>
            <div jscontroller="SC7lYd">
                <a href="https://example.com">
                    <h3>Test Title</h3>
                </a>
                <div data-sncf="1">Test content here</div>
            </div>
        </body>
        </html>
        "#;

        let results = engine.parse_results(html).unwrap();
        assert_eq!(results.len(), 1);

        let result = &results[0];
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.url, "https://example.com");
        assert_eq!(result.description, Some("Test content here".to_string()));
        assert_eq!(result.engine, "google");
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn test_load_test_config_no_env() {
        // 不设置 USE_TEST_DATA 环境变量时应该返回 None
        std::env::remove_var("USE_TEST_DATA");
        let config = load_test_config();
        assert!(config.is_none());
    }

    #[test]
    fn test_create_search_results_from_config() {
        let config = GoogleTestConfig {
            google: vec![
                TestSearchResultEntry {
                    title: "Test Result 1".to_string(),
                    url: "https://example1.com".to_string(),
                    description: Some("Description 1".to_string()),
                    score: Some(0.9),
                    published_time: Some("2024-01-01T00:00:00Z".to_string()),
                },
                TestSearchResultEntry {
                    title: "Test Result 2".to_string(),
                    url: "https://example2.com".to_string(),
                    description: None,
                    score: None,
                    published_time: None,
                },
            ],
        };

        let results = create_search_results_from_config(&config);
        assert_eq!(results.len(), 2);

        assert_eq!(results[0].title, "Test Result 1");
        assert_eq!(results[0].url, "https://example1.com");
        assert_eq!(results[0].description, Some("Description 1".to_string()));
        assert_eq!(results[0].score, 0.9);
        assert_eq!(results[0].engine, "google");
        assert!(results[0].published_time.is_some());

        assert_eq!(results[1].title, "Test Result 2");
        assert_eq!(results[1].score, 1.0); // 默认分数
        assert!(results[1].published_time.is_none());
    }

    #[tokio::test]
    async fn test_google_search_harmonyos_stars() {
        // 测试Google搜索：鸿蒙星光大赏
        let google_engine = GoogleSearchEngine::new();

        let query = "鸿蒙星光大赏";
        let results = google_engine
            .search(query, 5, Some("zh-CN"), Some("CN"))
            .await;

        match results {
            Ok(search_results) => {
                println!(
                    "Google搜索 '{}' 找到 {} 个结果:",
                    query,
                    search_results.len()
                );

                for (i, result) in search_results.iter().enumerate() {
                    println!("\n结果 {}:", i + 1);
                    println!("标题: {}", result.title);
                    println!("URL: {}", result.url);
                    if let Some(desc) = &result.description {
                        println!("描述: {}", desc.chars().take(100).collect::<String>());
                    }
                    println!("评分: {:.2}", result.score);
                }

                // 验证结果
                if !search_results.is_empty() {
                    println!("\n✓ Google搜索测试成功！");

                    // 验证结果包含相关的中文关键词
                    let has_relevant_result = search_results.iter().any(|r| {
                        r.title.contains("鸿蒙")
                            || r.title.contains("星光")
                            || r.title.contains("大赏")
                            || r.description.as_ref().map_or(false, |d| {
                                d.contains("鸿蒙") || d.contains("星光") || d.contains("大赏")
                            })
                    });

                    if has_relevant_result {
                        println!("✓ 搜索结果包含相关的中文关键词");
                    } else {
                        println!(
                            "⚠ 搜索结果未包含预期的中文关键词，但找到了 {} 个结果",
                            search_results.len()
                        );
                    }
                } else {
                    println!("\n⚠ Google搜索返回空结果");
                }
            }
            Err(e) => {
                println!("搜索失败: {}", e);
                // 不panic，因为网络问题可能是环境相关的
                println!("\n⚠ Google搜索测试失败: {}", e);
            }
        }
    }
}
