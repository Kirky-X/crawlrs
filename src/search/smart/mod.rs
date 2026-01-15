// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use rand::prelude::*;
use scraper::{Html, Selector};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::domain::models::search_result::SearchResult;
use crate::domain::services::rate_limiting_service::{
    RateLimitResult, RateLimitingError, RateLimitingService,
};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use crate::engines::engine_client::{
    EngineClient, EngineError, PageAction, ScrapeOptions, ScrapeRequest, ScrollDirection,
};
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};
use crate::utils::text_processing::process_string;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// 安全解析CSS选择器，如果解析失败则记录警告并返回None
fn safe_parse_selector(selector_str: &str) -> Option<Selector> {
    Selector::parse(selector_str).ok()
}

/// 解析并验证选择器，如果所有选择器都失败则返回错误
///
/// 这个辅助函数消除了重复的 `.expect()` 模式
fn parse_selectors(
    engine_name: &str,
    selectors: &[&'static str],
    selector_type: &str,
) -> Result<Selector, SearchError> {
    selectors
        .iter()
        .filter_map(|s| safe_parse_selector(s))
        .next()
        .ok_or_else(|| {
            SearchError::Parse(format!(
                "Failed to parse {} selector for {}",
                selector_type, engine_name
            ))
        })
}

/// 搜索结果解析器配置
struct SearchResultParserConfig {
    /// 结果选择器
    result_selectors: Vec<&'static str>,
    /// 标题选择器
    title_selectors: Vec<&'static str>,
    /// 链接选择器
    link_selectors: Vec<&'static str>,
    /// 摘要选择器
    snippet_selectors: Vec<&'static str>,
    /// 引擎名称
    engine_name: &'static str,
    /// URL属性名（默认为href）
    url_attr: Option<&'static str>,
}

/// 通用搜索结果解析函数 - 消除重复代码
fn parse_search_results_common(
    html: &str,
    config: SearchResultParserConfig,
) -> Result<Vec<SearchResult>, SearchError> {
    use crate::domain::services::relevance_scorer::RelevanceScorer;
    use scraper::Html;

    let document = Html::parse_document(html);

    // 使用辅助函数解析选择器，消除重复的 expect 模式
    let result_selector = parse_selectors(config.engine_name, &config.result_selectors, "result")?;

    let title_selector = parse_selectors(config.engine_name, &config.title_selectors, "title")?;

    let link_selector = parse_selectors(config.engine_name, &config.link_selectors, "link")?;

    let snippet_selector =
        parse_selectors(config.engine_name, &config.snippet_selectors, "snippet")?;

    // 确定URL属性名（默认为href）
    let url_attr = config.url_attr.unwrap_or("href");

    let mut results = Vec::new();
    let scorer = RelevanceScorer::new(config.engine_name);

    for element in document.select(&result_selector) {
        let title = element
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 使用指定的属性提取URL
        let url = element
            .select(&link_selector)
            .next()
            .and_then(|el| el.value().attr(url_attr))
            .map(|href| href.to_string())
            .unwrap_or_default();

        let description = element
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let description = process_string(&description).unwrap_or(description);

        if !title.is_empty() && !url.is_empty() {
            let mut result = SearchResult::new(
                title,
                url,
                Some(description),
                config.engine_name.to_string(),
            );
            result.score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);
            results.push(result);
        }
    }

    Ok(results)
}

/// 智能搜索引擎配置
pub struct SmartSearchEngineConfig {
    /// 搜索引擎类型
    pub engine_type: SearchEngineType,
    /// 是否启用速率限制
    pub rate_limiting_enabled: bool,
    /// 速率限制服务（可选）
    pub rate_limiting_service: Option<Arc<dyn RateLimitingService>>,
    /// 请求超时时间（秒）
    pub timeout_seconds: u64,
    /// 是否启用测试数据模式
    pub test_data_enabled: bool,
    /// 测试数据路径
    pub test_data_path: Option<PathBuf>,
    /// 重试次数
    pub max_retries: u32,
    /// 重试间隔（毫秒）
    pub retry_delay_ms: u64,
}

impl std::fmt::Debug for SmartSearchEngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartSearchEngineConfig")
            .field("engine_type", &self.engine_type)
            .field("rate_limiting_enabled", &self.rate_limiting_enabled)
            .field("rate_limiting_service", &"Arc<dyn RateLimitingService>")
            .field("timeout_seconds", &self.timeout_seconds)
            .field("test_data_enabled", &self.test_data_enabled)
            .field("test_data_path", &self.test_data_path)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_ms", &self.retry_delay_ms)
            .finish()
    }
}

