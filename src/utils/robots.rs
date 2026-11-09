// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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

/// robots.txt 响应体上限（512 KiB；正常 robots.txt 远小于此值）
const MAX_ROBOTS_BODY_BYTES: usize = 512 * 1024;

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// robots.txt 抓取器（生产=直连 reqwest；测试可注入可控实现，
    /// 不走抓取引擎反爬链）
    fetcher: Arc<dyn RobotsFetcher>,

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
        Self {
            fetcher: Arc::new(HttpRobotsFetcher {
                client: http_client,
            }),
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
            RobotsFetch::FailClosed | RobotsFetch::Rejected => Ok(FetchedRobots {
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
        let mut last_error = None;
        let mut rejected = false;

        while attempt < self.retry_policy.max_retries {
            attempt += 1;

            match self.fetch_robots_once(robots_url).await {
                RobotsFetchAttempt::Content(content) => {
                    return RobotsFetch::Content(content);
                }
                RobotsFetchAttempt::Retryable(err) => {
                    last_error = Some(err);
                }
                RobotsFetchAttempt::Rejected(err) => {
                    log::warn!("robots fetch rejected (no retry): {}", err);
                    last_error = Some(err);
                    rejected = true;
                    break;
                }
            }

            if attempt < self.retry_policy.max_retries {
                let backoff = self.retry_policy.calculate_backoff(attempt);
                tokio::time::sleep(backoff).await;
            }
        }

        if rejected {
            // 不可重试拒绝：无论 fail_closed 与否都写短 TTL 失败标记，
            // 避免空内容被持久缓存 24h（审查 L2）。
            self.store_failure_marker(robots_url).await;
            return RobotsFetch::Rejected;
        }
        match last_error {
            Some(err) => self.handle_robots_fetch_error(robots_url, err).await,
            // max_retries=0：从未发起尝试，等价于历史空内容返回
            None => RobotsFetch::Content(String::new()),
        }
    }

    /// 单次抓取尝试：成功/404/其余非 5xx 结束本轮（内容或空串）；
    /// 5xx 与传输错误交给重试循环
    async fn fetch_robots_once(&self, robots_url: &str) -> RobotsFetchAttempt {
        // D5 修复：robots.txt 直连 HTTP，不走抓取引擎链。此前经 EngineRouter
        // 抓取，极短纯文本（如 `User-agent: *\nDisallow: /`）被反爬启发式
        // StructuralBlock 误判为「JS 空壳」并强制 needs_js，无 JS 引擎环境下
        // 抓取必败 → fail-open 放行全站 Disallow（E2E SB-17 实测）。SSRF 校验
        // 仍在调用方（get_robots_content）以 validate_url 前置执行。
        self.fetcher.fetch(robots_url, &self.user_agent).await
    }

    /// 写短 TTL 失败标记（60s），防止对不可达/非法目标站打爆。
    async fn store_failure_marker(&self, robots_url: &str) {
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

    /// 重试耗尽后的降级：fail-closed 写短 TTL 失败标记并判禁；fail-open 告警并放行空内容
    async fn handle_robots_fetch_error(&self, robots_url: &str, err: anyhow::Error) -> RobotsFetch {
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
            self.store_failure_marker(robots_url).await;
            return RobotsFetch::FailClosed;
        }
        log::warn!("Failed to fetch robots.txt from {}: {}", robots_url, err);
        // Default to empty content on persistent error
        RobotsFetch::Content(String::new())
    }

    /// 构造 robots.txt 抓取请求（5s 超时 + 配置的 User-Agent）
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
    /// 不可重试拒绝（超大响应体 / 私网地址）：写短 TTL 失败标记，
    /// 不写 24h 持久缓存（避免恶意站点污染规则缓存）。
    Rejected,
}

/// 单次 robots.txt 抓取尝试的结果（重试循环据此判定是否继续）
pub enum RobotsFetchAttempt {
    /// 已结束本轮（成功内容 / 404 空串 / 其余非 5xx 空串）
    Content(String),
    /// 5xx 或传输错误：按 retry_policy 重试
    Retryable(anyhow::Error),
    /// 不可重试的拒绝（超大响应体 / 解析到内网地址）：不再重试，避免
    /// 对同一非法目标反复请求；fail_closed=true 时按获取失败判禁。
    Rejected(anyhow::Error),
}

/// robots.txt 抓取器抽象。
///
/// 生产实现 [`HttpRobotsFetcher`] 直连 reqwest；测试注入可控实现
/// （本仓库约定：真实 trait 实现 + 测试专用行为，不引入 mock 库）。
#[async_trait]
pub trait RobotsFetcher: Send + Sync {
    /// 发起一次抓取尝试（不做重试与缓存）。
    async fn fetch(&self, robots_url: &str, user_agent: &str) -> RobotsFetchAttempt;
}

/// 生产抓取器：直连 HTTP（不走抓取引擎反爬链）。
struct HttpRobotsFetcher {
    client: Arc<reqwest::Client>,
}

#[async_trait]
impl RobotsFetcher for HttpRobotsFetcher {
    async fn fetch(&self, robots_url: &str, user_agent: &str) -> RobotsFetchAttempt {
        let response = self
            .client
            .get(robots_url)
            .header(reqwest::header::USER_AGENT, user_agent.to_string())
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        match response {
            Ok(mut resp) => {
                // 审查 L5：DNS 重绑定缓解——连接建立后校验对端地址，
                // 命中内网/回环立即按可重试失败处理，不读取响应体。
                match resp.remote_addr() {
                    Some(addr) if crate::engines::shared::is_private_ip(addr.ip()) => {
                        return RobotsFetchAttempt::Rejected(anyhow::anyhow!(
                            "Robots fetch resolved to private/reserved address: {}",
                            addr.ip()
                        ));
                    }
                    Some(_) => {}
                    None => {
                        // HTTP/2 连接池等场景可能取不到对端地址；静态 DNS 校验
                        // 与逐跳重定向策略仍在，记录 debug 后继续。
                        log::debug!(
                            "robots fetch remote_addr unavailable; relying on DNS validation"
                        );
                    }
                }
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    // 审查 M2：robots.txt 响应体限长（防恶意站点大体积响应）
                    let mut body: Vec<u8> = Vec::new();
                    loop {
                        match resp.chunk().await {
                            Ok(Some(chunk)) => {
                                if body.len() + chunk.len() > MAX_ROBOTS_BODY_BYTES {
                                    return RobotsFetchAttempt::Rejected(anyhow::anyhow!(
                                        "robots.txt exceeds {} bytes",
                                        MAX_ROBOTS_BODY_BYTES
                                    ));
                                }
                                body.extend_from_slice(&chunk);
                            }
                            Ok(None) => break,
                            Err(e) => {
                                return RobotsFetchAttempt::Retryable(anyhow::anyhow!(
                                    "Read body failed: {e}"
                                ));
                            }
                        }
                    }
                    RobotsFetchAttempt::Content(String::from_utf8_lossy(&body).into_owned())
                } else if status == 404 {
                    RobotsFetchAttempt::Content(String::new())
                } else if status >= 500 {
                    RobotsFetchAttempt::Retryable(anyhow::anyhow!("Server error: {status}"))
                } else {
                    RobotsFetchAttempt::Content(String::new())
                }
            }
            Err(e) => RobotsFetchAttempt::Retryable(anyhow::anyhow!("Request failed: {e}")),
        }
    }
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

#[cfg(test)]
#[path = "tests/robots/mod.rs"]
mod tests;
