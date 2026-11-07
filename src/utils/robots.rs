// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, HttpMethod, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::impl_basic_error_conversions;
use crate::infrastructure::oxcache::CacheService;
use crate::utils::retry_policy::RetryPolicy;
use anyhow::Result;
use robotstxt::DefaultMatcher;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;
use url::Url;

use async_trait::async_trait;

/// Robots.txt 检查器错误
#[derive(Error, Debug)]
pub enum RobotsCheckerError {
    #[error("Cache lock acquisition failed: {0}")]
    CacheLockError(String),

    #[error("URL parse failed: {0}")]
    UrlParseError(String),

    #[error("Validation failed: {0}")]
    ValidationError(String),
}

impl_basic_error_conversions!(RobotsCheckerError, ValidationError);

impl From<crate::infrastructure::security::ssrf::SsrfError> for RobotsCheckerError {
    fn from(err: crate::infrastructure::security::ssrf::SsrfError) -> Self {
        RobotsCheckerError::ValidationError(err.to_string())
    }
}

/// Robots.txt缓存统计
#[derive(Default, Clone)]
pub struct CacheStats {
    pub hits: Arc<AtomicU64>,
    pub misses: Arc<AtomicU64>,
}

impl CacheStats {
    /// 获取缓存命中次数
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// 获取缓存未命中次数
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// 记录缓存命中
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录缓存未命中
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }
}

/// Robots.txt检查器接口
#[async_trait]
pub trait RobotsCheckerTrait: Send + Sync {
    /// 检查URL是否被允许访问
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool>;
    /// 获取爬取延迟
    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>>;
}

/// 缓存的Robots.txt内容
#[derive(Clone)]
struct CachedRobots {
    /// 内容
    content: String,

    /// 过期时间
    expires_at: Instant,

    /// 是否为获取失败标记（重试耗尽）。`fail_closed=true` 时命中该标记判禁，
    /// 避免对不可达目标站反复发起 robots.txt 请求。
    fetch_failed: bool,
}

/// robots.txt 获取结果：内容 + 是否为获取失败标记
#[derive(Clone)]
struct FetchedRobots {
    content: String,
    fetch_failed: bool,
}

/// robots.txt 获取失败标记的缓存 TTL（短 TTL，防止对不可达目标站打爆）
const ROBOTS_FAILURE_CACHE_TTL: Duration = Duration::from_secs(60);

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// HTTP客户端 (Arc 包装，支持依赖注入)
    engine_client: Arc<EngineClient>,

    /// robots.txt 匹配与抓取使用的 User-Agent（可配置，见 RobotsSettings）
    user_agent: String,

    /// 内存缓存
    memory_cache: Arc<Mutex<HashMap<String, CachedRobots>>>,

    /// 缓存服务（可选，用于持久化缓存）
    cache_service: Option<Arc<dyn CacheService>>,

    /// robots.txt 缓存 TTL
    cache_ttl: Duration,

    /// 重试策略
    retry_policy: RetryPolicy,

    /// robots.txt 获取失败（重试耗尽）时是否判禁（fail-closed）。
    /// 默认 `false` 维持 fail-open 放行（见 `RobotsSettings::fail_closed`）。
    fail_closed: bool,

    /// 缓存统计
    cache_stats: Arc<CacheStats>,
}

#[async_trait]
impl RobotsCheckerTrait for RobotsChecker {
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
        let fetched = self.get_robots_content(url_str).await?;
        if fetched.fetch_failed && self.fail_closed {
            return Ok(false);
        }
        let url = Url::parse(url_str)?;
        let mut matcher = DefaultMatcher::default();
        // F6：匹配目标携带 query——`Disallow: /*?s=` 类规则以 query 为
        // 匹配特征，只传 path 会让此类规则永不命中
        let path_and_query = match url.query() {
            Some(q) => format!("{}?{}", url.path(), q),
            None => url.path().to_string(),
        };
        Ok(matcher.one_agent_allowed_by_robots(&fetched.content, user_agent, &path_and_query))
    }

    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>> {
        let fetched = self.get_robots_content(url_str).await?;
        Ok(self.parse_crawl_delay(&fetched.content, user_agent))
    }
}

