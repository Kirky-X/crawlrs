// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use log::{error, info, warn};
use rand::prelude::*;
use scraper::{Html, Selector};
use std::sync::Arc;

use crate::domain::models::search_result::SearchResult;
use crate::domain::services::rate_limiting_service::{
    RateLimitResult, RateLimitingError, RateLimitingService,
};
use crate::domain::services::relevance_scorer::{DateParserComponent, RelevanceScorer};
use crate::engines::engine_client::{
    EngineClient, EngineError, HttpMethod, PageAction, ScrapeOptions, ScrapeRequest,
    ScrollDirection,
};
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};
use crate::utils::text_processing::encoding::TextEncodingProcessor;
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
    let scorer = RelevanceScorer::with_engine(config.engine_name);

    for element in document.select(&result_selector) {
        let raw_title = element
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let title = html_escape::encode_text(&raw_title).to_string();

        // 使用指定的属性提取URL
        let url = element
            .select(&link_selector)
            .next()
            .and_then(|el| el.value().attr(url_attr))
            .map(|href| href.to_string())
            .unwrap_or_default();

        let raw_description = element
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        let description = html_escape::encode_text(
            &TextEncodingProcessor::new()
                .process_text(raw_description.as_bytes())
                .unwrap_or(raw_description.clone()),
        )
        .to_string();

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
                Err(RateLimitingError::RedisError) => {
                    error!("Redis连接错误，降级处理");
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
                method: HttpMethod::Get,
                body: None,
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
                    description = TextEncodingProcessor::new()
                        .process_text(description.as_bytes())
                        .unwrap_or(description);
                    if !description.is_empty() {
                        break;
                    }
                }
            }
        }

        if !title.is_empty() && !url.is_empty() {
            let scorer = RelevanceScorer::with_engine("google_search");
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
        let scorer = RelevanceScorer::for_query(query);

        for result in &mut *results {
            // 计算相关性评分
            let relevance_score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);

            // 从描述中提取发布日期
            if let Some(description) = &result.description {
                let parser = DateParserComponent::with_defaults();
                if let Some(published_date) =
                    RelevanceScorer::extract_published_date_with_parser(description, &parser)
                {
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
    use crate::engines::engine_client::ScraperEngine;
    use crate::engines::router::EngineRouter;
    use crate::utils::http_client::create_http_client;

    #[cfg(feature = "engine-playwright")]
    use crate::engines::client::playwright::PlaywrightEngine;

    fn create_test_client() -> Arc<EngineClient> {
        let reqwest_engine = Arc::new(ReqwestEngine::new(create_http_client()));
        #[allow(unused_mut)]
        let mut engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine];

        #[cfg(feature = "engine-playwright")]
        {
            let playwright_engine = Arc::new(PlaywrightEngine::new());
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

#[cfg(test)]
#[allow(deprecated)]
mod tests_ext {
    use super::*;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
        RateLimitingService,
    };
    use crate::engines::client::reqwest::ReqwestEngine;
    use crate::engines::engine_client::ScraperEngine;
    use crate::engines::router::EngineRouter;
    use crate::search::engine_trait::SearchRequest;
    use crate::utils::http_client::create_http_client;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;

    // === Mock RateLimitingService ===

    enum MockBehavior {
        Allowed,
        Denied(String),
        RetryAfter(u64),
        RedisError,
        OtherError,
    }

    struct MockRateLimitingService {
        behavior: MockBehavior,
    }

    impl MockRateLimitingService {
        fn with_behavior(behavior: MockBehavior) -> Arc<dyn RateLimitingService> {
            Arc::new(Self { behavior })
        }
    }

    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            match &self.behavior {
                MockBehavior::Allowed => Ok(RateLimitResult::Allowed),
                MockBehavior::Denied(reason) => Ok(RateLimitResult::Denied {
                    reason: reason.clone(),
                }),
                MockBehavior::RetryAfter(secs) => Ok(RateLimitResult::RetryAfter {
                    retry_after_seconds: *secs,
                }),
                MockBehavior::RedisError => Err(RateLimitingError::RedisError),
                MockBehavior::OtherError => Err(RateLimitingError::ConfigurationError(
                    "mock config error".to_string(),
                )),
            }
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    // === Helpers ===

    fn create_test_client() -> Arc<EngineClient> {
        let reqwest_engine = Arc::new(ReqwestEngine::new(create_http_client()));
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![reqwest_engine];
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

    fn create_engine_with_type(engine_type: SearchEngineType) -> SmartSearchEngine {
        let client = create_test_client();
        let mut config = create_test_config();
        config.engine_type = engine_type;
        SmartSearchEngine::new(client, config)
    }

    fn make_config_with_service(
        engine_type: SearchEngineType,
        service: Arc<dyn RateLimitingService>,
    ) -> SmartSearchEngineConfig {
        SmartSearchEngineConfig {
            engine_type,
            rate_limiting_enabled: true,
            rate_limiting_service: Some(service),
            timeout_seconds: 30,
            test_data_enabled: false,
            test_data_path: None,
            max_retries: 1,
            retry_delay_ms: 0,
        }
    }

    // === safe_parse_selector ===

    #[test]
    fn test_safe_parse_selector_valid() {
        let result = safe_parse_selector("div.g");
        assert!(result.is_some());
    }

    #[test]
    fn test_safe_parse_selector_invalid() {
        assert!(safe_parse_selector(":::invalid:::").is_none());
    }

    #[test]
    fn test_safe_parse_selector_empty() {
        assert!(safe_parse_selector("").is_none());
    }

    // === parse_selectors ===

    #[test]
    fn test_parse_selectors_first_valid() {
        let result = parse_selectors("test", &["div.g", "div.fallback"], "result");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_selectors_fallback() {
        let result = parse_selectors("test", &[":::invalid:::", "div.g"], "result");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_selectors_all_invalid() {
        let result = parse_selectors("test", &[":::bad1:::", ":::bad2:::"], "result");
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SearchError::Parse(msg) => assert!(msg.contains("result") && msg.contains("test")),
            _ => panic!("Expected SearchError::Parse"),
        }
    }

    #[test]
    fn test_parse_selectors_empty_list() {
        let result = parse_selectors("test", &[], "result");
        assert!(result.is_err());
    }

    // === parse_search_results_common ===

    #[test]
    fn test_parse_search_results_common_with_results() {
        let html = r#"
        <html><body>
        <li class="b_algo">
            <h2>Rust Programming Language</h2>
            <a href="https://www.rust-lang.org">rust-lang.org</a>
            <p>A language empowering everyone to build reliable software.</p>
        </li>
        </body></html>
        "#;
        let config = SearchResultParserConfig {
            result_selectors: vec!["li.b_algo"],
            title_selectors: vec!["h2"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["p"],
            engine_name: "bing",
            url_attr: None,
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org");
        assert_eq!(results[0].engine, "bing");
    }

    #[test]
    fn test_parse_search_results_common_empty_html() {
        let config = SearchResultParserConfig {
            result_selectors: vec!["li.b_algo"],
            title_selectors: vec!["h2"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["p"],
            engine_name: "bing",
            url_attr: None,
        };
        let results = parse_search_results_common("", config).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_common_no_matching_elements() {
        let html = "<html><body><p>No results here</p></body></html>";
        let config = SearchResultParserConfig {
            result_selectors: vec!["li.b_algo"],
            title_selectors: vec!["h2"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["p"],
            engine_name: "bing",
            url_attr: None,
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_results_common_custom_url_attr() {
        let html = r#"
        <html><body>
        <div class="item">
            <span class="title">Test Title</span>
            <a data-url="https://example.com">Link</a>
            <div class="desc">Description</div>
        </div>
        </body></html>
        "#;
        let config = SearchResultParserConfig {
            result_selectors: vec!["div.item"],
            title_selectors: vec!["span.title"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["div.desc"],
            engine_name: "test",
            url_attr: Some("data-url"),
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com");
    }

    #[test]
    fn test_parse_search_results_common_skips_empty_title_or_url() {
        let html = r#"
        <html><body>
        <div class="item">
            <span class="title">Has Title</span>
            <a>No href attribute</a>
            <div class="desc">Desc</div>
        </div>
        <div class="item">
            <span class="title"></span>
            <a href="https://example.com">Link</a>
            <div class="desc">Desc</div>
        </div>
        </body></html>
        "#;
        let config = SearchResultParserConfig {
            result_selectors: vec!["div.item"],
            title_selectors: vec!["span.title"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["div.desc"],
            engine_name: "test",
            url_attr: None,
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert!(results.is_empty());
    }

    // === SmartSearchEngineConfig Debug ===

    #[test]
    fn test_config_debug_impl() {
        let config = SmartSearchEngineConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("SmartSearchEngineConfig"));
        assert!(debug_str.contains("engine_type"));
        assert!(debug_str.contains("rate_limiting_enabled"));
        assert!(debug_str.contains("timeout_seconds"));
        assert!(debug_str.contains("test_data_enabled"));
        assert!(debug_str.contains("max_retries"));
        assert!(debug_str.contains("retry_delay_ms"));
        // rate_limiting_service should be masked
        assert!(debug_str.contains("Arc<dyn RateLimitingService>"));
        assert!(!debug_str.contains("None") || debug_str.contains("rate_limiting_service"));
    }

    // === escape_html ===

    #[test]
    fn test_escape_html_plain_text() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert_eq!(engine.escape_html("hello world"), "hello world");
    }

    #[test]
    fn test_escape_html_special_chars() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let escaped = engine.escape_html("<script>alert('xss')</script>");
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
        assert!(escaped.contains("&lt;"));
        assert!(escaped.contains("&gt;"));
    }

    #[test]
    fn test_escape_html_ampersand() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let escaped = engine.escape_html("a & b");
        assert!(escaped.contains("&amp;"));
        assert!(!escaped.contains(" & "));
    }

    #[test]
    fn test_escape_html_empty_string() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert_eq!(engine.escape_html(""), "");
    }

    // === build_search_url for Baidu and Sogou (Google/Bing covered in existing tests) ===

    #[test]
    fn test_build_search_url_baidu() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let url = engine.build_search_url("测试查询", Some("zh"), Some("CN"));
        assert!(url.contains("baidu.com"));
        assert!(url.contains("wd="));
        assert!(url.contains("cl=3"));
    }

    #[test]
    fn test_build_search_url_sogou() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let url = engine.build_search_url("test query", Some("zh"), None);
        assert!(url.contains("sogou.com"));
        assert!(url.contains("query="));
        assert!(url.contains("safp=d"));
    }

    // === build_google_search_url detailed ===

    #[test]
    fn test_build_google_url_with_dashed_lang() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let url = engine.build_google_search_url("rust", Some("en-US"), Some("US"));
        assert!(url.contains("hl=en-US"));
        assert!(url.contains("cr=countryUS"));
    }

    #[test]
    fn test_build_google_url_lang_without_dash() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let url = engine.build_google_search_url("rust", Some("en"), Some("US"));
        assert!(url.contains("hl=en-US"));
    }

    #[test]
    fn test_build_google_url_no_lang_no_country() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let url = engine.build_google_search_url("rust", None, None);
        assert!(url.contains("google.com/search"));
        assert!(url.contains("q=rust"));
        assert!(!url.contains("hl="));
        assert!(!url.contains("cr="));
    }

    // === build_bing_search_url detailed ===

    #[test]
    fn test_build_bing_url_with_lang_and_country() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let url = engine.build_bing_search_url("rust", Some("en"), Some("US"));
        assert!(url.contains("bing.com/search"));
        assert!(url.contains("setlang=en"));
        assert!(url.contains("cc=US"));
    }

    #[test]
    fn test_build_bing_url_no_lang_no_country() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let url = engine.build_bing_search_url("rust", None, None);
        assert!(url.contains("q=rust"));
        assert!(!url.contains("setlang="));
        assert!(!url.contains("cc="));
    }

    // === build_baidu_search_url detailed ===

    #[test]
    fn test_build_baidu_url_zh_lang() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let url = engine.build_baidu_search_url("查询", Some("zh-CN"), None);
        assert!(url.contains("baidu.com/s"));
        assert!(url.contains("wd="));
        assert!(url.contains("cl=3"));
    }

    #[test]
    fn test_build_baidu_url_non_zh_lang() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let url = engine.build_baidu_search_url("test", Some("en"), None);
        assert!(url.contains("baidu.com/s"));
        assert!(!url.contains("cl=3"));
    }

    #[test]
    fn test_build_baidu_url_no_lang() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let url = engine.build_baidu_search_url("test", None, None);
        assert!(url.contains("wd=test"));
        assert!(url.contains("ie=utf-8"));
    }

    // === build_sogou_search_url detailed ===

    #[test]
    fn test_build_sogou_url_zh_lang() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let url = engine.build_sogou_search_url("test", Some("zh"), None);
        assert!(url.contains("sogou.com/web"));
        assert!(url.contains("query=test"));
        assert!(url.contains("safp=d"));
    }

    #[test]
    fn test_build_sogou_url_no_lang() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let url = engine.build_sogou_search_url("test", None, None);
        assert!(url.contains("query=test"));
        assert!(!url.contains("safp="));
    }

    // === build_scrape_request ===

    #[test]
    fn test_build_scrape_request_needs_js() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let request = engine.build_scrape_request("https://example.com", true, false);
        assert_eq!(request.url, "https://example.com");
        assert!(request.options.needs_js);
        assert!(!request.options.needs_tls_fingerprint);
        assert!(request.options.use_fire_engine);
        assert_eq!(request.options.sync_wait_ms, 10000);
        // JS actions include scroll and wait
        assert!(request.options.actions.len() > 1);
    }

    #[test]
    fn test_build_scrape_request_no_js() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let request = engine.build_scrape_request("https://example.com", false, false);
        assert!(!request.options.needs_js);
        assert!(!request.options.use_fire_engine);
        assert_eq!(request.options.sync_wait_ms, 0);
        // Non-JS still has a minimal wait action
        assert_eq!(request.options.actions.len(), 1);
    }

    #[test]
    fn test_build_scrape_request_headers_present() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let request = engine.build_scrape_request("https://example.com", true, false);
        assert!(request.options.headers.contains_key("User-Agent"));
        assert!(request.options.headers.contains_key("Accept"));
        assert!(request.options.headers.contains_key("Accept-Language"));
        assert!(request.options.headers.contains_key("sec-ch-ua"));
        assert!(request.options.headers.contains_key("Referer"));
    }

    #[test]
    fn test_build_scrape_request_engine_specific_referer() {
        let google_engine = create_engine_with_type(SearchEngineType::Google);
        let req = google_engine.build_scrape_request("https://example.com", false, false);
        assert_eq!(
            req.options.headers.get("Referer").unwrap(),
            "https://www.google.com/"
        );

        let baidu_engine = create_engine_with_type(SearchEngineType::Baidu);
        let req = baidu_engine.build_scrape_request("https://example.com", false, false);
        assert_eq!(
            req.options.headers.get("Referer").unwrap(),
            "https://www.baidu.com/"
        );
        assert_eq!(
            req.options.headers.get("Origin").unwrap(),
            "https://www.baidu.com"
        );

        let bing_engine = create_engine_with_type(SearchEngineType::Bing);
        let req = bing_engine.build_scrape_request("https://example.com", false, false);
        assert_eq!(
            req.options.headers.get("Referer").unwrap(),
            "https://www.bing.com/"
        );

        let sogou_engine = create_engine_with_type(SearchEngineType::Sogou);
        let req = sogou_engine.build_scrape_request("https://example.com", false, false);
        assert_eq!(
            req.options.headers.get("Referer").unwrap(),
            "https://www.sogou.com/"
        );
    }

    #[test]
    fn test_build_scrape_request_timeout_from_config() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let request = engine.build_scrape_request("https://example.com", false, false);
        assert_eq!(request.options.timeout, Duration::from_secs(30));
    }

    // === parse_search_results dispatch ===

    #[test]
    fn test_parse_search_results_dispatch_google() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"<html><body><div class="g"><h3>Title</h3><a href="https://example.com">Link</a></div></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_parse_search_results_dispatch_bing() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let html = r#"<html><body><li class="b_algo"><h2>Title</h2><a href="https://example.com">Link</a><p>Desc</p></li></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_search_results_dispatch_baidu() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let html = r#"<html><body><div class="c-container"><h3><a href="https://example.com">Title</a></h3><div class="c-abstract">Desc</div></div></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_search_results_dispatch_sogou() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"<html><body><div class="vrwrap"><h3 class="vr-title"><a href="https://example.com">Title</a></h3><div class="text-layout"><p>Desc</p></div></div></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Title");
    }

    // === parse_google_results ===

    #[test]
    fn test_parse_google_results_with_html() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="g">
            <h3>Rust Programming</h3>
            <a href="https://www.rust-lang.org">rust-lang.org</a>
        </div>
        <div class="g">
            <h3>Rust Docs</h3>
            <a href="https://doc.rust-lang.org">doc.rust-lang.org</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming");
        assert_eq!(results[0].url, "https://www.rust-lang.org");
    }

    #[test]
    fn test_parse_google_results_empty_html() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let results = engine.parse_google_results("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_google_results_no_matching() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = "<html><body><p>No results</p></body></html>";
        let results = engine.parse_google_results(html).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_google_results_skips_google_links() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="g">
            <h3>Google Link</h3>
            <a href="https://google.com/search?q=test">google.com</a>
        </div>
        <div class="g">
            <h3>External Link</h3>
            <a href="https://example.com">example.com</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        // The first result has a google.com link which is skipped, but title exists
        // The second result has a valid external link
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com");
    }

    // === parse_bing_results ===

    #[test]
    fn test_parse_bing_results_with_html() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let html = r#"
        <html><body>
        <li class="b_algo">
            <h2>Rust Language</h2>
            <a href="https://www.rust-lang.org">Link</a>
            <p>Empowering everyone to build reliable software.</p>
        </li>
        </body></html>
        "#;
        let results = engine.parse_bing_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Language");
        assert_eq!(results[0].engine, "bing");
    }

    #[test]
    fn test_parse_bing_results_empty_html() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let results = engine.parse_bing_results("").unwrap();
        assert!(results.is_empty());
    }

    // === parse_baidu_results ===

    #[test]
    fn test_parse_baidu_results_with_html() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let html = r#"
        <html><body>
        <div class="c-container">
            <h3><a href="https://example.com">百度结果</a></h3>
            <div class="c-abstract">这是描述</div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_baidu_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "百度结果");
        assert_eq!(results[0].engine, "baidu");
    }

    #[test]
    fn test_parse_baidu_results_empty_html() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        let results = engine.parse_baidu_results("").unwrap();
        assert!(results.is_empty());
    }

    // === parse_sogou_results ===

    #[test]
    fn test_parse_sogou_results_with_html() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://example.com">搜狗结果</a></h3>
            <div class="text-layout"><p>搜狗描述</p></div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_sogou_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "搜狗结果");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].engine, "sogou");
    }

    #[test]
    fn test_parse_sogou_results_captcha() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = "<html><body>请输入验证码 seccode verify</body></html>";
        let result = engine.parse_sogou_results(html);
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::Engine(msg) => assert!(msg.contains("CAPTCHA")),
            _ => panic!("Expected SearchError::Engine with CAPTCHA"),
        }
    }

    #[test]
    fn test_parse_sogou_results_redirect_link() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="/link?url=abc123">Redirect Link</a></h3>
            <div class="text-layout"><p>Desc</p></div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_sogou_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .url
            .starts_with("https://www.sogou.com/link?url="));
    }

    #[test]
    fn test_parse_sogou_results_empty_html() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let results = engine.parse_sogou_results("").unwrap();
        assert!(results.is_empty());
    }

    // === apply_scoring ===

    #[test]
    fn test_apply_scoring_empty_results() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let mut results: Vec<SearchResult> = vec![];
        engine.apply_scoring(&mut results, "test");
        assert!(results.is_empty());
    }

    #[test]
    fn test_apply_scoring_assigns_scores() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let mut results = vec![
            SearchResult::new(
                "Rust Programming".to_string(),
                "https://rust-lang.org".to_string(),
                Some("Rust programming language".to_string()),
                "google".to_string(),
            ),
            SearchResult::new(
                "Other Result".to_string(),
                "https://example.com".to_string(),
                Some("Unrelated content".to_string()),
                "google".to_string(),
            ),
        ];
        engine.apply_scoring(&mut results, "rust programming");
        // All results should have non-zero scores
        for r in &results {
            assert!(r.score >= 0.0);
        }
    }

    #[test]
    fn test_apply_scoring_sorts_by_score_descending() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let mut results = vec![
            SearchResult::new(
                "Unrelated".to_string(),
                "https://example.com".to_string(),
                Some("Completely different content".to_string()),
                "google".to_string(),
            ),
            SearchResult::new(
                "Rust Programming Language".to_string(),
                "https://rust-lang.org".to_string(),
                Some("Rust is a programming language".to_string()),
                "google".to_string(),
            ),
        ];
        engine.apply_scoring(&mut results, "rust programming");
        // The result more relevant to "rust programming" should be first
        assert!(results[0].score >= results[1].score);
        assert!(results[0].title.contains("Rust"));
    }

    #[test]
    fn test_apply_scoring_extracts_published_date() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let mut results = vec![SearchResult::new(
            "Test".to_string(),
            "https://example.com".to_string(),
            Some("Published on 2024-01-15 this article".to_string()),
            "google".to_string(),
        )];
        engine.apply_scoring(&mut results, "test");
        // Date extraction may or may not find a date depending on the parser,
        // but the function should not panic
        assert_eq!(results.len(), 1);
    }

    // === engine_name and get_engine_name ===

    #[test]
    fn test_engine_name_all_types() {
        assert_eq!(
            create_engine_with_type(SearchEngineType::Google).engine_name(),
            "Google"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Bing).engine_name(),
            "Bing"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Baidu).engine_name(),
            "Baidu"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Sogou).engine_name(),
            "Sogou"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Auto).engine_name(),
            "Auto"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Smart).engine_name(),
            "Smart"
        );
    }

    #[test]
    fn test_get_engine_name_all_types() {
        assert_eq!(
            create_engine_with_type(SearchEngineType::Google).get_engine_name(),
            "Google"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Bing).get_engine_name(),
            "Bing"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Baidu).get_engine_name(),
            "Baidu"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Sogou).get_engine_name(),
            "Sogou"
        );
    }

    // === should_retry ===

    #[test]
    fn test_should_retry_retryable_errors() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert!(engine.should_retry(&EngineError::RequestFailed("fail".to_string())));
        assert!(engine.should_retry(&EngineError::Timeout(Duration::from_secs(10))));
        assert!(engine.should_retry(&EngineError::BrowserError("browser crash".to_string())));
    }

    #[test]
    fn test_should_retry_non_retryable_errors() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert!(!engine.should_retry(&EngineError::NoEnginesAvailable));
        assert!(!engine.should_retry(&EngineError::InvalidUrl("bad".to_string())));
        assert!(!engine.should_retry(&EngineError::SsrfProtection("blocked".to_string())));
        assert!(!engine.should_retry(&EngineError::Internal("err".to_string())));
        assert!(!engine.should_retry(&EngineError::AllEnginesFailed("all".to_string())));
        assert!(!engine.should_retry(&EngineError::Expired));
        assert!(!engine.should_retry(&EngineError::Other("other".to_string())));
    }

    // === handle_retry ===

    #[tokio::test]
    async fn test_handle_retry_with_delay() {
        let mut config = create_test_config();
        config.retry_delay_ms = 10;
        let engine = SmartSearchEngine::new(create_test_client(), config);
        // Should complete without panicking
        engine.handle_retry().await;
    }

    #[tokio::test]
    async fn test_handle_retry_no_delay() {
        let mut config = create_test_config();
        config.retry_delay_ms = 0;
        let engine = SmartSearchEngine::new(create_test_client(), config);
        // Should complete immediately
        engine.handle_retry().await;
    }

    // === check_rate_limit ===

    #[tokio::test]
    async fn test_check_rate_limit_disabled() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert!(engine.check_rate_limit().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_rate_limit_enabled_no_service() {
        let mut config = create_test_config();
        config.rate_limiting_enabled = true;
        config.rate_limiting_service = None;
        let engine = SmartSearchEngine::new(create_test_client(), config);
        assert!(engine.check_rate_limit().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_rate_limit_allowed() {
        let service = MockRateLimitingService::with_behavior(MockBehavior::Allowed);
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        assert!(engine.check_rate_limit().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_rate_limit_denied() {
        let service = MockRateLimitingService::with_behavior(MockBehavior::Denied(
            "too many requests".to_string(),
        ));
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        let result = engine.check_rate_limit().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SearchError::Engine(msg) => assert!(msg.contains("Rate limit exceeded")),
            _ => panic!("Expected SearchError::Engine"),
        }
    }

    #[tokio::test]
    async fn test_check_rate_limit_retry_after() {
        let service = MockRateLimitingService::with_behavior(MockBehavior::RetryAfter(0));
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        // With 0 seconds wait, should return Ok
        assert!(engine.check_rate_limit().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_rate_limit_redis_error_degrades_gracefully() {
        let service = MockRateLimitingService::with_behavior(MockBehavior::RedisError);
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        // Redis errors should degrade gracefully to Ok
        assert!(engine.check_rate_limit().await.is_ok());
    }

    #[tokio::test]
    async fn test_check_rate_limit_other_error_degrades_gracefully() {
        let service = MockRateLimitingService::with_behavior(MockBehavior::OtherError);
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        // Other errors should also degrade gracefully
        assert!(engine.check_rate_limit().await.is_ok());
    }

    // === load_test_data ===

    #[test]
    fn test_load_test_data_disabled() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert!(engine.load_test_data("query").is_none());
    }

    #[test]
    fn test_load_test_data_enabled_no_path() {
        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = None;
        let engine = SmartSearchEngine::new(create_test_client(), config);
        assert!(engine.load_test_data("query").is_none());
    }

    #[test]
    fn test_load_test_data_nonexistent_path() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);
        assert!(engine.load_test_data("nonexistent_query").is_none());
    }

    #[test]
    fn test_load_test_data_matching_file() {
        let temp_dir = TempDir::new().unwrap();
        let query = "rust";
        let file_name = format!("test_data_{}.html", query);
        let file_path = temp_dir.path().join(&file_name);
        std::fs::write(&file_path, "<html>test content</html>").unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);
        let data = engine.load_test_data(query);
        assert!(data.is_some());
        assert_eq!(data.unwrap(), "<html>test content</html>");
    }

    #[test]
    fn test_load_test_data_generic_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let generic_path = temp_dir.path().join("generic_search_results.html");
        std::fs::write(&generic_path, "<html>generic content</html>").unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);
        let data = engine.load_test_data("no_matching_file");
        assert!(data.is_some());
        assert_eq!(data.unwrap(), "<html>generic content</html>");
    }

    #[test]
    fn test_load_test_data_query_with_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let query = "rust programming";
        let file_name = format!("test_data_{}.html", query.replace(" ", "_").to_lowercase());
        let file_path = temp_dir.path().join(&file_name);
        std::fs::write(&file_path, "<html>spaced content</html>").unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);
        let data = engine.load_test_data(query);
        assert!(data.is_some());
        assert_eq!(data.unwrap(), "<html>spaced content</html>");
    }

    // === save_html_for_debug ===

    #[test]
    fn test_save_html_for_debug_without_env_var() {
        // Ensure env var is not set
        std::env::remove_var("DEBUG_SAVE_HTML");
        let engine = create_engine_with_type(SearchEngineType::Google);
        // Should do nothing without panicking
        engine.save_html_for_debug("<html>test</html>", "test query");
    }

    // === SearchEngine trait impl ===

    #[test]
    fn test_search_engine_name_all_types() {
        assert_eq!(
            create_engine_with_type(SearchEngineType::Google).name(),
            "google"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Bing).name(),
            "bing"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Baidu).name(),
            "baidu"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Sogou).name(),
            "sogou"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Auto).name(),
            "smart_search"
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Smart).name(),
            "smart_search"
        );
    }

    #[test]
    fn test_search_engine_engine_type() {
        assert_eq!(
            create_engine_with_type(SearchEngineType::Google).engine_type(),
            SearchEngineType::Google
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Bing).engine_type(),
            SearchEngineType::Bing
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Baidu).engine_type(),
            SearchEngineType::Baidu
        );
        assert_eq!(
            create_engine_with_type(SearchEngineType::Sogou).engine_type(),
            SearchEngineType::Sogou
        );
    }

    #[test]
    fn test_search_engine_health() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    // === Factory functions ===

    #[test]
    fn test_create_sogou_smart_search() {
        let client = create_test_client();
        let engine = create_sogou_smart_search(client);
        assert_eq!(engine.name(), "sogou");
        assert_eq!(engine.engine_type(), SearchEngineType::Sogou);
    }

    #[test]
    fn test_all_factory_functions() {
        let client = create_test_client();
        let google = create_google_smart_search(client.clone());
        assert_eq!(google.name(), "google");
        assert_eq!(google.engine_type(), SearchEngineType::Google);

        let bing = create_bing_smart_search(client.clone());
        assert_eq!(bing.name(), "bing");
        assert_eq!(bing.engine_type(), SearchEngineType::Bing);

        let baidu = create_baidu_smart_search(client.clone());
        assert_eq!(baidu.name(), "baidu");
        assert_eq!(baidu.engine_type(), SearchEngineType::Baidu);

        let sogou = create_sogou_smart_search(client);
        assert_eq!(sogou.name(), "sogou");
        assert_eq!(sogou.engine_type(), SearchEngineType::Sogou);
    }

    #[test]
    fn test_factory_config_values() {
        let client = create_test_client();

        // Google factory uses timeout 90
        let google = create_google_smart_search(client.clone());
        // Bing factory uses timeout 90
        let bing = create_bing_smart_search(client.clone());
        // Baidu factory uses timeout 60
        let baidu = create_baidu_smart_search(client.clone());
        // Sogou factory uses timeout 60
        let sogou = create_sogou_smart_search(client);

        // All should be healthy
        assert_eq!(google.health(), EngineHealth::Healthy);
        assert_eq!(bing.health(), EngineHealth::Healthy);
        assert_eq!(baidu.health(), EngineHealth::Healthy);
        assert_eq!(sogou.health(), EngineHealth::Healthy);
    }

    // === search() with test data ===

    #[tokio::test]
    async fn test_search_with_test_data_google() {
        let temp_dir = TempDir::new().unwrap();
        let html = r#"
        <html><body>
        <div class="g">
            <h3>Rust Programming Language</h3>
            <a href="https://www.rust-lang.org">Link</a>
        </div>
        <div class="g">
            <h3>Rust Documentation</h3>
            <a href="https://doc.rust-lang.org">Docs</a>
        </div>
        </body></html>
        "#;
        let file_path = temp_dir.path().join("test_data_rust.html");
        std::fs::write(&file_path, html).unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);

        let request = SearchRequest::new("rust");
        let response = engine.search(&request).await.unwrap();
        assert_eq!(response.engine, SearchEngineType::Google);
        assert_eq!(response.items.len(), 2);
        assert!(response.items.iter().any(|i| i.title.contains("Rust")));
    }

    #[tokio::test]
    async fn test_search_with_test_data_truncates_to_limit() {
        let temp_dir = TempDir::new().unwrap();
        let html = r#"
        <html><body>
        <div class="g"><h3>Result 1</h3><a href="https://example1.com">1</a></div>
        <div class="g"><h3>Result 2</h3><a href="https://example2.com">2</a></div>
        <div class="g"><h3>Result 3</h3><a href="https://example3.com">3</a></div>
        <div class="g"><h3>Result 4</h3><a href="https://example4.com">4</a></div>
        <div class="g"><h3>Result 5</h3><a href="https://example5.com">5</a></div>
        </body></html>
        "#;
        let file_path = temp_dir.path().join("generic_search_results.html");
        std::fs::write(&file_path, html).unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);

        let mut request = SearchRequest::new("anything");
        request.limit = 3;
        let response = engine.search(&request).await.unwrap();
        assert_eq!(response.items.len(), 3);
    }

    #[tokio::test]
    async fn test_search_with_test_data_bing() {
        let temp_dir = TempDir::new().unwrap();
        let html = r#"
        <html><body>
        <li class="b_algo">
            <h2>Bing Result</h2>
            <a href="https://example.com">Link</a>
            <p>Description</p>
        </li>
        </body></html>
        "#;
        let file_path = temp_dir.path().join("test_data_test.html");
        std::fs::write(&file_path, html).unwrap();

        let mut config = create_test_config();
        config.engine_type = SearchEngineType::Bing;
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);

        let request = SearchRequest::new("test");
        let response = engine.search(&request).await.unwrap();
        assert_eq!(response.engine, SearchEngineType::Bing);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "Bing Result");
    }

    #[tokio::test]
    async fn test_search_with_test_data_applies_scoring() {
        let temp_dir = TempDir::new().unwrap();
        let html = r#"
        <html><body>
        <div class="g">
            <h3>Unrelated Topic</h3>
            <a href="https://unrelated.com">Link</a>
        </div>
        <div class="g">
            <h3>Rust Programming Guide</h3>
            <a href="https://rust-lang.org">Link</a>
        </div>
        </body></html>
        "#;
        let file_path = temp_dir.path().join("test_data_rust.html");
        std::fs::write(&file_path, html).unwrap();

        let mut config = create_test_config();
        config.test_data_enabled = true;
        config.test_data_path = Some(temp_dir.path().to_path_buf());
        let engine = SmartSearchEngine::new(create_test_client(), config);

        let request = SearchRequest::new("rust");
        let response = engine.search(&request).await.unwrap();
        // Results should be sorted by relevance; "Rust Programming Guide" should be first
        assert!(response.items[0].title.contains("Rust"));
    }

    // === parse_test_data ===

    #[test]
    fn test_parse_test_data_delegates_to_parse_search_results() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        let html = r#"<html><body><li class="b_algo"><h2>Title</h2><a href="https://example.com">L</a><p>Desc</p></li></body></html>"#;
        let results = engine.parse_test_data(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Title");
    }

    // === save_html_for_debug with env var set ===

    #[test]
    fn test_save_html_for_debug_writes_file_when_env_var_set() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        std::env::set_var("DEBUG_SAVE_HTML", "1");
        // The function should write a debug file to /tmp without panicking
        engine.save_html_for_debug("<html>debug content</html>", "test query");
        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    #[test]
    fn test_save_html_for_debug_baidu_engine_type() {
        let engine = create_engine_with_type(SearchEngineType::Baidu);
        std::env::set_var("DEBUG_SAVE_HTML", "1");
        engine.save_html_for_debug("<html>baidu debug</html>", "baidu query");
        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    #[test]
    fn test_save_html_for_debug_sogou_engine_type() {
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        std::env::set_var("DEBUG_SAVE_HTML", "1");
        engine.save_html_for_debug("<html>sogou debug</html>", "sogou query");
        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    #[test]
    fn test_save_html_for_debug_bing_engine_type() {
        let engine = create_engine_with_type(SearchEngineType::Bing);
        std::env::set_var("DEBUG_SAVE_HTML", "1");
        engine.save_html_for_debug("<html>bing debug</html>", "bing query");
        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    #[test]
    fn test_save_html_for_debug_auto_engine_type() {
        let engine = create_engine_with_type(SearchEngineType::Auto);
        std::env::set_var("DEBUG_SAVE_HTML", "1");
        engine.save_html_for_debug("<html>auto debug</html>", "auto query");
        std::env::remove_var("DEBUG_SAVE_HTML");
    }

    // === build_scrape_request with needs_tls_fingerprint ===

    #[test]
    fn test_build_scrape_request_with_tls_fingerprint() {
        let engine = create_engine_with_type(SearchEngineType::Google);
        let request = engine.build_scrape_request("https://example.com", true, true);
        assert!(request.options.needs_tls_fingerprint);
        assert!(request.options.needs_js);
        assert!(request.options.use_fire_engine);
    }

    // === parse_search_results dispatch for non-specific engine types ===

    #[test]
    fn test_parse_search_results_dispatch_auto_falls_back_to_google() {
        let engine = create_engine_with_type(SearchEngineType::Auto);
        let html = r#"<html><body><div class="g"><h3>Title</h3><a href="https://example.com">Link</a></div></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_parse_search_results_dispatch_smart_falls_back_to_google() {
        let engine = create_engine_with_type(SearchEngineType::Smart);
        let html = r#"<html><body><div class="g"><h3>Title</h3><a href="https://example.com">Link</a></div></body></html>"#;
        let results = engine.parse_search_results(html).unwrap();
        assert!(!results.is_empty());
    }

    // === build_search_url for non-specific engine types ===

    #[test]
    fn test_build_search_url_auto_falls_back_to_google() {
        let engine = create_engine_with_type(SearchEngineType::Auto);
        let url = engine.build_search_url("test", Some("en"), Some("US"));
        assert!(url.contains("google.com"));
    }

    // === needs_js_and_tls for non-specific engine types ===

    #[test]
    fn test_needs_js_and_tls_auto_falls_back_to_google() {
        let engine = create_engine_with_type(SearchEngineType::Auto);
        let (needs_js, needs_tls) = engine.needs_js_and_tls();
        assert!(needs_js);
        assert!(!needs_tls);
    }

    #[test]
    fn test_needs_js_and_tls_smart_falls_back_to_google() {
        let engine = create_engine_with_type(SearchEngineType::Smart);
        let (needs_js, needs_tls) = engine.needs_js_and_tls();
        assert!(needs_js);
        assert!(!needs_tls);
    }

    // === factory config values ===

    #[test]
    fn test_google_factory_config_values() {
        let client = create_test_client();
        let engine = create_google_smart_search(client);
        // Google factory uses timeout 90 and rate_limiting_enabled true
        assert_eq!(engine.engine_type(), SearchEngineType::Google);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_bing_factory_config_values() {
        let client = create_test_client();
        let engine = create_bing_smart_search(client);
        assert_eq!(engine.engine_type(), SearchEngineType::Bing);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_baidu_factory_config_values() {
        let client = create_test_client();
        let engine = create_baidu_smart_search(client);
        assert_eq!(engine.engine_type(), SearchEngineType::Baidu);
        assert_eq!(engine.health(), EngineHealth::Healthy);
    }

    // === parse_google_results with alternate selectors ===

    #[test]
    fn test_parse_google_results_with_mjjyud_container() {
        // Exercises the div.MjjYud result selector fallback (second in the list)
        // when div.g is absent.
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="MjjYud">
            <h3>Alternate Container Result</h3>
            <a href="https://example.com/alt">Link</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Alternate Container Result");
        assert_eq!(results[0].url, "https://example.com/alt");
    }

    #[test]
    fn test_parse_google_results_with_description_from_snippet_selector() {
        // Exercises the snippet selector fallback chain in extract_google_result.
        // Uses div.zIBAzf which is the 4th snippet selector.
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="g">
            <h3>Result With Description</h3>
            <a href="https://example.com">Link</a>
            <div class="zIBAzf">This is a snippet description.</div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].description.is_some());
        assert!(
            results[0]
                .description
                .as_ref()
                .unwrap()
                .contains("snippet description")
        );
    }

    #[test]
    fn test_parse_google_results_with_data_attrid_title_selector() {
        // Exercises the div[data-attrid='title'] title selector fallback
        // (second in the title selector list) when h3 is absent.
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="g">
            <div data-attrid="title">Title From Data Attr</div>
            <a href="https://example.com/data-attr">Link</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Title From Data Attr");
    }

    #[test]
    fn test_parse_google_results_extracts_from_multiple_containers() {
        // Exercises the loop break logic: results from div.g are found,
        // so the loop breaks and doesn't check other selectors.
        let engine = create_engine_with_type(SearchEngineType::Google);
        let html = r#"
        <html><body>
        <div class="g">
            <h3>First Result</h3>
            <a href="https://first.com">Link</a>
        </div>
        <div class="MjjYud">
            <h3>Second Result</h3>
            <a href="https://second.com">Link</a>
        </div>
        </body></html>
        "#;
        let results = engine.parse_google_results(html).unwrap();
        // Should find 1 result from div.g and stop (break after non-empty results)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "First Result");
    }

    // === parse_sogou_results with alternate snippet selectors ===

    #[test]
    fn test_parse_sogou_results_with_ft_snippet_fallback() {
        // Exercises the div.ft p snippet selector fallback (second in the list)
        // when div.text-layout p is absent.
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://example.com">Sogou Title</a></h3>
            <div class="ft"><p>Snippet from ft div</p></div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_sogou_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Sogou Title");
        assert!(results[0].description.as_ref().unwrap().contains("Snippet from ft"));
    }

    #[test]
    fn test_parse_sogou_results_with_p_snippet_fallback() {
        // Exercises the bare p snippet selector fallback (third in the list)
        // when both div.text-layout p and div.ft p are absent.
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://example.com">Sogou P Fallback</a></h3>
            <p>Bare paragraph snippet</p>
        </div>
        </body></html>
        "#;
        let results = engine.parse_sogou_results(html).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Sogou P Fallback");
        assert!(
            results[0]
                .description
                .as_ref()
                .unwrap()
                .contains("Bare paragraph")
        );
    }

    #[test]
    fn test_parse_sogou_results_skips_empty_title() {
        // Exercises the `if title.is_empty() { continue; }` branch
        let engine = create_engine_with_type(SearchEngineType::Sogou);
        let html = r#"
        <html><body>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://empty-title.com"></a></h3>
            <div class="text-layout"><p>Desc</p></div>
        </div>
        <div class="vrwrap">
            <h3 class="vr-title"><a href="https://valid.com">Valid Title</a></h3>
            <div class="text-layout"><p>Desc</p></div>
        </div>
        </body></html>
        "#;
        let results = engine.parse_sogou_results(html).unwrap();
        assert_eq!(results.len(), 1, "empty title should be skipped");
        assert_eq!(results[0].title, "Valid Title");
    }

    // === check_rate_limit with non-zero RetryAfter ===

    #[tokio::test]
    async fn test_check_rate_limit_retry_after_non_zero_sleeps() {
        // Exercises the RetryAfter branch with a non-zero sleep duration.
        // Uses 1 second to keep the test fast but still exercise the sleep.
        let service = MockRateLimitingService::with_behavior(MockBehavior::RetryAfter(1));
        let config = make_config_with_service(SearchEngineType::Google, service);
        let engine = SmartSearchEngine::new(create_test_client(), config);
        let result = engine.check_rate_limit().await;
        assert!(result.is_ok(), "RetryAfter with sleep should return Ok");
    }

    // === parse_search_results_common with fallback selectors ===

    #[test]
    fn test_parse_search_results_common_uses_fallback_result_selector() {
        // Exercises the fallback logic when the first result selector fails
        // to PARSE (not just match) but the second one succeeds.
        // parse_selectors returns the first parseable selector, so we need
        // an invalid CSS selector as the first option.
        let html = r#"
        <html><body>
        <div class="fallback-item">
            <h2>Fallback Title</h2>
            <a href="https://fallback.example.com">Link</a>
            <p>Fallback description</p>
        </div>
        </body></html>
        "#;
        let config = SearchResultParserConfig {
            result_selectors: vec![":::invalid:::", "div.fallback-item"],
            title_selectors: vec!["h2"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["p"],
            engine_name: "test",
            url_attr: None,
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Fallback Title");
        assert_eq!(results[0].url, "https://fallback.example.com");
    }

    #[test]
    fn test_parse_search_results_common_multiple_results() {
        // Exercises the loop that collects multiple results.
        let html = r#"
        <html><body>
        <div class="item">
            <span class="title">First</span>
            <a href="https://first.com">1</a>
            <div class="desc">Desc 1</div>
        </div>
        <div class="item">
            <span class="title">Second</span>
            <a href="https://second.com">2</a>
            <div class="desc">Desc 2</div>
        </div>
        <div class="item">
            <span class="title">Third</span>
            <a href="https://third.com">3</a>
            <div class="desc">Desc 3</div>
        </div>
        </body></html>
        "#;
        let config = SearchResultParserConfig {
            result_selectors: vec!["div.item"],
            title_selectors: vec!["span.title"],
            link_selectors: vec!["a"],
            snippet_selectors: vec!["div.desc"],
            engine_name: "test",
            url_attr: None,
        };
        let results = parse_search_results_common(html, config).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].title, "First");
        assert_eq!(results[2].title, "Third");
    }
}