impl Default for SmartSearchEngineConfig {
    fn default() -> Self {
        Self {
            engine_type: SearchEngineType::Google,
            rate_limiting_enabled: true,
            rate_limiting_service: None,
            timeout_seconds: 90,
            test_data_enabled: false,
            test_data_path: None,
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}

/// 智能搜索引擎
///
/// 使用EngineClient智能路由，根据目标网站的特征自动选择最适合的抓取引擎
/// 支持速率限制、超时控制和测试数据加载
pub struct SmartSearchEngine {
    engine_client: Arc<EngineClient>,
    config: SmartSearchEngineConfig,
}

impl SmartSearchEngine {
    pub fn new(engine_client: Arc<EngineClient>, config: SmartSearchEngineConfig) -> Self {
        Self {
            engine_client,
            config,
        }
    }

    /// 检查速率限制
    async fn check_rate_limit(&self) -> Result<(), SearchError> {
        if !self.config.rate_limiting_enabled {
            return Ok(());
        }

        if let Some(ref service) = self.config.rate_limiting_service {
            match service.check_rate_limit("default", "smart_search").await {
                Ok(RateLimitResult::Allowed) => {
                    info!("速率限制检查通过");
                    Ok(())
                }
                Ok(RateLimitResult::Denied { reason }) => {
                    warn!("速率限制被拒绝: {}", reason);
                    Err(SearchError::Engine(format!(
                        "Rate limit exceeded: {}",
                        reason
                    )))
                }
                Ok(RateLimitResult::RetryAfter {
                    retry_after_seconds,
                }) => {
                    warn!("速率限制要求重试，等待 {} 秒", retry_after_seconds);
                    tokio::time::sleep(Duration::from_secs(retry_after_seconds)).await;
                    Ok(())
                }
                Err(RateLimitingError::RedisError(e)) => {
                    error!("Redis连接错误，降级处理: {}", e);
                    Ok(())
                }
                Err(e) => {
                    error!("速率限制服务错误: {}，降级处理", e);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    /// 加载测试数据
    fn load_test_data(&self, query: &str) -> Option<String> {
        if !self.config.test_data_enabled {
            return None;
        }

        if let Some(ref path) = self.config.test_data_path {
            // 尝试查找匹配的测试数据文件
            let test_file_pattern =
                format!("test_data_{}.html", query.replace(" ", "_").to_lowercase());
            let test_file_path = path.join(&test_file_pattern);

            if test_file_path.exists() {
                info!("加载测试数据文件: {:?}", test_file_path);
                return Some(fs::read_to_string(&test_file_path).unwrap_or_default());
            }

            // 尝试通用测试数据文件
            let generic_test_file = path.join("generic_search_results.html");
            if generic_test_file.exists() {
                info!("加载通用测试数据文件");
                return Some(fs::read_to_string(&generic_test_file).unwrap_or_default());
            }
        }

        None
    }

    /// 从测试数据解析结果
    fn parse_test_data(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        info!("从测试数据解析搜索结果");
        self.parse_search_results(html)
    }

    /// 智能判断是否需要JS和TLS指纹
    fn needs_js_and_tls(&self) -> (bool, bool) {
        match self.config.engine_type {
            SearchEngineType::Google => (true, false),
            SearchEngineType::Bing => (true, false),
            SearchEngineType::Baidu => (false, false),
            SearchEngineType::Sogou => (true, false), // Sogou需要JS渲染结果
            // 对于非特定引擎类型，默认使用 Google 的配置
            _ => (true, false),
        }
    }

    /// HTML 转义以防止 XSS 攻击
    fn escape_html(&self, text: &str) -> String {
        html_escape::encode_text(text).trim().to_string()
    }

    /// 构建搜索URL
    fn build_search_url(&self, query: &str, lang: Option<&str>, country: Option<&str>) -> String {
        match self.config.engine_type {
            SearchEngineType::Google => self.build_google_search_url(query, lang, country),
            SearchEngineType::Bing => self.build_bing_search_url(query, lang, country),
            SearchEngineType::Baidu => self.build_baidu_search_url(query, lang, country),
            SearchEngineType::Sogou => self.build_sogou_search_url(query, lang, country),
            // 对于非特定引擎类型，默认使用 Google 的 URL 构建方式
            _ => self.build_google_search_url(query, lang, country),
        }
    }

    /// 构建Google搜索URL
    fn build_google_search_url(
        &self,
        query: &str,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> String {
        let mut query_params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("ie", "utf8".to_string()),
            ("oe", "utf8".to_string()),
        ];

        if let Some(l) = lang {
            let hl_value = if l.contains("-") {
                l.to_string()
            } else {
                format!("{}-{}", l, country.unwrap_or("US"))
            };
            query_params.push(("hl", hl_value));
        }

        if let Some(c) = country {
            query_params.push(("cr", format!("country{}", c.to_uppercase())));
        }

        let mut url = "https://www.google.com/search?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&query_string);

        url
    }

    /// 构建Bing搜索URL
    fn build_bing_search_url(
        &self,
        query: &str,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> String {
        let mut query_params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("form", "QBLH".to_string()),
            ("sp", "-1".to_string()),
            ("pq", query.to_string()),
            ("sc", "0-0".to_string()),
            ("qs", "n".to_string()),
            ("sk", "".to_string()),
        ];

        if let Some(l) = lang {
            query_params.push(("setlang", l.to_string()));
        }

        if let Some(c) = country {
            query_params.push(("cc", c.to_string()));
        }

        let mut url = "https://www.bing.com/search?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&query_string);

        url
    }

    /// 构建百度搜索URL
    fn build_baidu_search_url(
        &self,
        query: &str,
        lang: Option<&str>,
        _country: Option<&str>,
    ) -> String {
        let mut query_params: Vec<(&str, String)> =
            vec![("wd", query.to_string()), ("ie", "utf-8".to_string())];

        // 百度主要支持中文，语言参数处理简化
        if let Some(l) = lang {
            if l.starts_with("zh") {
                query_params.push(("cl", "3".to_string())); // 中文搜索
            }
        }

        let mut url = "https://www.baidu.com/s?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&query_string);

        url
    }

    /// 智能构建ScrapeRequest
    fn build_scrape_request(
        &self,
        url: &str,
        needs_js: bool,
        needs_tls_fingerprint: bool,
    ) -> ScrapeRequest {
        use rand::rng;

        let mut headers = HashMap::with_capacity(16);

        // 为所有请求类型添加完整的浏览器指纹
        let user_agents = [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0",
        ];

        let (user_agent, is_mobile) = match self.config.engine_type {
            SearchEngineType::Baidu => {
                // 百度使用PC端User-Agent更不容易被检测
                let ua = user_agents.choose(&mut rng()).unwrap_or(&user_agents[0]);
                (ua.to_string(), false)
            }
            _ => {
                let ua = user_agents.choose(&mut rng()).unwrap_or(&user_agents[0]);
                (ua.to_string(), false)
            }
        };

        headers.insert("User-Agent".to_string(), user_agent.clone());
        headers.insert("Accept".to_string(), "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8".to_string());
        headers.insert(
            "Accept-Language".to_string(),
            "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7".to_string(),
        );
        headers.insert(
            "Accept-Encoding".to_string(),
            "gzip, deflate, br".to_string(),
        );
        headers.insert("DNT".to_string(), "1".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());
        headers.insert("Upgrade-Insecure-Requests".to_string(), "1".to_string());
        headers.insert("Sec-Fetch-Dest".to_string(), "document".to_string());
        headers.insert("Sec-Fetch-Mode".to_string(), "navigate".to_string());
        headers.insert("Sec-Fetch-Site".to_string(), "none".to_string());
        headers.insert("Sec-Fetch-User".to_string(), "?1".to_string());
        headers.insert("Cache-Control".to_string(), "max-age=0".to_string());

        let (sec_ch_ua, sec_ch_ua_platform) = if user_agent.contains("Edg") {
            (
                "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Edge\";v=\"120\"".to_string(),
                "\"Windows\"".to_string(),
            )
        } else if user_agent.contains("Firefox") {
            (
                "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Firefox\";v=\"121\""
                    .to_string(),
                if is_mobile {
                    "\"Android\"".to_string()
                } else {
                    "\"Windows\"".to_string()
                },
            )
        } else if user_agent.contains("Safari") && user_agent.contains("Apple") {
            (
                "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Safari\";v=\"605.1\""
                    .to_string(),
                "\"macOS\"".to_string(),
            )
        } else {
            (
                "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\""
                    .to_string(),
                "\"Windows\"".to_string(),
            )
        };

        headers.insert("sec-ch-ua".to_string(), sec_ch_ua);
        headers.insert(
            "sec-ch-ua-mobile".to_string(),
            if is_mobile {
                "?1".to_string()
            } else {
                "?0".to_string()
            },
        );
        headers.insert("sec-ch-ua-platform".to_string(), sec_ch_ua_platform);

        // 搜索引擎特定的额外头
        match self.config.engine_type {
            SearchEngineType::Baidu => {
                headers.insert("Referer".to_string(), "https://www.baidu.com/".to_string());
                headers.insert("Origin".to_string(), "https://www.baidu.com".to_string());
            }
            SearchEngineType::Google => {
                headers.insert("Referer".to_string(), "https://www.google.com/".to_string());
            }
            SearchEngineType::Bing => {
                headers.insert("Referer".to_string(), "https://www.bing.com/".to_string());
            }
            SearchEngineType::Sogou => {
                headers.insert("Referer".to_string(), "https://www.sogou.com/".to_string());
            }
            _ => {
                headers.insert("Referer".to_string(), "https://www.google.com/".to_string());
            }
        }

        let actions = if needs_js {
            vec![
                PageAction::Wait { milliseconds: 3000 },
                PageAction::Scroll {
                    direction: ScrollDirection::Top,
                },
                PageAction::Wait { milliseconds: 1000 },
                PageAction::Scroll {
                    direction: ScrollDirection::Down,
                },
                PageAction::Wait { milliseconds: 1500 },
                PageAction::Scroll {
                    direction: ScrollDirection::Down,
                },
                PageAction::Wait { milliseconds: 1000 },
                PageAction::Scroll {
                    direction: ScrollDirection::Bottom,
                },
                PageAction::Wait { milliseconds: 3000 },
            ]
        } else {
            // 即使不需要JS，也为搜索引擎添加短暂等待
            vec![PageAction::Wait { milliseconds: 2000 }]
        };

        ScrapeRequest {
            url: url.to_string(),
            options: ScrapeOptions {
                headers,
                timeout: Duration::from_secs(self.config.timeout_seconds),
                needs_js,
                needs_screenshot: false,
                screenshot_config: None,
                mobile: false,
                proxy: None,
                skip_tls_verification: false,
                needs_tls_fingerprint,
                use_fire_engine: needs_js,
                actions,
                sync_wait_ms: if needs_js { 10000 } else { 0 },
            },
        }
    }

    /// 构建搜狗搜索URL
    fn build_sogou_search_url(
        &self,
        query: &str,
        lang: Option<&str>,
        _country: Option<&str>,
    ) -> String {
        let mut query_params: Vec<(&str, String)> = vec![("query", query.to_string())];

        if let Some(l) = lang {
            if l.starts_with("zh") {
                query_params.push(("safp", "d".to_string()));
            }
        }

        let mut url = "https://www.sogou.com/web?".to_string();
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        url.push_str(&query_string);

        url
    }

    /// 解析搜索结果（根据搜索引擎类型使用不同的解析器）
    fn parse_search_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        match self.config.engine_type {
            SearchEngineType::Google => self.parse_google_results(html),
            SearchEngineType::Bing => self.parse_bing_results(html),
            SearchEngineType::Baidu => self.parse_baidu_results(html),
            SearchEngineType::Sogou => self.parse_sogou_results(html),
            _ => self.parse_google_results(html),
        }
    }

    /// 解析 Google 搜索结果
    fn parse_google_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);
        let mut results = Vec::new();

        // Google 现代搜索结果容器选择器
        let result_selectors = vec![
            "div.g",
            "div[data-sokoban-container]",
            "div.MjjYud",
            "div.Ww4FFb",
            "div.v7W49e",
        ];

        for selector_str in result_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                let elements: Vec<_> = document.select(&selector).collect();
                if !elements.is_empty() {
                    // 使用第一个找到有效结果的选择器
                    for element in elements {
                        if let Some(result) = self.extract_google_result(&element) {
                            results.push(result);
                        }
                    }
                    if !results.is_empty() {
                        break;
                    }
                }
            }
        }

        info!("Google 解析完成，找到 {} 个结果", results.len());
        Ok(results)
    }

    /// 从 Google 结果元素中提取信息
    fn extract_google_result(&self, element: &scraper::ElementRef<'_>) -> Option<SearchResult> {
        use scraper::Selector;

        // 标题选择器（多个备用）
        let title_selectors = vec![
            "h3",
            "div[data-attrid='title']",
            "span.dvSrP",
            "div.v7W49e h3",
        ];

        let mut title = String::new();
        for selector_str in &title_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                if let Some(el) = element.select(&selector).next() {
                    title = self.escape_html(el.text().collect::<String>().trim());
                    if !title.is_empty() {
                        break;
                    }
                }
            }
        }

