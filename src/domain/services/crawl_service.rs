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

use crate::domain::models::task::{DomainError, Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
#[cfg(feature = "metrics")]
use crate::infrastructure::observability::metrics::{get_cpu_usage, get_memory_usage};
use crate::utils::robots::{RobotsChecker, RobotsCheckerTrait};
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;
use url::Url;
use uuid::Uuid;

/// 爬取服务
///
/// 处理网站爬取任务的核心业务逻辑
pub struct CrawlService<R: TaskRepository, C: RobotsCheckerTrait = RobotsChecker> {
    /// 任务仓库
    repo: Arc<R>,
    /// Robots.txt检查器
    robots_checker: C,
}

impl<R: TaskRepository> CrawlService<R, RobotsChecker> {
    /// 创建新的爬取服务实例
    ///
    /// # 参数
    ///
    /// * `repo` - 任务仓库实例
    ///
    /// # 返回值
    ///
    /// 返回新的爬取服务实例
    pub fn new(repo: Arc<R>) -> Self {
        Self {
            repo,
            robots_checker: RobotsChecker::new(None),
        }
    }
}

impl<R: TaskRepository, C: RobotsCheckerTrait> CrawlService<R, C> {
    /// 使用自定义Robots检查器创建新的爬取服务实例
    pub fn new_with_checker(repo: Arc<R>, checker: C) -> Self {
        Self {
            repo,
            robots_checker: checker,
        }
    }

    /// 处理爬取结果
    ///
    /// 解析HTML内容，提取链接并创建新的爬取任务
    ///
    /// # 参数
    ///
    /// * `parent_task` - 父任务
    /// * `html_content` - HTML内容
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<Task>)` - 新创建的任务列表
    /// * `Err(DomainError)` - 处理过程中出现的错误
    pub async fn process_crawl_result(
        &self,
        parent_task: &Task,
        html_content: &str,
    ) -> Result<Vec<Task>, DomainError> {
        tracing::debug!("Processing crawl result for task: {}", parent_task.id);

        let config = self.parse_crawl_config(parent_task)?;

        if config.depth >= config.max_depth {
            return Ok(vec![]);
        }

        let effective_max_depth = self.apply_load_degradation(config.depth, config.max_depth);

        if config.depth >= effective_max_depth {
            tracing::warn!(
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
                tracing::error!("Crawl service error: {:?}", e);
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
        let cpu_usage = get_cpu_usage();
        let mem_usage = get_memory_usage();

        if cpu_usage > HIGH_LOAD_THRESHOLD || mem_usage > HIGH_LOAD_THRESHOLD {
            tracing::warn!(
                "High load detected: cpu={}, mem={}, limiting depth",
                cpu_usage,
                mem_usage
            );
            std::cmp::min(max_depth, depth + 1)
        } else if cpu_usage > MEDIUM_LOAD_THRESHOLD || mem_usage > MEDIUM_LOAD_THRESHOLD {
            tracing::warn!(
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
                debug!(url_exists = link);
                continue;
            }
            debug!(url_new = link);

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
            tracing::info!("Skipping blacklisted domain: {} for link: {}", domain, link);
            debug!(domain);
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
        debug!(link, allowed);
        if !allowed {
            debug!(robots_blocked = link);
            return Ok(None);
        }
        debug!(robots_allowed = link);

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
            Some((Utc::now() + chrono::Duration::milliseconds(delay_ms as i64)).into())
        } else {
            None
        };

        let new_task = Task {
            id: Uuid::new_v4(),
            task_type: parent_task.task_type,
            status: TaskStatus::Queued,
            priority,
            team_id: parent_task.team_id,
            url: link.to_string(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: parent_task.max_retries,
            scheduled_at,
            created_at: Utc::now().into(),
            started_at: None,
            completed_at: None,
            crawl_id: parent_task.crawl_id,
            updated_at: Utc::now().into(),
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
        debug!(base_url);
        let fragment = Html::parse_document(html_content);
        let selector =
            Selector::parse("a").map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;
        let base = Url::parse(base_url)?;
        let mut links = HashSet::new();

        for element in fragment.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                debug!(href);
                // Ignore fragment identifiers, mailto and javascript links
                if href.starts_with('#')
                    || href.starts_with("mailto:")
                    || href.starts_with("javascript:")
                {
                    continue;
                }

                match base.join(href) {
                    Ok(url) => {
                        debug!(url = %url);
                        // Only keep http/https links
                        if url.scheme() == "http" || url.scheme() == "https" {
                            // Remove fragment to improve deduplication
                            let mut url_clean = url.clone();
                            url_clean.set_fragment(None);
                            links.insert(url_clean.to_string());
                            debug!(url = %url_clean);
                        } else {
                            debug!(skipped_scheme = url.scheme());
                        }
                    }
                    Err(e) => {
                        debug!(error = ?e);
                    }
                }
            }
        }

        debug!(total = links.len());
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
            total_links = links.len(),
            ?include_patterns,
            ?exclude_patterns
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
                debug!(link);
                // If include patterns are provided, link must match at least one
                let matches_include = if include_regexes.is_empty() {
                    debug!("No include patterns, allowing all");
                    true
                } else {
                    let matched = include_regexes.iter().any(|regex| {
                        let matches = regex.is_match(link);
                        debug!(link, pattern = regex.as_str(), matches);
                        matches
                    });
                    debug!(matches_include = matched);
                    matched
                };

                // Link must NOT match any exclude pattern
                let matches_exclude = exclude_regexes.iter().any(|regex| {
                    let matches = regex.is_match(link);
                    debug!(link, pattern = regex.as_str(), matches);
                    matches
                });
                debug!(matches_exclude = matches_exclude);

                let result = matches_include && !matches_exclude;
                debug!(link, result);
                result
            })
            .collect();

        debug!(filtered_count = filtered.len());
        filtered
    }
}