impl RobotsChecker {
    /// 创建新的Robots检查器实例（通过依赖注入）
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP 客户端（通过依赖注入）
    /// * `cache_service` - 缓存服务（可选，用于持久化缓存）
    /// * `cache_stats` - 缓存统计（可选，用于追踪缓存命中率）
    ///
    /// # Returns
    ///
    /// 返回新的Robots检查器实例
    pub fn new(
        http_client: Arc<reqwest::Client>,
        settings: &crate::config::settings::RobotsSettings,
        cache_service: Option<Arc<dyn CacheService>>,
        cache_stats: Option<Arc<CacheStats>>,
    ) -> Self {
        let engine_client = Self::create_engine_client(http_client);
        Self {
            engine_client,
            user_agent: settings.user_agent.clone(),
            memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
            cache_service,
            cache_ttl: Duration::from_secs(settings.cache_ttl_secs),
            retry_policy: RetryPolicy {
                max_retries: settings.fetch_max_retries,
                initial_backoff: Duration::from_secs(settings.fetch_initial_backoff_secs),
                max_backoff: Duration::from_secs(settings.fetch_max_backoff_secs),
                ..Default::default()
            },
            fail_closed: settings.fail_closed,
            cache_stats: cache_stats.unwrap_or_else(|| Arc::new(CacheStats::default())),
        }
    }

    /// robots.txt 匹配与抓取使用的 User-Agent（来自 `robots.user_agent` 配置）
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// 获取Robots.txt内容（带缓存）
    ///
    /// 返回内容与是否为获取失败标记。获取失败（重试耗尽）时：
    /// - `fail_closed=true`：内容置空、`fetch_failed=true`，并按短 TTL
    ///   （60s）写入内存缓存防打爆；不写入 `cache_service`（其 TTL 长，
    ///   短时失败不应被长期缓存否决）。
    /// - `fail_closed=false`：维持现行 fail-open——空内容按正常 TTL 缓存。
    async fn get_robots_content(&self, url_str: &str) -> Result<FetchedRobots, RobotsCheckerError> {
        let robots_url = build_robots_url(url_str)?;

        // 1. Check memory cache
        if let Some(hit) = self.try_memory_cache(&robots_url).await {
            return Ok(hit);
        }

        self.cache_stats.record_miss();

        // 2. Check cache service（仅存成功获取的内容，fetch_failed 恒为 false）
        let cache_key = format!("robots_cache:{}", robots_url);
        if let Some(hit) = self.try_cache_service(&cache_key, &robots_url).await {
            return Ok(hit);
        }

        // SSRF protection
        crate::engines::validators::validate_url(&robots_url).await?;

        // 3. Fetch robots.txt with retry
        match self.fetch_robots_text(&robots_url).await {
            RobotsFetch::FailClosed => Ok(FetchedRobots {
                content: String::new(),
                fetch_failed: true,
            }),
            RobotsFetch::Content(content) => {
                self.store_fetched_content(&robots_url, &cache_key, &content)
                    .await;

                Ok(FetchedRobots {
                    content,
                    fetch_failed: false,
                })
            }
        }
    }

    /// 成功抓取后的持久化：按正常 TTL 写内存缓存 + 24h 写持久缓存
    async fn store_fetched_content(&self, robots_url: &str, cache_key: &str, content: &str) {
        // 4. Update memory cache
        self.store_memory_cache(robots_url, content).await;

        // 5. Update cache service
        if let Some(ref cache_service) = self.cache_service {
            let _ = cache_service.set(cache_key, content, 86400).await;
            // Cache for 24 hours
        }
    }

    /// 内存缓存查询：命中未过期 → 记 hit 并返回内容；过期条目移除
    async fn try_memory_cache(&self, robots_url: &str) -> Option<FetchedRobots> {
        let mut cache = self.memory_cache.lock().await;
        if let Some(cached) = cache.get(robots_url) {
            if cached.expires_at > Instant::now() {
                self.cache_stats.record_hit();
                return Some(FetchedRobots {
                    content: cached.content.clone(),
                    fetch_failed: cached.fetch_failed,
                });
            } else {
                cache.remove(robots_url);
            }
        }
        None
    }