        // 链接选择器
        let link_selector = Selector::parse("a").ok()?;
        let mut url = String::new();
        for el in element.select(&link_selector) {
            if let Some(href) = el.value().attr("href") {
                if href.starts_with("http") && !href.contains("google.com") {
                    url = href.to_string();
                    break;
                }
            }
        }

        // 描述选择器
        let snippet_selectors = vec![
            "span[ae30]",
            "div[itemprop='description']",
            "div.yXK7ld",
            "div.zIBAzf",
            "span[style='color:#4d5156']",
        ];

        let mut description = String::new();
        for selector_str in &snippet_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                if let Some(el) = element.select(&selector).next() {
                    description = self.escape_html(el.text().collect::<String>().trim());
                    description = process_string(&description).unwrap_or(description);
                    if !description.is_empty() {
                        break;
                    }
                }
            }
        }

        if !title.is_empty() && !url.is_empty() {
            let scorer = RelevanceScorer::new("google_search");
            let engine_name = self.get_engine_name();
            let mut result = SearchResult::new(title, url, Some(description), engine_name);
            result.score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);
            Some(result)
        } else {
            None
        }
    }

    /// 解析 Bing 搜索结果
    fn parse_bing_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        let config = SearchResultParserConfig {
            result_selectors: vec!["li.b_algo", "div.sb_add"],
            title_selectors: vec!["h2", "a"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["p", "div"],
            engine_name: "bing",
            url_attr: None,
        };
        parse_search_results_common(html, config)
    }

    /// 解析百度搜索结果
    fn parse_baidu_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        let config = SearchResultParserConfig {
            result_selectors: vec!["div.c-container", "div.result"],
            title_selectors: vec!["h3 a", "a"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["div.c-abstract", "div"],
            engine_name: "baidu",
            url_attr: None,
        };
        parse_search_results_common(html, config)
    }

    /// 解析搜狗搜索结果
    fn parse_sogou_results(&self, html: &str) -> Result<Vec<SearchResult>, SearchError> {
        // 检测验证码页面
        if html.contains("验证码") || html.contains("seccode") || html.contains("verify") {
            warn!("Sogou returned CAPTCHA verification page");
            return Err(SearchError::Engine(
                "Sogou blocked the request with CAPTCHA verification. Try again later or use a different engine.".to_string(),
            ));
        }

        let document = Html::parse_document(html);
        let mut results = Vec::new();

        // 根据 temp/search.md 中的逆向工程结果
        // Sogou 的结果包裹在 class="vrwrap" 中
        let result_selector = safe_parse_selector("div.vrwrap")
            .expect("Failed to parse Sogou result selector: div.vrwrap");

        // 标题在 h3.vr-title > a 中
        let title_selector = safe_parse_selector("h3.vr-title a, h3 a")
            .expect("Failed to parse Sogou title selector");

        // URL 从 href 属性提取
        let link_selector = safe_parse_selector("h3.vr-title a, h3 a")
            .expect("Failed to parse Sogou link selector");

        // 摘要从 text-layout > p 中提取
        let snippet_selector = safe_parse_selector("div.text-layout p, div.ft p, p")
            .expect("Failed to parse Sogou snippet selector");

        for element in document.select(&result_selector) {
            // 提取标题
            let title_node = element.select(&title_selector).next();
            let title = match title_node {
                Some(e) => {
                    let text: String = e.text().collect();
                    text.trim().to_string()
                }
                None => String::new(),
            };

            if title.is_empty() {
                continue;
            }

            // 提取 URL - 处理内部重定向链接
            let link_node = element.select(&link_selector).next();
            let mut url = match link_node {
                Some(e) => e.value().attr("href").unwrap_or("").to_string(),
                None => String::new(),
            };

            // 处理 /link?url= 格式的重定向链接
            if url.starts_with("/link?url=") {
                url = format!("https://www.sogou.com{}", url);
            }

            // 提取摘要
            let snippet_node = element.select(&snippet_selector).next();
            let description = match snippet_node {
                Some(e) => {
                    let text: String = e.text().collect();
                    text.trim().to_string()
                }
                None => String::new(),
            };

            if !url.is_empty() {
                results.push(SearchResult::new(
                    title,
                    url,
                    Some(description),
                    "sogou".to_string(),
                ));
            }
        }

        info!("Parsed {} Sogou search results", results.len());
        Ok(results)
    }

    /// 保存HTML用于调试分析
    fn save_html_for_debug(&self, html: &str, query: &str) {
        if std::env::var("DEBUG_SAVE_HTML").is_ok() {
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let engine_name = match self.config.engine_type {
                SearchEngineType::Google => "google",
                SearchEngineType::Bing => "bing",
                SearchEngineType::Baidu => "baidu",
                SearchEngineType::Sogou => "sogou",
                _ => "smart_search",
            };
            let filename = format!(
                "/tmp/search_debug_{}_{}_{}.html",
                engine_name,
                query.replace(" ", "_"),
                timestamp
            );

            // 同时保存查询信息
            let debug_info = format!(
                "<!-- Search Query: {} -->\n<!-- Engine: {} -->\n<!-- Timestamp: {} -->\n{}\n",
                query, engine_name, timestamp, html
            );

            if let Err(e) = std::fs::write(&filename, &debug_info) {
                warn!("保存调试HTML失败: {}", e);
            } else {
                info!("已保存调试HTML到: {}", filename);
            }
        }
    }

    /// 应用相关性评分和新鲜度计算
    fn apply_scoring(&self, results: &mut Vec<SearchResult>, query: &str) {
        let scorer = RelevanceScorer::new(query);

        for result in &mut *results {
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
    }

    /// 获取引擎名称
    #[allow(dead_code)]
    fn engine_name(&self) -> &'static str {
        self.config.engine_type.name()
    }

    /// 获取引擎名称字符串
    fn get_engine_name(&self) -> String {
        self.engine_name().to_string()
    }

    /// 判断是否应该重试
    fn should_retry(&self, error: &EngineError) -> bool {
        error.is_retryable()
    }

    /// 处理重试等待
    async fn handle_retry(&self) {
        if self.config.retry_delay_ms > 0 {
            let delay = Duration::from_millis(self.config.retry_delay_ms);
            warn!("等待 {} 毫秒后重试", self.config.retry_delay_ms);
            tokio::time::sleep(delay).await;
        }
    }
}

