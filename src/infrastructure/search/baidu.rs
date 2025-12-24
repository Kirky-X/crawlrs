// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::domain::services::relevance_scorer::RelevanceScorer;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::info;

const MIN_REQUEST_INTERVAL_MS: u64 = 3000;
const MAX_REQUEST_INTERVAL_MS: u64 = 8000;

#[allow(dead_code)]
const MAX_RETRIES: u32 = 3;
#[allow(dead_code)]
const INITIAL_BACKOFF_MS: u64 = 1000;
#[allow(dead_code)]
const MAX_BACKOFF_MS: u64 = 30000;

const BAIDU_PC_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0",
];

const BAIDU_MOBILE_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Linux; Android 10; SM-G975F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.6099.43 Mobile Safari/537.36",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.6099.210 Mobile Safari/537.36",
];

#[derive(Clone)]
pub struct RateLimiter {
    last_request_time: Arc<Mutex<Instant>>,
    #[allow(dead_code)]
    min_interval: Duration,
    #[allow(dead_code)]
    max_interval: Duration,
    consecutive_failures: Arc<Mutex<u32>>,
}

impl RateLimiter {
    pub fn new(min_interval_ms: u64, max_interval_ms: u64) -> Self {
        Self {
            last_request_time: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10))),
            min_interval: Duration::from_millis(min_interval_ms),
            max_interval: Duration::from_millis(max_interval_ms),
            consecutive_failures: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn wait_before_request(&self) {
        let failures = *self.consecutive_failures.lock().await;
        let interval = if failures > 2 {
            let base_interval = Duration::from_millis(rand::random_range(
                MIN_REQUEST_INTERVAL_MS..MAX_REQUEST_INTERVAL_MS * 2,
            ));
            let failure_multiplier = 2u64.saturating_pow(failures - 2);
            let interval_ms = (base_interval.as_millis() as u64 * failure_multiplier).min(60_000);
            Duration::from_millis(interval_ms)
        } else {
            Duration::from_millis(rand::random_range(
                MIN_REQUEST_INTERVAL_MS..MAX_REQUEST_INTERVAL_MS,
            ))
        };

        let last_time = *self.last_request_time.lock().await;
        let elapsed = last_time.elapsed();

        if elapsed < interval {
            let jitter = Duration::from_millis(rand::random_range(0..1000));
            tokio::time::sleep(interval - elapsed + jitter).await;
        }

        *self.last_request_time.lock().await = Instant::now();
    }

    pub async fn record_failure(&self) {
        *self.consecutive_failures.lock().await += 1;
    }

    pub async fn record_success(&self) {
        *self.consecutive_failures.lock().await = 0;
    }

    pub async fn get_consecutive_failures(&self) -> u32 {
        *self.consecutive_failures.lock().await
    }
}

#[derive(Clone)]
pub struct UserAgentManager {
    pc_agents: Vec<String>,
    mobile_agents: Vec<String>,
    last_rotation: Arc<Mutex<Instant>>,
    rotation_interval: Duration,
}

impl UserAgentManager {
    pub fn new() -> Self {
        Self {
            pc_agents: BAIDU_PC_USER_AGENTS.iter().map(|s| s.to_string()).collect(),
            mobile_agents: BAIDU_MOBILE_USER_AGENTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            last_rotation: Arc::new(Mutex::new(Instant::now())),
            rotation_interval: Duration::from_secs(300),
        }
    }

    pub fn get_random_pc_ua(&self) -> String {
        self.pc_agents[rand::random_range(0..self.pc_agents.len())].clone()
    }

    pub fn get_random_mobile_ua(&self) -> String {
        self.mobile_agents[rand::random_range(0..self.mobile_agents.len())].clone()
    }

    pub async fn should_rotate(&self) -> bool {
        self.last_rotation.lock().await.elapsed() > self.rotation_interval
    }

    pub async fn record_rotation(&self) {
        *self.last_rotation.lock().await = Instant::now();
    }
}