    /// 持久缓存查询：命中 → 回填内存缓存（正常 TTL）并记 hit
    async fn try_cache_service(&self, cache_key: &str, robots_url: &str) -> Option<FetchedRobots> {
        if let Some(ref cache_service) = self.cache_service {
            if let Ok(Some(content)) = cache_service.get(cache_key).await {
                let mut cache = self.memory_cache.lock().await;
                cache.insert(
                    robots_url.to_string(),
                    CachedRobots {
                        content: content.clone(),
                        expires_at: Instant::now() + self.cache_ttl,
                        fetch_failed: false,
                    },
                );
                self.cache_stats.record_hit();
                return Some(FetchedRobots {
                    content,
                    fetch_failed: false,
                });
            }
        }
        None
    }

    /// 按 retry_policy 重试抓取 robots.txt；含 fail-closed / fail-open 降级语义
    async fn fetch_robots_text(&self, robots_url: &str) -> RobotsFetch {
        let mut attempt = 0;
        let mut content = String::new();
        let mut last_error = None;

        while attempt < self.retry_policy.max_retries {
            attempt += 1;
            let request = Self::build_robots_request(robots_url, &self.user_agent);

            let response = self.engine_client.scrape(&request).await;

            match response {
                Ok(resp) => {
                    if resp.is_success() {
                        content = resp.content;
                        last_error = None;
                        break;
                    } else if resp.status_code == 404 {
                        content = "".to_string();
                        last_error = None;
                        break;
                    } else if resp.status_code >= 500 {
                        last_error = Some(anyhow::anyhow!("Server error: {}", resp.status_code));
                    } else {
                        content = "".to_string();
                        last_error = None;
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Request failed: {}", e));
                }
            }

            if attempt < self.retry_policy.max_retries {
                let backoff = self.retry_policy.calculate_backoff(attempt);
                tokio::time::sleep(backoff).await;
            }
        }

        if let Some(err) = last_error {
            if self.fail_closed {
                log::warn!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "robots-fetch-fail-closed",
                        &[
                            ("url", fluent_bundle::FluentValue::from(robots_url)),
                            ("error", fluent_bundle::FluentValue::from(err.to_string())),
                        ],
                    )
                );
                // 失败标记按短 TTL 写入内存缓存，防止对不可达目标站打爆
                {
                    let mut cache = self.memory_cache.lock().await;
                    cache.insert(
                        robots_url.to_string(),
                        CachedRobots {
                            content: String::new(),
                            expires_at: Instant::now() + ROBOTS_FAILURE_CACHE_TTL,
                            fetch_failed: true,
                        },
                    );
                }
                return RobotsFetch::FailClosed;
            }
            log::warn!("Failed to fetch robots.txt from {}: {}", robots_url, err);
            // Default to empty content on persistent error
            content = "".to_string();
        }