#[async_trait]
impl SearchEngine for SmartSearchEngine {
    fn name(&self) -> &'static str {
        // 根据 engine_type 返回实际的引擎名称
        match self.config.engine_type {
            SearchEngineType::Google => "google",
            SearchEngineType::Bing => "bing",
            SearchEngineType::Baidu => "baidu",
            SearchEngineType::Sogou => "sogou",
            _ => "smart_search",
        }
    }

    fn engine_type(&self) -> SearchEngineType {
        self.config.engine_type
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    #[allow(deprecated)]
    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let query = &request.query;
        let limit = request.limit;
        let lang: Option<&str> = None;
        let country: Option<&str> = None;

        info!("智能搜索开始: query={}, limit={}", query, limit);

        // 检查速率限制
        self.check_rate_limit().await?;

        // 尝试加载测试数据
        if let Some(test_data) = self.load_test_data(query) {
            info!("使用测试数据进行搜索");
            let results = self.parse_test_data(&test_data)?;
            let mut scored_results = results;
            self.apply_scoring(&mut scored_results, query);
            scored_results.truncate(limit as usize);
            info!("返回 {} 个测试搜索结果", scored_results.len());

            let items: Vec<ResponseItem> = scored_results
                .into_iter()
                .map(|r| ResponseItem {
                    title: r.title,
                    url: r.url,
                    description: r.description.unwrap_or_default(),
                    engine: self.config.engine_type,
                })
                .collect();

            return Ok(Response {
                items,
                total_results: None,
                engine: self.config.engine_type,
            });
        }

        // 构建搜索URL
        let search_url = self.build_search_url(query, lang, country);
        info!("构建搜索URL: {}", search_url);

        // 智能判断是否需要JS和TLS指纹
        let (needs_js, needs_tls_fingerprint) = self.needs_js_and_tls();

        // 构建ScrapeRequest
        let scrape_request =
            self.build_scrape_request(&search_url, needs_js, needs_tls_fingerprint);

        info!(
            "使用智能路由进行抓取: needs_js={}, needs_tls_fingerprint={}",
            needs_js, needs_tls_fingerprint
        );

        // 执行搜索，支持重试
        let mut retries = 0;
        let scrape_response = loop {
            let timeout_duration = Duration::from_secs(self.config.timeout_seconds);
            let scrape_result =
                tokio::time::timeout(timeout_duration, self.engine_client.scrape(&scrape_request))
                    .await;

            match scrape_result {
                Ok(Ok(response)) => {
                    break Ok(response);
                }
                Ok(Err(e)) => {
                    warn!("智能路由抓取失败: {}", e);
                    if self.should_retry(&e) && retries < self.config.max_retries {
                        retries += 1;
                        self.handle_retry().await;
                        continue;
                    }
                    break Err(SearchError::Engine(format!("Smart routing failed: {}", e)));
                }
                Err(_) => {
                    warn!("智能路由抓取超时");
                    if retries < self.config.max_retries {
                        retries += 1;
                        self.handle_retry().await;
                        continue;
                    }
                    break Err(SearchError::Engine(format!(
                        "Timeout after {} seconds",
                        self.config.timeout_seconds
                    )));
                }
            }
        }?;

        info!("智能路由抓取成功，状态码: {}", scrape_response.status_code);

        let html = scrape_response.content;
        info!("搜索返回HTML长度: {} bytes", html.len());

        // 保存HTML用于调试分析
        self.save_html_for_debug(&html, query);

        // 如果HTML内容太少，可能是被拦截了
        if html.len() < 1000 {
            warn!("搜索返回的HTML内容过少，可能被反爬虫拦截");
            return Err(SearchError::Engine(
                "Search returned insufficient content".to_string(),
            ));
        }

        // 解析搜索结果
        let mut results = self.parse_search_results(&html)?;
        info!("解析到 {} 个搜索结果", results.len());

        // 应用相关性评分和新鲜度计算
        self.apply_scoring(&mut results, query);

        // 限制结果数量
        results.truncate(limit as usize);

        info!("返回 {} 个最终搜索结果", results.len());

        let items: Vec<ResponseItem> = results
            .into_iter()
            .map(|r| ResponseItem {
                title: r.title,
                url: r.url,
                description: r.description.unwrap_or_default(),
                engine: self.config.engine_type,
            })
            .collect();

        Ok(Response {
            items,
            total_results: None,
            engine: self.config.engine_type,
        })
    }
}