impl Default for UserAgentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ProxyConfig {
    pub proxy_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    pub fn new(proxy_url: String, username: Option<String>, password: Option<String>) -> Self {
        Self {
            proxy_url,
            username,
            password,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BaiduSearchCategory {
    General,
    Images,
}

impl BaiduSearchCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            BaiduSearchCategory::General => "general",
            BaiduSearchCategory::Images => "images",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduResponse {
    feed: Option<BaiduFeed>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduFeed {
    entry: Option<Vec<BaiduEntry>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaiduEntry {
    title: Option<String>,
    url: Option<String>,
    abs: Option<String>, // 摘要字段
}

/// 配置文件中的搜索结果条目
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestSearchResultEntry {
    title: String,
    url: String,
    description: String,
}

/// 配置文件结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaiduTestConfig {
    results: Vec<TestSearchResultEntry>,
}

/// 加载测试结果配置
fn load_test_config() -> Option<BaiduTestConfig> {
    // 优先从配置文件读取
    let config_paths = vec![
        PathBuf::from("test-data/search-engines/test-results.yaml"),
        PathBuf::from("../test-data/search-engines/test-results.yaml"),
        PathBuf::from("../../test-data/search-engines/test-results.yaml"),
    ];

    for config_path in config_paths {
        if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => {
                    // 解析 YAML 配置文件
                    let config: HashMap<String, BaiduTestConfig> =
                        serde_yaml::from_str(&content).ok()?;

                    // 返回百度搜索测试配置
                    if let Some(baidu_config) = config.get("baidu") {
                        return Some(baidu_config.clone());
                    }
                }
                Err(_) => continue,
            }
        }
    }

    None
}

pub struct BaiduSearchEngine {
    #[allow(dead_code)]
    client: reqwest::Client,
    rate_limiter: RateLimiter,
    #[allow(dead_code)]
    user_agent_manager: UserAgentManager,
    #[allow(dead_code)]
    proxy_config: Option<ProxyConfig>,
}

impl Default for BaiduSearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BaiduSearchEngine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            rate_limiter: RateLimiter::new(MIN_REQUEST_INTERVAL_MS, MAX_REQUEST_INTERVAL_MS),
            user_agent_manager: UserAgentManager::new(),
            proxy_config: None,
        }
    }

    pub fn with_proxy(
        proxy_url: String,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            rate_limiter: RateLimiter::new(MIN_REQUEST_INTERVAL_MS, MAX_REQUEST_INTERVAL_MS),
            user_agent_manager: UserAgentManager::new(),
            proxy_config: Some(ProxyConfig::new(proxy_url, username, password)),
        }
    }

    #[allow(dead_code)]
    fn create_request_builder(&self, url: &str) -> reqwest::RequestBuilder {
        let use_mobile = rand::random::<f32>() < 0.2;
        let user_agent = if use_mobile {
            self.user_agent_manager.get_random_mobile_ua()
        } else {
            self.user_agent_manager.get_random_pc_ua()
        };

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Accept",
            reqwest::header::HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
        );
        headers.insert(
            "Accept-Language",
            reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );
        headers.insert(
            "Accept-Encoding",
            reqwest::header::HeaderValue::from_static("gzip, deflate, br"),
        );
        headers.insert(
            "Cache-Control",
            reqwest::header::HeaderValue::from_static("no-cache"),
        );
        headers.insert(
            "Pragma",
            reqwest::header::HeaderValue::from_static("no-cache"),
        );
        headers.insert(
            "Sec-Ch-Ua",
            reqwest::header::HeaderValue::from_static(
                "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\"",
            ),
        );
        headers.insert(
            "Sec-Ch-Ua-Mobile",
            reqwest::header::HeaderValue::from_static(if use_mobile { "?1" } else { "?0" }),
        );
        headers.insert(
            "Sec-Ch-Ua-Platform",
            reqwest::header::HeaderValue::from_static("\"Windows\""),
        );
        headers.insert(
            "Sec-Fetch-Dest",
            reqwest::header::HeaderValue::from_static("document"),
        );
        headers.insert(
            "Sec-Fetch-Mode",
            reqwest::header::HeaderValue::from_static("navigate"),
        );
        headers.insert(
            "Sec-Fetch-Site",
            reqwest::header::HeaderValue::from_static("none"),
        );
        headers.insert(
            "Sec-Fetch-User",
            reqwest::header::HeaderValue::from_static("?1"),
        );
        headers.insert(
            "Upgrade-Insecure-Requests",
            reqwest::header::HeaderValue::from_static("1"),
        );

        let mut request_builder = self.client.get(url).headers(headers);

        request_builder = request_builder.header("User-Agent", user_agent);

        request_builder
    }

    #[allow(dead_code)]
    async fn execute_with_retry<F, T, E>(&self, operation: F) -> Result<T, SearchError>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
        E: std::fmt::Display,
    {
        let mut last_error: Option<String> = None;

        for attempt in 0..MAX_RETRIES {
            match operation().await {
                Ok(result) => {
                    self.rate_limiter.record_success().await;
                    return Ok(result);
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    last_error = Some(error_msg.clone());

                    self.rate_limiter.record_failure().await;

                    if attempt < MAX_RETRIES - 1 {
                        let backoff_ms = INITIAL_BACKOFF_MS
                            .saturating_mul(2u64.saturating_pow(attempt))
                            .min(MAX_BACKOFF_MS);
                        let jitter = rand::random_range(0..backoff_ms / 2);
                        let total_delay = backoff_ms + jitter;

                        tracing::warn!(
                            "Baidu request attempt {} failed: {}, retrying in {}ms",
                            attempt + 1,
                            error_msg,
                            total_delay
                        );

                        tokio::time::sleep(Duration::from_millis(total_delay)).await;
                    }
                }
            }
        }

        Err(SearchError::EngineError(format!(
            "Baidu request failed after {} attempts: {}",
            MAX_RETRIES,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )))
    }

    #[allow(dead_code)]
    fn is_captcha_page(&self, content: &str) -> bool {
        content.contains("captcha")
            || content.contains("wappass.baidu.com")
            || content.contains("验证码")
            || content.contains("请输入验证码")
            || content.contains("百度安全验证")
            || content.contains("/static/antispam")
            || content.contains("antispam")
    }

    #[allow(dead_code)]
    fn is_rate_limited(&self, content: &str, status_code: u16) -> bool {
        status_code == 429
            || status_code == 403
            || content.contains("请求过于频繁")
            || content.contains("访问频率过快")
            || content.contains("Too Many Requests")
    }

    async fn handle_search_with_anti_crawl(
        &self,
        query: &str,
        page: u32,
        category: BaiduSearchCategory,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.rate_limiter.wait_before_request().await;

        let (url, params) = self.build_baidu_url(query, page, category);

        let use_mobile = rand::random::<f32>() < 0.2;
        let user_agent = if use_mobile {
            "Mozilla/5.0 (Linux; Android 10; SM-G975F) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.6099.43 Mobile Safari/537.36".to_string()
        } else {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string()
        };

        let result: Result<Vec<SearchResult>, SearchError> = async move {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                "Accept",
                reqwest::header::HeaderValue::from_static("application/json, text/plain, */*"),
            );
            headers.insert(
                "Accept-Language",
                reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9"),
            );
            headers.insert(
                "Referer",
                reqwest::header::HeaderValue::from_static("https://www.baidu.com/"),
            );
            headers.insert(
                "User-Agent",
                reqwest::header::HeaderValue::from_str(&user_agent).unwrap(),
            );

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .default_headers(headers)
                .build()
                .map_err(|e| SearchError::EngineError(e.to_string()))?;

            let request = client.get(&url).query(&params);

            let response = request
                .send()
                .await
                .map_err(|e| SearchError::EngineError(e.to_string()))?;

            let status = response.status();

            if status == 429 || status == 403 {
                return Err(SearchError::EngineError(format!(
                    "Rate limited with status: {}",
                    status
                )));
            }

            if !status.is_success() {
                return Err(SearchError::EngineError(format!("HTTP error: {}", status)));
            }

            let json_content = response
                .text()
                .await
                .map_err(|e| SearchError::EngineError(e.to_string()))?;

            if json_content.contains("captcha")
                || json_content.contains("wappass.baidu.com")
                || json_content.contains("验证码")
                || json_content.contains("请输入验证码")
            {
                return Err(SearchError::EngineError("CAPTCHA required".to_string()));
            }

            if json_content.trim_start().starts_with('<') {
                return Err(SearchError::EngineError(
                    "Received HTML instead of JSON".to_string(),
                ));
            }

            let results = serde_json::from_str::<BaiduResponse>(&json_content)
                .map_err(|e| SearchError::EngineError(format!("JSON parsing error: {}", e)))?;

            let mut search_results = Vec::new();

            if let Some(feed) = results.feed {
                if let Some(entries) = feed.entry {
                    for entry in entries {
                        if let (Some(title), Some(url)) = (entry.title, entry.url) {
                            let decoded_title =
                                html_escape::decode_html_entities(&title).to_string();
                            let decoded_content = entry
                                .abs
                                .as_ref()
                                .map(|abs| html_escape::decode_html_entities(abs).to_string());

                            let scorer = RelevanceScorer::new("");
                            let relevance_score = scorer.calculate_score(
                                &decoded_title,
                                decoded_content.as_deref(),
                                &url,
                            );

                            let mut search_result = SearchResult::new(
                                decoded_title,
                                url.clone(),
                                decoded_content,
                                "baidu".to_string(),
                            );
                            search_result.score = relevance_score;
                            search_results.push(search_result);
                        }
                    }
                }
            }

            Ok(search_results)
        }
        .await;

        match result {
            Ok(results) => {
                self.rate_limiter.record_success().await;
                Ok(results)
            }
            Err(e) => {
                self.rate_limiter.record_failure().await;
                Err(e)
            }
        }
    }

    /// 从配置创建搜索结果
    fn create_search_results_from_config(
        &self,
        config: &BaiduTestConfig,
        engine_name: &str,
    ) -> Vec<SearchResult> {
        config
            .results
            .iter()
            .map(|entry| {
                let scorer = RelevanceScorer::new("");
                let relevance_score =
                    scorer.calculate_score(&entry.title, Some(&entry.description), &entry.url);

                let mut result = SearchResult::new(
                    entry.title.clone(),
                    entry.url.clone(),
                    Some(entry.description.clone()),
                    engine_name.to_string(),
                );
                result.score = relevance_score;
                result
            })
            .collect()
    }

    /// 构建百度搜索URL，支持多端点
    pub fn build_baidu_url(
        &self,
        query: &str,
        page: u32,
        category: BaiduSearchCategory,
    ) -> (String, HashMap<String, String>) {
        let page_size = 10;
        let offset = (page - 1) * page_size;

        let (url, params) = match category {
            BaiduSearchCategory::General => {
                // 通用搜索 API
                let url = "https://www.baidu.com/s".to_string();
                let mut params = HashMap::new();
                params.insert("wd".to_string(), query.to_string());
                params.insert("rn".to_string(), page_size.to_string());
                params.insert("pn".to_string(), offset.to_string());
                params.insert("tn".to_string(), "json".to_string()); // 关键参数：请求 JSON 响应
                (url, params)
            }
            BaiduSearchCategory::Images => {
                // 图片搜索 API
                let url = "https://image.baidu.com/search/acjson".to_string();
                let mut params = HashMap::new();
                params.insert("word".to_string(), query.to_string());
                params.insert("rn".to_string(), page_size.to_string());
                params.insert("pn".to_string(), offset.to_string());
                params.insert("tn".to_string(), "resultjson_com".to_string());
                (url, params)
            }
        };

        (url, params)
    }

    /// 解析百度JSON响应
    pub fn parse_baidu_response(&self, json_text: &str) -> Result<Vec<SearchResult>, SearchError> {
        let data: BaiduResponse = serde_json::from_str(json_text)
            .map_err(|e| SearchError::EngineError(format!("JSON parsing error: {}", e)))?;

        let mut results = Vec::new();

        // 检查是否有结果
        if let Some(feed) = data.feed {
            if let Some(entries) = feed.entry {
                for entry in entries {
                    if let (Some(title), Some(url)) = (entry.title, entry.url) {
                        // HTML转义字符解码
                        let decoded_title = html_escape::decode_html_entities(&title).to_string();
                        let decoded_content = entry
                            .abs
                            .as_ref()
                            .map(|abs| html_escape::decode_html_entities(abs).to_string());

                        // 计算相关性分数
                        let scorer = RelevanceScorer::new(""); // 将在search方法中设置正确的查询词
                        let relevance_score = scorer.calculate_score(
                            &decoded_title,
                            decoded_content.as_deref(),
                            &url,
                        );

                        let mut search_result = SearchResult::new(
                            decoded_title,
                            url.clone(),
                            decoded_content, // 使用原始值，因为calculate_score只需要引用
                            "baidu".to_string(),
                        );

                        search_result.score = relevance_score;
                        results.push(search_result);
                    }
                }
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl SearchEngine for BaiduSearchEngine {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // 优先从配置文件读取测试结果
        if std::env::var("USE_TEST_DATA").is_ok() {
            info!("Using test data from configuration file for Baidu search");
            if let Some(config) = load_test_config() {
                let mut results = self.create_search_results_from_config(&config, "baidu");
                results.truncate(limit as usize);
                return Ok(results);
            }
        }

        // 保留环境变量兼容性作为备选
        if std::env::var("BAIDU_TEST_RESULTS").is_ok() {
            info!("Using fallback test results from environment for Baidu search");
            return Ok(vec![SearchResult::new(
                "Baidu Ernie Bot - AI Chatbot".to_string(),
                "https://yiyan.baidu.com/".to_string(),
                Some(
                    "Ernie Bot is Baidu's AI chatbot powered by Ernie (Enhanced Representation of Knowledge Integration) large language model."
                        .to_string(),
                ),
                "baidu".to_string(),
            )]);
        }

        // 默认使用通用搜索，可以通过参数或配置扩展到支持图片搜索
        let category = BaiduSearchCategory::General;
        let page = 1;

        let mut results = self
            .handle_search_with_anti_crawl(query, page, category)
            .await?;

        results.truncate(limit as usize);

        let scorer = RelevanceScorer::new(query);
        for result in &mut results {
            let relevance_score =
                scorer.calculate_score(&result.title, result.description.as_deref(), &result.url);

            if let Some(published_date) = RelevanceScorer::extract_published_date(&result.title) {
                result.published_time = Some(published_date);
            }

            let freshness_score = if let Some(published_time) = result.published_time {
                RelevanceScorer::calculate_freshness_score(published_time)
            } else {
                0.5
            };

            result.score = relevance_score * 0.7 + freshness_score * 0.3;
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(results)
    }

    fn name(&self) -> &'static str {
        "baidu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_test_config_from_file() {
        std::env::set_var("USE_TEST_DATA", "1");

        let config = load_test_config();
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.results.len(), 3);

        std::env::remove_var("USE_TEST_DATA");
    }

    #[tokio::test]
    async fn test_create_search_results_from_config() {
        let engine = BaiduSearchEngine::new();
        let config = BaiduTestConfig {
            results: vec![
                TestSearchResultEntry {
                    title: "Test Result 1".to_string(),
                    url: "https://example1.com".to_string(),
                    description: "Description 1".to_string(),
                },
                TestSearchResultEntry {
                    title: "Test Result 2".to_string(),
                    url: "https://example2.com".to_string(),
                    description: "Description 2".to_string(),
                },
            ],
        };

        let results = engine.create_search_results_from_config(&config, "baidu");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Test Result 1");
        assert_eq!(results[0].url, "https://example1.com");
        assert_eq!(results[1].engine, "baidu");
    }
}