        RobotsFetch::Content(content)
    }

    /// 构造 robots.txt 抓取请求（5s 超时 + 配置的 User-Agent）
    fn build_robots_request(robots_url: &str, user_agent: &str) -> ScrapeRequest {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), user_agent.to_string());

        ScrapeRequest::new(robots_url).with_options(
            ScrapeOptions::builder()
                .method(HttpMethod::Get)
                .headers(headers)
                .timeout(Duration::from_secs(5))
                .build(),
        )
    }

    /// 按正常 TTL 写入内存缓存（成功获取 / fail-open 降级共用）
    async fn store_memory_cache(&self, robots_url: &str, content: &str) {
        let mut cache = self.memory_cache.lock().await;
        cache.insert(
            robots_url.to_string(),
            CachedRobots {
                content: content.to_string(),
                expires_at: Instant::now() + self.cache_ttl, // Cache for 1 hour
                fetch_failed: false,
            },
        );
    }

    /// 解析Crawl-delay指令
    fn parse_crawl_delay(&self, content: &str, user_agent: &str) -> Option<Duration> {
        // 简单的解析逻辑：查找适用于该 User-Agent 的 Crawl-delay
        // 注意：这是一个简化的实现，不完全符合 RFC 规范，但足以处理大多数情况
        // 逻辑：
        // 1. 找到匹配的 User-agent 块
        // 2. 在块内查找 Crawl-delay

        let mut current_agent_matched = false;
        let mut delay: Option<f64> = None;
        let mut specific_agent_found = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            match classify_robots_line(line) {
                RobotsLine::UserAgent(agent) => {
                    if agent == "*" {
                        current_agent_matched = !specific_agent_found;
                    } else if user_agent.to_lowercase().contains(&agent.to_lowercase()) {
                        current_agent_matched = true;
                        specific_agent_found = true;
                        // Reset delay if we found a more specific agent
                        delay = None;
                    } else {
                        current_agent_matched = false;
                    }
                }
                RobotsLine::CrawlDelay(raw) if current_agent_matched => {
                    apply_crawl_delay_value(&mut delay, raw);
                }
                RobotsLine::Other | RobotsLine::CrawlDelay(_) => {}
            }
        }

        delay.map(Duration::from_secs_f64)
    }

    /// 获取缓存统计信息
    pub fn get_cache_stats(&self) -> (u64, u64) {
        (self.cache_stats.hits(), self.cache_stats.misses())
    }
}

/// 从目标 URL 构造 robots.txt URL（保留 scheme 与显式端口，缺省端口取知名默认）
fn build_robots_url(url_str: &str) -> Result<String, RobotsCheckerError> {
    let url = Url::parse(url_str).map_err(|e| RobotsCheckerError::UrlParseError(e.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| RobotsCheckerError::UrlParseError("Invalid URL: no host".to_string()))?;
    let scheme = url.scheme();
    let port = url.port_or_known_default().unwrap_or(80);

    Ok(format!("{}://{}:{}/robots.txt", scheme, host, port))
}

/// 重试抓取 robots.txt 的分类结果
enum RobotsFetch {
    /// fail_closed=true 且重试耗尽：内容置空、失败标记已按短 TTL 写入内存缓存
    FailClosed,
    /// 抓取成功 / 404 / fail-open 降级（内容可能为空串）
    Content(String),
}

/// robots.txt 行分类（指令前缀大小写不敏感）
enum RobotsLine<'a> {
    /// `user-agent:` 值（已 trim）
    UserAgent(&'a str),
    /// `crawl-delay:` 值（已 trim）
    CrawlDelay(&'a str),
    /// 其余行
    Other,
}

/// 按指令前缀分类一行 robots.txt（前缀比较大小写不敏感）
fn classify_robots_line(line: &str) -> RobotsLine<'_> {
    let lower_line = line.to_lowercase();
    if lower_line.starts_with("user-agent:") {
        RobotsLine::UserAgent(line[11..].trim())
    } else if lower_line.starts_with("crawl-delay:") {
        RobotsLine::CrawlDelay(line[12..].trim())
    } else {
        RobotsLine::Other
    }
}

/// 解析并应用 Crawl-delay 数值：负数 / inf / nan 视为无效，delay 维持原值
fn apply_crawl_delay_value(delay: &mut Option<f64>, raw: &str) {
    if let Ok(d) = raw.parse::<f64>() {
        // CWE-626：`Duration::from_secs_f64` 对负数/inf/nan 会 panic，
        // 恶意 robots.txt 不得中断爬取——视为无效指令，交由调用方默认策略
        if d.is_finite() && d >= 0.0 {
            *delay = Some(d);
        } else {
            log::warn!("Ignoring non-finite or negative Crawl-delay value: {}", d);
        }
    }
}

impl RobotsChecker {
    fn create_engine_client(http_client: Arc<reqwest::Client>) -> Arc<EngineClient> {
        let reqwest_engine = ReqwestEngine::new(http_client);
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
        Arc::new(EngineClient::with_router(router))
    }
}

#[cfg(test)]
#[path = "tests/robots/mod.rs"]
mod tests;