/// 创建Google智能搜索引擎
pub fn create_google_smart_search(engine_client: Arc<EngineClient>) -> Arc<dyn SearchEngine> {
    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Google,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 90,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };
    Arc::new(SmartSearchEngine::new(engine_client, config))
}

/// 创建Bing智能搜索引擎
pub fn create_bing_smart_search(engine_client: Arc<EngineClient>) -> Arc<dyn SearchEngine> {
    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Bing,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 90,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };
    Arc::new(SmartSearchEngine::new(engine_client, config))
}

/// 创建百度智能搜索引擎
pub fn create_baidu_smart_search(engine_client: Arc<EngineClient>) -> Arc<dyn SearchEngine> {
    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Baidu,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 60,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };
    Arc::new(SmartSearchEngine::new(engine_client, config))
}

/// 创建搜狗智能搜索引擎
pub fn create_sogou_smart_search(engine_client: Arc<EngineClient>) -> Arc<dyn SearchEngine> {
    let config = SmartSearchEngineConfig {
        engine_type: SearchEngineType::Sogou,
        rate_limiting_enabled: true,
        rate_limiting_service: None,
        timeout_seconds: 60,
        test_data_enabled: false,
        test_data_path: None,
        max_retries: 3,
        retry_delay_ms: 1000,
    };
    Arc::new(SmartSearchEngine::new(engine_client, config))
}

