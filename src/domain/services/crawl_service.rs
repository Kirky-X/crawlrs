// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! CrawlService - Core business logic for web crawling

#![allow(unused_variables)]

/// Load degradation thresholds
const HIGH_LOAD_THRESHOLD: f64 = 0.8;
const MEDIUM_LOAD_THRESHOLD: f64 = 0.6;
const MEDIUM_LOAD_DEPTH_FACTOR: f64 = 0.75;

use crate::domain::models::{DomainError, Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::robots::RobotsCheckerTrait;
use anyhow::Result;
use chrono::Utc;
use log::debug;
use regex::Regex;
use scraper::{Html, Selector};
use shaku::{Component, Interface};
use std::collections::HashSet;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

/// 爬取服务接口
#[async_trait::async_trait]
pub trait CrawlServiceTrait: Interface + Send + Sync {
    /// 处理爬取结果
    async fn process_crawl_result(
        &self,
        parent_task: &Task,
        html_content: &str,
    ) -> Result<Vec<Task>, DomainError>;
}

/// 爬取服务
///
/// 处理网站爬取任务的核心业务逻辑
#[derive(Component)]
#[shaku(interface = CrawlServiceTrait)]
pub struct CrawlService {
    /// 任务仓库
    #[shaku(inject)]
    repo: Arc<dyn TaskRepository>,
    /// Robots.txt检查器
    #[shaku(inject)]
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    /// 系统监控器（用于负载降级）
    #[cfg(feature = "metrics")]
    #[shaku(inject)]
    system_monitor: Arc<dyn crate::infrastructure::observability::metrics::SystemMonitorTrait>,
}

#[async_trait::async_trait]
impl CrawlServiceTrait for CrawlService {
    async fn process_crawl_result(
        &self,
        parent_task: &Task,
        html_content: &str,
    ) -> Result<Vec<Task>, DomainError> {
        self.process_crawl_result_internal(parent_task, html_content)
            .await
    }
}

impl CrawlService {
    /// 处理爬取结果 (Internal implementation)
    pub async fn process_crawl_result_internal(
        &self,
        parent_task: &Task,
        html_content: &str,
    ) -> Result<Vec<Task>, DomainError> {
        log::debug!("Processing crawl result for task: {}", parent_task.id);

        let config = self.parse_crawl_config(parent_task)?;

        if config.depth >= config.max_depth {
            return Ok(vec![]);
        }

        let effective_max_depth = self.apply_load_degradation(config.depth, config.max_depth);

        if config.depth >= effective_max_depth {
            log::warn!(
                "Crawl depth limited by degradation strategy: depth={}, effective_max_depth={}",
                config.depth,
                effective_max_depth
            );
            return Ok(vec![]);
        }

        let links = self.discover_links(html_content, &parent_task.url, &config)?;

        // Convert anyhow::Result to Result<_, DomainError>
        match self
            .create_tasks_from_links(parent_task, &links, config, effective_max_depth)
            .await
        {
            Ok(tasks) => Ok(tasks),
            Err(e) => {
                log::error!("Crawl service error: {:?}", e);
                Err(DomainError::CrawlError(e.to_string()))
            }
        }
    }

    fn parse_crawl_config(&self, parent_task: &Task) -> anyhow::Result<CrawlConfig> {
        let depth = parent_task
            .payload
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let max_depth = parent_task
            .payload
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(3);

        let include_patterns: Vec<String> = parent_task
            .payload
            .get("include_patterns")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let exclude_patterns: Vec<String> = parent_task
            .payload
            .get("exclude_patterns")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let strategy = parent_task
            .payload
            .get("strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("bfs");

        let domain_blacklist: Vec<String> = parent_task
            .payload
            .get("domain_blacklist")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        Ok(CrawlConfig {
            depth,
            max_depth,
            include_patterns,
            exclude_patterns,
            strategy: strategy.to_string(),
            domain_blacklist,
        })
    }

    #[cfg(feature = "metrics")]
    fn apply_load_degradation(&self, depth: u64, max_depth: u64) -> u64 {
        let cpu_usage = self.system_monitor.cpu_usage();
        let mem_usage = self.system_monitor.memory_usage();

        if cpu_usage > HIGH_LOAD_THRESHOLD || mem_usage > HIGH_LOAD_THRESHOLD {
            log::warn!(
                "High load detected: cpu={}, mem={}, limiting depth",
                cpu_usage,
                mem_usage
            );
            std::cmp::min(max_depth, depth + 1)
        } else if cpu_usage > MEDIUM_LOAD_THRESHOLD || mem_usage > MEDIUM_LOAD_THRESHOLD {
            log::warn!(
                "Medium load detected: cpu={}, mem={}, limiting depth",
                cpu_usage,
                mem_usage
            );
            std::cmp::max(
                depth + 1,
                (max_depth as f64 * MEDIUM_LOAD_DEPTH_FACTOR) as u64,
            )
        } else {
            max_depth
        }
    }

    #[cfg(not(feature = "metrics"))]
    fn apply_load_degradation(&self, _depth: u64, max_depth: u64) -> u64 {
        max_depth
    }

    fn discover_links(
        &self,
        html_content: &str,
        base_url: &str,
        config: &CrawlConfig,
    ) -> anyhow::Result<HashSet<String>> {
        let links = LinkDiscoverer::extract_links(html_content, base_url)?;
        let filtered =
            LinkDiscoverer::filter_links(links, &config.include_patterns, &config.exclude_patterns);
        Ok(filtered)
    }

    async fn create_tasks_from_links(
        &self,
        parent_task: &Task,
        links: &HashSet<String>,
        config: CrawlConfig,
        effective_max_depth: u64,
    ) -> anyhow::Result<Vec<Task>> {
        let filtered_vec: Vec<String> = links.iter().cloned().collect();
        let existing_urls = self.repo.find_existing_urls(&filtered_vec).await?;

        let mut created_tasks = Vec::new();

        for link in links.clone() {
            if self.is_domain_blacklisted(&link, &config.domain_blacklist) {
                continue;
            }

            if existing_urls.contains(&link) {
                debug!("url_exists={:?}", link);
                continue;
            }
            debug!("url_new={:?}", link);

            if let Some(task) = self
                .create_single_task(parent_task, &link, &config, effective_max_depth)
                .await?
            {
                created_tasks.push(task);
            }
        }

        Ok(created_tasks)
    }

    /// Check if a domain is in the blacklist using early returns
    ///
    /// Flattens 3-level nested conditions into guard clauses.
    fn is_domain_blacklisted(&self, link: &str, domain_blacklist: &[String]) -> bool {
        // Early return if URL parsing fails
        let url = match Url::parse(link) {
            Ok(url) => url,
            Err(_) => return false,
        };

        // Early return if no domain found
        let domain = match url.domain() {
            Some(d) => d,
            None => return false,
        };

        // Check if domain is blacklisted
        if let Some(blacklisted) = domain_blacklist
            .iter()
            .find(|d| domain.contains(d.as_str()))
        {
            log::info!("Skipping blacklisted domain: {} for link: {}", domain, link);
            debug!("domain={}", domain);
            return true;
        }

        false
    }

    async fn create_single_task(
        &self,
        parent_task: &Task,
        link: &str,
        config: &CrawlConfig,
        _effective_max_depth: u64,
    ) -> anyhow::Result<Option<Task>> {
        let mut payload = parent_task.payload.clone();
        payload["depth"] = serde_json::json!(config.depth + 1);

        let priority = if config.strategy.to_lowercase() == "dfs" {
            parent_task.priority + 10
        } else {
            parent_task.priority
        };

        let user_agent = "Crawlrs/0.1.0";
        let allowed = self.robots_checker.is_allowed(link, user_agent).await?;
        debug!("link={} allowed={}", link, allowed);
        if !allowed {
            debug!("robots_blocked={:?}", link);
            return Ok(None);
        }
        debug!("robots_allowed={:?}", link);

        let robots_delay = self
            .robots_checker
            .get_crawl_delay(link, user_agent)
            .await?
            .map(|d| d.as_millis() as u64);

        let config_delay = parent_task
            .payload
            .get("config")
            .and_then(|c| c.get("crawl_delay_ms"))
            .and_then(|v| v.as_u64());

        let delay_ms = match (robots_delay, config_delay) {
            (Some(r), Some(c)) => std::cmp::max(r, c),
            (Some(r), None) => r,
            (None, Some(c)) => c,
            (None, None) => 0,
        };

        let scheduled_at = if delay_ms > 0 {
            Some(Utc::now() + chrono::Duration::milliseconds(delay_ms as i64))
        } else {
            None
        };

        let new_task = Task {
            id: Uuid::new_v4(),
            task_type: parent_task.task_type,
            status: TaskStatus::Queued,
            priority,
            team_id: parent_task.team_id,
            api_key_id: parent_task.api_key_id,
            url: link.to_string(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: parent_task.max_retries,
            scheduled_at,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            crawl_id: parent_task.crawl_id,
            updated_at: Utc::now(),
            lock_token: None,
            lock_expires_at: None,
            expires_at: parent_task.expires_at,
        };

        self.repo.create(&new_task).await?;
        Ok(Some(new_task))
    }
}

struct CrawlConfig {
    depth: u64,
    max_depth: u64,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    strategy: String,
    domain_blacklist: Vec<String>,
}

/// 链接发现器
///
/// 负责从HTML内容中提取和过滤链接
pub struct LinkDiscoverer;

impl LinkDiscoverer {
    /// 将glob模式转换为正则表达式
    fn glob_to_regex(pattern: &str) -> anyhow::Result<Regex> {
        // 简单的glob到regex转换
        let mut regex_pattern = String::new();

        for ch in pattern.chars() {
            match ch {
                '*' => regex_pattern.push_str(".*"),
                '?' => regex_pattern.push('.'),
                '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                    regex_pattern.push('\\');
                    regex_pattern.push(ch);
                }
                _ => regex_pattern.push(ch),
            }
        }

        Regex::new(&regex_pattern).map_err(|e| anyhow::anyhow!("Invalid regex pattern: {}", e))
    }

    /// 从HTML内容中提取链接
    ///
    /// # 参数
    ///
    /// * `html_content` - HTML内容
    /// * `base_url` - 基础URL
    ///
    /// # 返回值
    ///
    /// * `Ok(HashSet<String>)` - 提取到的链接集合
    /// * `Err(anyhow::Error)` - 提取过程中出现的错误
    pub fn extract_links(html_content: &str, base_url: &str) -> anyhow::Result<HashSet<String>> {
        debug!("base_url={}", base_url);
        let fragment = Html::parse_document(html_content);
        let selector =
            Selector::parse("a").map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;
        let base = Url::parse(base_url)?;
        let mut links = HashSet::new();

        for element in fragment.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                debug!("href={}", href);
                // Ignore fragment identifiers, mailto and javascript links
                if href.starts_with('#')
                    || href.starts_with("mailto:")
                    || href.starts_with("javascript:")
                {
                    continue;
                }

                match base.join(href) {
                    Ok(url) => {
                        debug!("url={}", url);
                        // Only keep http/https links
                        if url.scheme() == "http" || url.scheme() == "https" {
                            // Remove fragment to improve deduplication
                            let mut url_clean = url.clone();
                            url_clean.set_fragment(None);
                            links.insert(url_clean.to_string());
                            debug!("url={}", url_clean);
                        } else {
                            debug!("skipped_scheme={:?}", url.scheme());
                        }
                    }
                    Err(e) => {
                        debug!("error={:?}", e);
                    }
                }
            }
        }

        debug!("total={:?}", links.len());
        Ok(links)
    }

    /// 过滤链接
    ///
    /// 根据包含和排除模式过滤链接
    ///
    /// # 参数
    ///
    /// * `links` - 原始链接集合
    /// * `include_patterns` - 包含模式列表
    /// * `exclude_patterns` - 排除模式列表
    ///
    /// # 返回值
    ///
    /// 过滤后的链接集合
    pub fn filter_links(
        links: HashSet<String>,
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) -> HashSet<String> {
        debug!(
            "total_links={:?} include_patterns={:?} exclude_patterns={:?}",
            links.len(),
            include_patterns,
            exclude_patterns
        );

        // Convert glob patterns to regex patterns
        let include_regexes: Vec<Regex> = include_patterns
            .iter()
            .filter_map(|p| Self::glob_to_regex(p).ok())
            .collect();

        let exclude_regexes: Vec<Regex> = exclude_patterns
            .iter()
            .filter_map(|p| Self::glob_to_regex(p).ok())
            .collect();

        let filtered: HashSet<String> = links
            .into_iter()
            .filter(|link| {
                debug!("link={}", link);
                // If include patterns are provided, link must match at least one
                let matches_include = if include_regexes.is_empty() {
                    debug!("No include patterns, allowing all");
                    true
                } else {
                    let matched = include_regexes.iter().any(|regex| {
                        let matches = regex.is_match(link);
                        debug!(
                            "{:?} pattern={:?} matches={}",
                            link,
                            regex.as_str(),
                            matches
                        );
                        matches
                    });
                    debug!("matches_include={:?}", matched);
                    matched
                };

                // Link must NOT match any exclude pattern
                let matches_exclude = exclude_regexes.iter().any(|regex| {
                    let matches = regex.is_match(link);
                    debug!(
                        "{:?} pattern={:?} matches={}",
                        link,
                        regex.as_str(),
                        matches
                    );
                    matches
                });
                debug!("matches_exclude={:?}", matches_exclude);

                let result = matches_include && !matches_exclude;
                debug!("link={} result={}", link, result);
                result
            })
            .collect();

        debug!("filtered_count={:?}", filtered.len());
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::TaskType;
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use crate::utils::robots::RobotsCheckerTrait;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;
    use std::time::Duration;

    // ========== Mocks ==========

    /// Configurable mock for TaskRepository.
    struct MockTaskRepo {
        existing_urls: HashSet<String>,
        find_existing_fails: bool,
        create_fails: bool,
        created: Mutex<Vec<Task>>,
    }

    impl MockTaskRepo {
        fn new() -> Self {
            Self {
                existing_urls: HashSet::new(),
                find_existing_fails: false,
                create_fails: false,
                created: Mutex::new(Vec::new()),
            }
        }

        fn with_existing(urls: Vec<String>) -> Self {
            let mut repo = Self::new();
            repo.existing_urls = urls.into_iter().collect();
            repo
        }

        fn created_count(&self) -> usize {
            self.created.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            if self.create_fails {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock create failure"
                )));
            }
            self.created.lock().unwrap().push(task.clone());
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError> {
            Ok(self.existing_urls.contains(url))
        }

        async fn find_existing_urls(
            &self,
            urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            if self.find_existing_fails {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock find_existing failure"
                )));
            }
            Ok(urls
                .iter()
                .filter(|u| self.existing_urls.contains(*u))
                .cloned()
                .collect())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    /// Configurable mock for RobotsCheckerTrait.
    struct MockRobotsChecker {
        allowed: bool,
        delay: Option<Duration>,
        fails: bool,
    }

    impl MockRobotsChecker {
        fn new_allow() -> Self {
            Self {
                allowed: true,
                delay: None,
                fails: false,
            }
        }

        fn new_block() -> Self {
            Self {
                allowed: false,
                delay: None,
                fails: false,
            }
        }

        fn new_with_delay(delay_ms: u64) -> Self {
            Self {
                allowed: true,
                delay: Some(Duration::from_millis(delay_ms)),
                fails: false,
            }
        }

        fn new_failing() -> Self {
            Self {
                allowed: true,
                delay: None,
                fails: true,
            }
        }
    }

    #[async_trait]
    impl RobotsCheckerTrait for MockRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> Result<bool> {
            if self.fails {
                return Err(anyhow::anyhow!("mock robots failure"));
            }
            Ok(self.allowed)
        }

        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> Result<Option<Duration>> {
            if self.fails {
                return Err(anyhow::anyhow!("mock robots delay failure"));
            }
            Ok(self.delay)
        }
    }

    /// Mock for SystemMonitorTrait (metrics feature only).
    #[cfg(feature = "metrics")]
    struct MockSystemMonitor {
        cpu: f64,
        mem: f64,
        stale: bool,
    }

    #[cfg(feature = "metrics")]
    impl crate::infrastructure::observability::metrics::SystemMonitorTrait for MockSystemMonitor {
        fn cpu_usage(&self) -> f64 {
            self.cpu
        }

        fn memory_usage(&self) -> f64 {
            self.mem
        }

        fn is_metrics_stale(&self) -> bool {
            self.stale
        }
    }

    // ========== Helpers ==========

    fn make_service(
        repo: Arc<dyn TaskRepository>,
        robots: Arc<dyn RobotsCheckerTrait>,
    ) -> CrawlService {
        CrawlService {
            repo,
            robots_checker: robots,
            #[cfg(feature = "metrics")]
            system_monitor: Arc::new(MockSystemMonitor {
                cpu: 0.1,
                mem: 0.1,
                stale: false,
            })
                as Arc<dyn crate::infrastructure::observability::metrics::SystemMonitorTrait>,
        }
    }

    #[cfg(feature = "metrics")]
    fn make_service_with_monitor(
        repo: Arc<dyn TaskRepository>,
        robots: Arc<dyn RobotsCheckerTrait>,
        monitor: Arc<dyn crate::infrastructure::observability::metrics::SystemMonitorTrait>,
    ) -> CrawlService {
        CrawlService {
            repo,
            robots_checker: robots,
            system_monitor: monitor,
        }
    }

    /// Build a parent Task with configurable depth and max_depth in payload.
    fn make_parent_task(depth: u64, max_depth: u64) -> Task {
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Active,
            priority: 50,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com/page".to_string(),
            payload: serde_json::json!({
                "depth": depth,
                "max_depth": max_depth,
                "strategy": "bfs",
            }),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            crawl_id: Some(Uuid::new_v4()),
            updated_at: Utc::now(),
            lock_token: None,
            lock_expires_at: None,
        }
    }

    /// Build a parent Task with extra config fields (e.g. crawl_delay_ms).
    fn make_parent_task_with_config(depth: u64, max_depth: u64, config_delay_ms: u64) -> Task {
        let mut task = make_parent_task(depth, max_depth);
        task.payload["config"] = serde_json::json!({ "crawl_delay_ms": config_delay_ms });
        task
    }

    fn sample_html() -> &'static str {
        r##"<html><body>
        <a href="/page1">1</a>
        <a href="https://example.com/page2">2</a>
        <a href="https://other.com/x">3</a>
        <a href="mailto:a@b.com">mail</a>
        <a href="#frag">frag</a>
        <a href="javascript:void(0)">js</a>
        <a href="ftp://files.example.com/file">ftp</a>
        </body></html>"##
    }

    fn make_config(
        depth: u64,
        max_depth: u64,
        include: Vec<&str>,
        exclude: Vec<&str>,
        strategy: &str,
        blacklist: Vec<&str>,
    ) -> CrawlConfig {
        CrawlConfig {
            depth,
            max_depth,
            include_patterns: include.iter().map(|s| s.to_string()).collect(),
            exclude_patterns: exclude.iter().map(|s| s.to_string()).collect(),
            strategy: strategy.to_string(),
            domain_blacklist: blacklist.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ========== LinkDiscoverer::extract_links tests ==========

    #[test]
    fn test_extract_links_basic_extracts_http_links() {
        let links = LinkDiscoverer::extract_links(sample_html(), "https://example.com/page")
            .expect("extract should succeed");
        assert!(
            links.contains("https://example.com/page1"),
            "should contain resolved relative link"
        );
        assert!(
            links.contains("https://example.com/page2"),
            "should contain absolute link"
        );
        assert!(
            links.contains("https://other.com/x"),
            "should contain external link"
        );
        assert_eq!(links.len(), 3, "should extract exactly 3 http links");
    }

    #[test]
    fn test_extract_links_filters_mailto_javascript_fragment() {
        let links = LinkDiscoverer::extract_links(sample_html(), "https://example.com/page")
            .expect("extract should succeed");
        assert!(
            !links.iter().any(|l| l.contains("mailto")),
            "mailto links should be filtered"
        );
        assert!(
            !links.iter().any(|l| l.contains("javascript")),
            "javascript links should be filtered"
        );
        assert!(
            !links.iter().any(|l| l.contains("#")),
            "fragment-only links should be filtered"
        );
    }

    #[test]
    fn test_extract_links_filters_non_http_schemes() {
        let html = r#"<a href="ftp://files.example.com/file">ftp</a>"#;
        let links = LinkDiscoverer::extract_links(html, "https://example.com/").unwrap();
        assert!(links.is_empty(), "ftp links should be filtered out");
    }

    #[test]
    fn test_extract_links_empty_html() {
        let links = LinkDiscoverer::extract_links("", "https://example.com/").unwrap();
        assert!(links.is_empty(), "empty HTML should yield no links");
    }

    #[test]
    fn test_extract_links_no_anchor_tags() {
        let html = "<html><body><p>no links here</p></body></html>";
        let links = LinkDiscoverer::extract_links(html, "https://example.com/").unwrap();
        assert!(
            links.is_empty(),
            "HTML without <a> tags should yield no links"
        );
    }

    #[test]
    fn test_extract_links_deduplicates_same_url() {
        let html = r#"<a href="https://example.com/a">1</a><a href="https://example.com/a">2</a>"#;
        let links = LinkDiscoverer::extract_links(html, "https://example.com/").unwrap();
        assert_eq!(links.len(), 1, "duplicate URLs should be deduplicated");
    }

    #[test]
    fn test_extract_links_strips_fragment_from_url() {
        let html = r#"<a href="https://example.com/page#section">link</a>"#;
        let links = LinkDiscoverer::extract_links(html, "https://example.com/").unwrap();
        let url = links.iter().next().expect("should have one link");
        assert!(
            !url.contains("#section"),
            "fragment should be stripped: {}",
            url
        );
    }

    #[test]
    fn test_extract_links_invalid_base_url_returns_error() {
        let result = LinkDiscoverer::extract_links(sample_html(), "not-a-url");
        assert!(result.is_err(), "invalid base URL should return error");
    }

    #[test]
    fn test_extract_links_resolves_relative_urls() {
        let html = r#"<a href="../parent">up</a><a href="/root">root</a>"#;
        let links = LinkDiscoverer::extract_links(html, "https://example.com/sub/page").unwrap();
        assert!(
            links.contains("https://example.com/parent"),
            "should resolve ../ to parent dir"
        );
        assert!(
            links.contains("https://example.com/root"),
            "should resolve / to root"
        );
    }

    // ========== LinkDiscoverer::filter_links tests ==========

    #[test]
    fn test_filter_links_no_patterns_keeps_all() {
        let links: HashSet<String> = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ]
        .into_iter()
        .collect();
        let filtered = LinkDiscoverer::filter_links(links, &[], &[]);
        assert_eq!(filtered.len(), 2, "no patterns should keep all links");
    }

    #[test]
    fn test_filter_links_include_pattern_filters_non_matching() {
        let links: HashSet<String> = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
            "https://other.com/x".to_string(),
        ]
        .into_iter()
        .collect();
        let include = vec!["example.com/page*".to_string()];
        let filtered = LinkDiscoverer::filter_links(links, &include, &[]);
        assert_eq!(
            filtered.len(),
            2,
            "only matching include pattern should remain"
        );
        assert!(filtered.iter().all(|l| l.contains("example.com/page")));
    }

    #[test]
    fn test_filter_links_exclude_pattern_removes_matching() {
        let links: HashSet<String> = vec![
            "https://example.com/keep".to_string(),
            "https://example.com/skip".to_string(),
        ]
        .into_iter()
        .collect();
        let exclude = vec!["*skip*".to_string()];
        let filtered = LinkDiscoverer::filter_links(links, &[], &exclude);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains("https://example.com/keep"));
    }

    #[test]
    fn test_filter_links_include_and_exclude_combined() {
        let links: HashSet<String> = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
            "https://example.com/skip".to_string(),
        ]
        .into_iter()
        .collect();
        let include = vec!["*page*".to_string()];
        let exclude = vec!["*skip*".to_string()];
        let filtered = LinkDiscoverer::filter_links(links, &include, &exclude);
        assert_eq!(filtered.len(), 2, "should match include but not exclude");
    }

    #[test]
    fn test_filter_links_empty_input() {
        let filtered = LinkDiscoverer::filter_links(HashSet::new(), &[], &[]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_links_glob_question_mark_matches_single_char() {
        // `?` glob → `.` regex (unanchored). Pattern `ca?` → `ca.` matches any
        // string containing "ca" followed by exactly one char. "ca" (no trailing
        // char) does NOT match; "cat" does.
        let links: HashSet<String> = vec![
            "https://example.com/ca".to_string(),
            "https://example.com/cat".to_string(),
        ]
        .into_iter()
        .collect();
        let include = vec!["https://example.com/ca?".to_string()];
        let filtered = LinkDiscoverer::filter_links(links, &include, &[]);
        assert_eq!(filtered.len(), 1, "? should match exactly one char");
        assert!(filtered.contains("https://example.com/cat"));
    }

    // ========== LinkDiscoverer::glob_to_regex tests ==========

    #[test]
    fn test_glob_to_regex_plain_text_matches_exactly() {
        // glob_to_regex produces UNANCHORED regex (no ^/$), so plain text matches
        // any string containing the text as a substring.
        let regex = LinkDiscoverer::glob_to_regex("hello").expect("should compile");
        assert!(regex.is_match("hello"));
        assert!(
            regex.is_match("helloworld"),
            "unanchored: matches substring"
        );
        assert!(
            regex.is_match("worldhello"),
            "unanchored: matches substring"
        );
        assert!(!regex.is_match("hel"), "no match when text absent");
        assert!(!regex.is_match("world"), "no match when text absent");
    }

    #[test]
    fn test_glob_to_regex_star_matches_any() {
        // `hello*` → `hello.*` (unanchored). Matches any string containing "hello"
        // followed by zero-or-more chars.
        let regex = LinkDiscoverer::glob_to_regex("hello*").expect("should compile");
        assert!(regex.is_match("hello"));
        assert!(regex.is_match("helloworld"));
        assert!(
            regex.is_match("worldhello"),
            "unanchored: matches substring hello"
        );
        assert!(!regex.is_match("world"), "no hello prefix");
    }

    #[test]
    fn test_glob_to_regex_question_mark_matches_single_char() {
        let regex = LinkDiscoverer::glob_to_regex("h?llo").expect("should compile");
        assert!(regex.is_match("hello"));
        assert!(regex.is_match("hallo"));
        assert!(!regex.is_match("heello"));
    }

    #[test]
    fn test_glob_to_regex_escapes_special_regex_chars() {
        let regex = LinkDiscoverer::glob_to_regex("example.com").expect("should compile");
        assert!(regex.is_match("example.com"));
        assert!(
            !regex.is_match("examplexcom"),
            "dot should be literal not any-char"
        );
    }

    // ========== is_domain_blacklisted tests ==========

    #[test]
    fn test_is_domain_blacklisted_match() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        assert!(service.is_domain_blacklisted("https://spam.com/page", &["spam.com".to_string()]));
    }

    #[test]
    fn test_is_domain_blacklisted_no_match() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        assert!(!service.is_domain_blacklisted("https://good.com/page", &["spam.com".to_string()]));
    }

    #[test]
    fn test_is_domain_blacklisted_empty_list() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        assert!(!service.is_domain_blacklisted("https://example.com/", &[]));
    }

    #[test]
    fn test_is_domain_blacklisted_invalid_url() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        assert!(!service.is_domain_blacklisted("not-a-url", &["anything".to_string()]));
    }

    #[test]
    fn test_is_domain_blacklisted_substring_match() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        // "spam" is a substring of "spam.example.com"
        assert!(service.is_domain_blacklisted("https://spam.example.com/x", &["spam".to_string()]));
    }

    // ========== parse_crawl_config tests ==========

    #[test]
    fn test_parse_crawl_config_defaults() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let config = service.parse_crawl_config(&task).expect("should parse");
        assert_eq!(config.depth, 0, "default depth should be 0");
        assert_eq!(config.max_depth, 3, "default max_depth should be 3");
        assert_eq!(config.strategy, "bfs", "default strategy should be bfs");
        assert!(config.include_patterns.is_empty());
        assert!(config.exclude_patterns.is_empty());
        assert!(config.domain_blacklist.is_empty());
    }

    #[test]
    fn test_parse_crawl_config_with_custom_values() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({
                "depth": 2,
                "max_depth": 5,
                "include_patterns": ["*/blog/*"],
                "exclude_patterns": ["*/admin/*"],
                "strategy": "dfs",
                "domain_blacklist": ["spam.com"]
            }),
        );
        let config = service.parse_crawl_config(&task).expect("should parse");
        assert_eq!(config.depth, 2);
        assert_eq!(config.max_depth, 5);
        assert_eq!(config.strategy, "dfs");
        assert_eq!(config.include_patterns, vec!["*/blog/*"]);
        assert_eq!(config.exclude_patterns, vec!["*/admin/*"]);
        assert_eq!(config.domain_blacklist, vec!["spam.com"]);
    }

    #[test]
    fn test_parse_crawl_config_invalid_patterns_fallback_to_empty() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        // include_patterns is a string instead of array → serde fails → empty
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({
                "include_patterns": "not-an-array",
                "exclude_patterns": 42,
            }),
        );
        let config = service.parse_crawl_config(&task).expect("should parse");
        assert!(config.include_patterns.is_empty());
        assert!(config.exclude_patterns.is_empty());
    }

    #[test]
    fn test_parse_crawl_config_depth_as_non_u64_defaults_to_zero() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({"depth": "not-a-number"}),
        );
        let config = service.parse_crawl_config(&task).expect("should parse");
        assert_eq!(config.depth, 0, "non-u64 depth should default to 0");
    }

    // ========== apply_load_degradation tests (metrics feature) ==========

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_low_load_returns_max_depth() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.1,
                mem: 0.1,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(1, 10);
        assert_eq!(result, 10, "low load should return max_depth unchanged");
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_high_load_cpu_limits_to_depth_plus_1() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.95,
                mem: 0.1,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(2, 10);
        assert_eq!(result, 3, "high CPU should limit to depth+1");
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_high_load_mem_limits_to_depth_plus_1() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.1,
                mem: 0.95,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(2, 10);
        assert_eq!(result, 3, "high memory should limit to depth+1");
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_high_load_uses_min_when_max_depth_smaller() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.95,
                mem: 0.1,
                stale: false,
            }),
        );
        // depth=5, max_depth=3 → min(3, 6) = 3
        let result = service.apply_load_degradation(5, 3);
        assert_eq!(
            result, 3,
            "should use min(max_depth, depth+1) under high load"
        );
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_medium_load_applies_factor() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.7,
                mem: 0.1,
                stale: false,
            }),
        );
        // depth=1, max_depth=10 → max(2, (10*0.75)=7) = 7
        let result = service.apply_load_degradation(1, 10);
        assert_eq!(result, 7, "medium load should apply 0.75 factor");
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_medium_load_mem_applies_factor() {
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.1,
                mem: 0.7,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(1, 10);
        assert_eq!(result, 7, "medium mem load should apply 0.75 factor");
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_boundary_exactly_at_high_threshold_falls_to_medium() {
        // cpu exactly 0.8 → NOT high (0.8 > 0.8 is false), but IS medium (0.8 > 0.6 is true)
        // So medium-load degradation applies: max(depth+1, max_depth*0.75) = max(2, 7) = 7
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.8,
                mem: 0.1,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(1, 10);
        assert_eq!(
            result, 7,
            "exactly at high threshold falls to medium load (0.8*0.75=7.5→7)"
        );
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_apply_load_degradation_boundary_medium_exactly_at_threshold() {
        // cpu exactly 0.6 → NOT medium (> 0.6 is false)
        let service = make_service_with_monitor(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.6,
                mem: 0.1,
                stale: false,
            }),
        );
        let result = service.apply_load_degradation(1, 10);
        assert_eq!(result, 10, "exactly at medium threshold should NOT trigger");
    }

    // ========== discover_links tests ==========

    #[test]
    fn test_discover_links_success() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let links = service
            .discover_links(sample_html(), "https://example.com/page", &config)
            .expect("should discover");
        assert_eq!(links.len(), 3, "should extract 3 http links");
    }

    #[test]
    fn test_discover_links_with_include_filter() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let config = make_config(0, 3, vec!["*page1*"], vec![], "bfs", vec![]);
        let links = service
            .discover_links(sample_html(), "https://example.com/page", &config)
            .expect("should discover");
        assert_eq!(links.len(), 1, "include filter should match only page1");
    }

    #[test]
    fn test_discover_links_invalid_base_url() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let result = service.discover_links(sample_html(), "not-a-url", &config);
        assert!(result.is_err(), "invalid base URL should return error");
    }

    // ========== create_single_task tests ==========

    #[tokio::test]
    async fn test_create_single_task_success_bfs() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let result = service
            .create_single_task(&parent, "https://example.com/new", &config, 3)
            .await;
        let task = result.expect("should succeed").expect("should create task");
        assert_eq!(task.url, "https://example.com/new");
        assert_eq!(task.task_type, TaskType::Crawl);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.priority, 50, "bfs should keep parent priority");
        assert_eq!(
            task.payload["depth"].as_u64(),
            Some(1),
            "child depth should be parent depth + 1"
        );
        assert_eq!(repo.created_count(), 1);
        assert!(task.scheduled_at.is_none(), "no delay → no scheduled_at");
    }

    #[tokio::test]
    async fn test_create_single_task_dfs_increases_priority() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "dfs", vec![]);
        let task = service
            .create_single_task(&parent, "https://example.com/new", &config, 3)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.priority, 60, "dfs should add 10 to parent priority 50");
    }

    #[tokio::test]
    async fn test_create_single_task_robots_blocked_returns_none() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_block()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let result = service
            .create_single_task(&parent, "https://example.com/blocked", &config, 3)
            .await
            .expect("should not error");
        assert!(result.is_none(), "robots-blocked URL should return None");
        assert_eq!(repo.created_count(), 0, "no task should be created");
    }

    #[tokio::test]
    async fn test_create_single_task_robots_failure_returns_error() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_failing()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let result = service
            .create_single_task(&parent, "https://example.com/x", &config, 3)
            .await;
        assert!(result.is_err(), "robots failure should propagate as error");
    }

    #[tokio::test]
    async fn test_create_single_task_robots_delay_sets_scheduled_at() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(
            repo.clone(),
            Arc::new(MockRobotsChecker::new_with_delay(500)),
        );
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let task = service
            .create_single_task(&parent, "https://example.com/delayed", &config, 3)
            .await
            .unwrap()
            .unwrap();
        assert!(
            task.scheduled_at.is_some(),
            "robots delay should set scheduled_at"
        );
    }

    #[tokio::test]
    async fn test_create_single_task_takes_max_of_robots_and_config_delay() {
        let repo = Arc::new(MockTaskRepo::new());
        // robots delay = 200ms, config delay = 500ms → should use 500ms
        let service = make_service(
            repo.clone(),
            Arc::new(MockRobotsChecker::new_with_delay(200)),
        );
        let parent = make_parent_task_with_config(0, 3, 500);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let task = service
            .create_single_task(&parent, "https://example.com/x", &config, 3)
            .await
            .unwrap()
            .unwrap();
        let scheduled = task.scheduled_at.expect("should have scheduled_at");
        let now = Utc::now();
        let diff = scheduled.signed_duration_since(now);
        // Should be ~500ms (allow some tolerance)
        assert!(
            diff.num_milliseconds() >= 400 && diff.num_milliseconds() <= 600,
            "delay should be max(200, 500)=500ms, got {}ms",
            diff.num_milliseconds()
        );
    }

    #[tokio::test]
    async fn test_create_single_task_config_delay_only_when_no_robots_delay() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task_with_config(0, 3, 300);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let task = service
            .create_single_task(&parent, "https://example.com/x", &config, 3)
            .await
            .unwrap()
            .unwrap();
        assert!(
            task.scheduled_at.is_some(),
            "config delay should set scheduled_at"
        );
    }

    #[tokio::test]
    async fn test_create_single_task_create_fails_returns_error() {
        let mut repo = MockTaskRepo::new();
        repo.create_fails = true;
        let service = make_service(Arc::new(repo), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let result = service
            .create_single_task(&parent, "https://example.com/x", &config, 3)
            .await;
        assert!(result.is_err(), "repo create failure should return error");
    }

    #[tokio::test]
    async fn test_create_single_task_no_delay_no_scheduled_at() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let task = service
            .create_single_task(&parent, "https://example.com/x", &config, 3)
            .await
            .unwrap()
            .unwrap();
        assert!(
            task.scheduled_at.is_none(),
            "no robots delay and no config delay → no scheduled_at"
        );
    }

    // ========== create_tasks_from_links tests ==========

    #[tokio::test]
    async fn test_create_tasks_from_links_success() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let links: HashSet<String> = vec![
            "https://example.com/a".to_string(),
            "https://example.com/b".to_string(),
        ]
        .into_iter()
        .collect();
        let tasks = service
            .create_tasks_from_links(&parent, &links, config, 3)
            .await
            .expect("should succeed");
        assert_eq!(tasks.len(), 2, "should create 2 tasks");
        assert_eq!(repo.created_count(), 2);
    }

    #[tokio::test]
    async fn test_create_tasks_from_links_skips_blacklisted() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec!["spam.com"]);
        let links: HashSet<String> = vec![
            "https://example.com/a".to_string(),
            "https://spam.com/b".to_string(),
        ]
        .into_iter()
        .collect();
        let tasks = service
            .create_tasks_from_links(&parent, &links, config, 3)
            .await
            .expect("should succeed");
        assert_eq!(tasks.len(), 1, "blacklisted domain should be skipped");
        assert_eq!(tasks[0].url, "https://example.com/a");
    }

    #[tokio::test]
    async fn test_create_tasks_from_links_skips_existing() {
        let repo = Arc::new(MockTaskRepo::with_existing(vec![
            "https://example.com/existing".to_string(),
        ]));
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let links: HashSet<String> = vec![
            "https://example.com/existing".to_string(),
            "https://example.com/new".to_string(),
        ]
        .into_iter()
        .collect();
        let tasks = service
            .create_tasks_from_links(&parent, &links, config, 3)
            .await
            .expect("should succeed");
        assert_eq!(tasks.len(), 1, "existing URL should be skipped");
        assert_eq!(tasks[0].url, "https://example.com/new");
    }

    #[tokio::test]
    async fn test_create_tasks_from_links_find_existing_fails() {
        let mut repo = MockTaskRepo::new();
        repo.find_existing_fails = true;
        let service = make_service(Arc::new(repo), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let links: HashSet<String> = vec!["https://example.com/a".to_string()]
            .into_iter()
            .collect();
        let result = service
            .create_tasks_from_links(&parent, &links, config, 3)
            .await;
        assert!(result.is_err(), "find_existing failure should propagate");
    }

    #[tokio::test]
    async fn test_create_tasks_from_links_empty_links() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let tasks = service
            .create_tasks_from_links(&parent, &HashSet::new(), config, 3)
            .await
            .expect("should succeed");
        assert!(tasks.is_empty(), "empty links should yield empty tasks");
        assert_eq!(repo.created_count(), 0);
    }

    #[tokio::test]
    async fn test_create_tasks_from_links_robots_blocked_yields_no_task() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_block()));
        let parent = make_parent_task(0, 3);
        let config = make_config(0, 3, vec![], vec![], "bfs", vec![]);
        let links: HashSet<String> = vec!["https://example.com/a".to_string()]
            .into_iter()
            .collect();
        let tasks = service
            .create_tasks_from_links(&parent, &links, config, 3)
            .await
            .expect("should succeed");
        assert!(
            tasks.is_empty(),
            "robots-blocked URL should not create a task"
        );
        assert_eq!(repo.created_count(), 0);
    }

    // ========== process_crawl_result_internal tests ==========

    #[tokio::test]
    async fn test_process_crawl_result_depth_at_max_returns_empty() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        // depth=3, max_depth=3 → depth >= max_depth → empty
        let parent = make_parent_task(3, 3);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await
            .expect("should succeed");
        assert!(result.is_empty(), "depth >= max_depth should return empty");
        assert_eq!(repo.created_count(), 0);
    }

    #[tokio::test]
    async fn test_process_crawl_result_depth_exceeds_max_returns_empty() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(5, 3);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await
            .expect("should succeed");
        assert!(result.is_empty(), "depth > max_depth should return empty");
    }

    #[tokio::test]
    async fn test_process_crawl_result_success_creates_tasks() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await
            .expect("should succeed");
        assert_eq!(
            result.len(),
            3,
            "should create tasks for all 3 extracted links"
        );
        assert_eq!(repo.created_count(), 3);
    }

    #[tokio::test]
    async fn test_process_crawl_result_no_links_returns_empty() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let result = service
            .process_crawl_result_internal(&parent, "<html><body>no links</body></html>")
            .await
            .expect("should succeed");
        assert!(result.is_empty(), "no links in HTML → no tasks");
    }

    #[tokio::test]
    async fn test_process_crawl_result_create_fails_returns_crawl_error() {
        let mut repo = MockTaskRepo::new();
        repo.create_fails = true;
        let service = make_service(Arc::new(repo), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await;
        assert!(result.is_err(), "repo create failure should return error");
        match result.unwrap_err() {
            DomainError::CrawlError(msg) => {
                assert!(
                    msg.contains("mock create failure"),
                    "error should contain original message: {}",
                    msg
                );
            }
            other => panic!("expected CrawlError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_process_crawl_result_invalid_base_url_returns_error() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let mut parent = make_parent_task(0, 3);
        parent.url = "not-a-url".to_string();
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await;
        assert!(result.is_err(), "invalid base URL should return error");
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_process_crawl_result_high_load_still_crawls_when_depth_below_effective() {
        // NOTE: Source code finding — the degradation check `if config.depth >= effective_max_depth`
        // appears to be unreachable: apply_load_degradation always returns a value >= depth+1
        // (high load: min(max_depth, depth+1); medium load: max(depth+1, factor)). Since depth
        // is an integer, depth >= depth+1 is always false, so the second early-return never fires
        // unless max_depth <= depth (which is caught by the first check). This is a potential
        // logic bug in the source — reported but not fixed per task constraints.
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service_with_monitor(
            repo.clone(),
            Arc::new(MockRobotsChecker::new_allow()),
            Arc::new(MockSystemMonitor {
                cpu: 0.95,
                mem: 0.1,
                stale: false,
            }),
        );
        // depth=2, max_depth=10, high load → effective = min(10, 3) = 3, 2 < 3 → tasks created
        let parent = make_parent_task(2, 10);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await
            .expect("should succeed");
        assert_eq!(
            result.len(),
            3,
            "high load reduces max_depth to depth+1 but crawling continues when depth < effective"
        );
    }

    #[tokio::test]
    async fn test_process_crawl_result_trait_forwards_to_internal() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(0, 3);
        let result = service
            .process_crawl_result(&parent, sample_html())
            .await
            .expect("trait should forward to internal");
        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn test_process_crawl_result_trait_depth_at_max() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let parent = make_parent_task(3, 3);
        let result = service
            .process_crawl_result(&parent, sample_html())
            .await
            .expect("should succeed");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_process_crawl_result_with_blacklist_skips_domain() {
        let repo = Arc::new(MockTaskRepo::new());
        let service = make_service(repo.clone(), Arc::new(MockRobotsChecker::new_allow()));
        let mut parent = make_parent_task(0, 3);
        parent.payload["domain_blacklist"] = serde_json::json!(["other.com"]);
        let result = service
            .process_crawl_result_internal(&parent, sample_html())
            .await
            .expect("should succeed");
        // sample_html has 3 links: example.com/page1, example.com/page2, other.com/x
        // other.com is blacklisted → only 2 tasks created
        assert_eq!(result.len(), 2, "blacklisted domain should be skipped");
        assert!(
            !result.iter().any(|t| t.url.contains("other.com")),
            "no task should have other.com URL"
        );
    }

    #[test]
    fn test_is_domain_blacklisted_url_without_domain_returns_false() {
        let service = make_service(
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRobotsChecker::new_allow()),
        );
        // file:// URLs have no domain component
        assert!(!service.is_domain_blacklisted("file:///path/to/file", &["anything".to_string()]));
        // data: URLs have no domain
        assert!(!service.is_domain_blacklisted("data:text/plain,hello", &["anything".to_string()]));
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_mock_task_repo_remaining_methods_return_defaults() {
        use crate::domain::repositories::task_repository::TaskQueryParams;

        let repo = MockTaskRepo::new();
        let task_id = Uuid::new_v4();
        assert!(repo.find_by_id(task_id).await.unwrap().is_none());
        let task = Task::new(
            task_id,
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let updated = repo.update(&task).await.unwrap();
        assert_eq!(updated.id, task_id);
        assert!(repo.acquire_next(Uuid::new_v4()).await.unwrap().is_none());
        repo.mark_completed(task_id).await.unwrap();
        repo.mark_failed(task_id).await.unwrap();
        repo.mark_cancelled(task_id).await.unwrap();
        assert!(!repo.exists_by_url("https://a.com").await.unwrap());
        assert!(repo
            .find_existing_urls(&["https://a.com".to_string()])
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            repo.reset_stuck_tasks(chrono::Duration::minutes(5))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            repo.cancel_tasks_by_crawl_id(Uuid::new_v4()).await.unwrap(),
            0
        );
        assert_eq!(repo.expire_tasks().await.unwrap(), 0);
        assert!(repo
            .find_by_crawl_id(Uuid::new_v4())
            .await
            .unwrap()
            .is_empty());
        let (tasks, count) = repo.query_tasks(TaskQueryParams::default()).await.unwrap();
        assert!(tasks.is_empty());
        assert_eq!(count, 0);
        let (cancelled, failed) = repo
            .batch_cancel(vec![Uuid::new_v4()], Uuid::new_v4(), false)
            .await
            .unwrap();
        assert!(cancelled.is_empty());
        assert!(failed.is_empty());
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_mock_system_monitor_is_metrics_stale() {
        use crate::infrastructure::observability::metrics::SystemMonitorTrait;

        let monitor = MockSystemMonitor {
            cpu: 0.1,
            mem: 0.1,
            stale: true,
        };
        assert!(monitor.is_metrics_stale());
        let monitor2 = MockSystemMonitor {
            cpu: 0.1,
            mem: 0.1,
            stale: false,
        };
        assert!(!monitor2.is_metrics_stale());
    }
}