/// 创建带配置的智能搜索引擎
pub fn create_smart_search_engine(
    engine_client: Arc<EngineClient>,
    config: SmartSearchEngineConfig,
) -> Arc<dyn SearchEngine> {
    Arc::new(SmartSearchEngine::new(engine_client, config))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::engines::client::reqwest::ReqwestEngine;
    use crate::engines::router::EngineRouter;
    use crate::engines::traits::ScraperEngine;

    #[cfg(feature = "engine-playwright")]
    use crate::engines::client::playwright::PlaywrightEngine;

    fn create_test_client() -> Arc<EngineClient> {
        let reqwest_engine = Arc::new(ReqwestEngine::new());
        let mut engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine];

        #[cfg(feature = "engine-playwright")]
        {
            let playwright_engine = Arc::new(PlaywrightEngine);
            engines.push(playwright_engine);
        }

        let router = Arc::new(EngineRouter::new(engines));
        Arc::new(EngineClient::with_router(router))
    }

    fn create_test_config() -> SmartSearchEngineConfig {
        SmartSearchEngineConfig {
            engine_type: SearchEngineType::Google,
            rate_limiting_enabled: false,
            rate_limiting_service: None,
            timeout_seconds: 30,
            test_data_enabled: false,
            test_data_path: None,
            max_retries: 1,
            retry_delay_ms: 100,
        }
    }

    #[tokio::test]
    async fn test_smart_search_engine_creation() {
        let client = create_test_client();

        // 测试创建Google智能搜索引擎
        let google_engine = create_google_smart_search(client.clone());
        assert_eq!(google_engine.name(), "google");

        // 测试创建Bing智能搜索引擎
        let bing_engine = create_bing_smart_search(client.clone());
        assert_eq!(bing_engine.name(), "bing");

        // 测试创建百度智能搜索引擎
        let baidu_engine = create_baidu_smart_search(client.clone());
        assert_eq!(baidu_engine.name(), "baidu");
    }

    #[tokio::test]
    async fn test_smart_search_engine_with_config() {
        let client = create_test_client();
        let config = create_test_config();

        let smart_engine = Arc::new(SmartSearchEngine::new(client, config));
        assert_eq!(smart_engine.name(), "google");
    }

    #[test]
    fn test_build_search_url() {
        let client = create_test_client();
        let config = create_test_config();
        let smart_engine = SmartSearchEngine::new(client.clone(), config);

        // 测试Google搜索URL构建
        let google_url = smart_engine.build_search_url("rust programming", Some("en"), Some("US"));
        assert!(google_url.contains("google.com"));
        assert!(google_url.contains("rust"));
        assert!(google_url.contains("programming"));

        // 测试Bing搜索URL构建
        let mut bing_config = create_test_config();
        bing_config.engine_type = SearchEngineType::Bing;
        let bing_smart_engine = SmartSearchEngine::new(client, bing_config);
        let bing_url =
            bing_smart_engine.build_search_url("machine learning", Some("en"), Some("US"));
        assert!(bing_url.contains("bing.com"));
        assert!(bing_url.contains("machine"));
        assert!(bing_url.contains("learning"));
    }

    #[test]
    fn test_needs_js_and_tls() {
        let client = create_test_client();

        // 测试Google
        let mut google_config = create_test_config();
        google_config.engine_type = SearchEngineType::Google;
        let google_engine = SmartSearchEngine::new(client.clone(), google_config);
        let (needs_js_google, needs_tls_google) = google_engine.needs_js_and_tls();
        assert!(needs_js_google);
        assert!(!needs_tls_google);

        // 测试Bing
        let mut bing_config = create_test_config();
        bing_config.engine_type = SearchEngineType::Bing;
        let bing_engine = SmartSearchEngine::new(client.clone(), bing_config);
        let (needs_js_bing, needs_tls_bing) = bing_engine.needs_js_and_tls();
        assert!(needs_js_bing);
        assert!(!needs_tls_bing);

        // 测试百度
        let mut baidu_config = create_test_config();
        baidu_config.engine_type = SearchEngineType::Baidu;
        let baidu_engine = SmartSearchEngine::new(client.clone(), baidu_config);
        let (needs_js_baidu, needs_tls_baidu) = baidu_engine.needs_js_and_tls();
        assert!(!needs_js_baidu);
        assert!(!needs_tls_baidu);

        // 测试搜狗
        let mut sogou_config = create_test_config();
        sogou_config.engine_type = SearchEngineType::Sogou;
        let sogou_engine = SmartSearchEngine::new(client, sogou_config);
        let (needs_js_sogou, needs_tls_sogou) = sogou_engine.needs_js_and_tls();
        assert!(
            needs_js_sogou,
            "Sogou should need JS rendering for search results"
        );
        assert!(!needs_tls_sogou);
    }

    #[test]
    fn test_smart_search_engine_config_default() {
        let config = SmartSearchEngineConfig::default();
        assert_eq!(config.engine_type, SearchEngineType::Google);
        assert!(config.rate_limiting_enabled);
        assert_eq!(config.timeout_seconds, 90);
        assert!(!config.test_data_enabled);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay_ms, 1000);
    }

    #[test]
    fn test_create_smart_search_engine_with_config() {
        let client = create_test_client();
        let config = create_test_config();

        let engine = create_smart_search_engine(client, config);
        assert_eq!(engine.name(), "google");
    }
}
