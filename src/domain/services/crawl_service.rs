// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::infrastructure::observability::metrics::{get_cpu_usage, get_memory_usage};
use crate::utils::robots::{RobotsChecker, RobotsCheckerTrait};
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::sync::Arc;
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
    /// * `Err(anyhow::Error)` - 处理过程中出现的错误
    pub async fn process_crawl_result(
        &self,
        parent_task: &Task,
        html_content: &str,
    ) -> Result<Vec<Task>> {
        tracing::debug!("Processing crawl result for task: {}", parent_task.id);
        // Parse config from payload
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

        // Check depth limit
        if depth >= max_depth {
            return Ok(vec![]);
        }

        // Degradation strategy: reduce max_depth under high load
        let cpu_usage = get_cpu_usage();
        let mem_usage = get_memory_usage();
        let effective_max_depth = if cpu_usage > 0.8 || mem_usage > 0.8 {
            // High load: limit depth to current + 1 or config/2
            std::cmp::min(max_depth, depth + 1)
        } else if cpu_usage > 0.6 || mem_usage > 0.6 {
            // Medium load: limit depth to 75% of max_depth
            std::cmp::max(depth + 1, (max_depth as f64 * 0.75) as u64)
        } else {
            max_depth
        };

        if depth >= effective_max_depth {
            tracing::warn!(
                "Crawl depth limited by degradation strategy: depth={}, effective_max_depth={}, cpu={}, mem={}",
                depth, effective_max_depth, cpu_usage, mem_usage
            );
            return Ok(vec![]);
        }

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

        // Extract links
        let links = LinkDiscoverer::extract_links(html_content, &parent_task.url)?;

        // Filter links
        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        let mut created_tasks = Vec::new();

        for link in filtered {
            // Domain blacklist check
            if let Ok(url) = Url::parse(&link) {
                if let Some(domain) = url.domain() {
                    if domain_blacklist.iter().any(|d| domain.contains(d)) {
                        tracing::info!(
                            "Skipping blacklisted domain: {} for link: {}",
                            domain,
                            link
                        );
                        continue;
                    }
                }
            }

            // Deduplication check
            if self.repo.exists_by_url(&link).await? {
                continue;
            }

            // Create new task payload
            let mut payload = parent_task.payload.clone();
            payload["depth"] = serde_json::json!(depth + 1);

            // Adjust priority based on strategy
            // BFS: Same priority (FIFO) or lower
            // DFS: Higher priority (LIFO)
            let mut priority = if strategy.to_lowercase() == "dfs" {
                parent_task.priority + 10
            } else {
                parent_task.priority
            };

            // Degradation strategy: reduce priority for non-critical tasks under high load
            if cpu_usage > 0.7 || mem_usage > 0.7 {
                priority = priority.saturating_sub(5);
            }

            // Get robots.txt crawl delay if available
            // 注意：这里将 robots 检查推迟到任务处理时，或者在此处并行检查
            // 为了优化 UAT-012 的处理时间，我们可以在这里跳过 robots 检查，
            // 让 Worker 在实际执行时进行检查。或者我们只获取 delay 但不进行完整的 allowed 检查。
            // 但根据目前的代码结构，Robots 检查是在这里进行的。
            // 优化：只有当 URL 不在数据库中时才检查 robots.txt

            let user_agent = "Crawlrs/0.1.0";
            // Robots.txt check - Move AFTER deduplication check to save network requests
            if !self.robots_checker.is_allowed(&link, user_agent).await? {
                continue;
            }

            let robots_delay = self
                .robots_checker
                .get_crawl_delay(&link, user_agent)
                .await?
                .map(|d| d.as_millis() as u64);

            let config_delay = parent_task
                .payload
                .get("config")
                .and_then(|c| c.get("crawl_delay_ms"))
                .and_then(|v| v.as_u64());

            // Prefer robots.txt delay if it's larger, or use config delay
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
                task_type: parent_task.task_type, // Propagate task type
                status: TaskStatus::Queued,
                priority,
                team_id: parent_task.team_id,
                url: link,
                payload,
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

            // Save to repository
            self.repo.create(&new_task).await?;
            created_tasks.push(new_task);
        }

        Ok(created_tasks)
    }
}

/// 链接发现器
///
/// 负责从HTML内容中提取和过滤链接
pub struct LinkDiscoverer;

impl LinkDiscoverer {
    /// 将glob模式转换为正则表达式
    fn glob_to_regex(pattern: &str) -> Result<Regex> {
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
    pub fn extract_links(html_content: &str, base_url: &str) -> Result<HashSet<String>> {
        println!("DEBUG: extract_links called with base_url: {}", base_url);
        let fragment = Html::parse_document(html_content);
        let selector =
            Selector::parse("a").map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;
        let base = Url::parse(base_url)?;
        let mut links = HashSet::new();

        for element in fragment.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                println!("DEBUG: Found href: {}", href);
                // Ignore fragment identifiers, mailto and javascript links
                if href.starts_with('#')
                    || href.starts_with("mailto:")
                    || href.starts_with("javascript:")
                {
                    continue;
                }

                match base.join(href) {
                    Ok(url) => {
                        println!("DEBUG: Successfully joined URL: {}", url);
                        // Only keep http/https links
                        if url.scheme() == "http" || url.scheme() == "https" {
                            // Remove fragment to improve deduplication
                            let mut url_clean = url.clone();
                            url_clean.set_fragment(None);
                            links.insert(url_clean.to_string());
                            println!("DEBUG: Added URL to links: {}", url_clean);
                        } else {
                            println!("DEBUG: Skipped URL due to scheme: {}", url.scheme());
                        }
                    }
                    Err(e) => {
                        println!("DEBUG: Failed to join URL: {:?}", e);
                    }
                }
            }
        }

        println!("DEBUG: Total links extracted: {}", links.len());
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
        println!("DEBUG: filter_links called with {} links", links.len());
        println!("DEBUG: include_patterns: {:?}", include_patterns);
        println!("DEBUG: exclude_patterns: {:?}", exclude_patterns);

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
                println!("DEBUG: Processing link: {}", link);
                // If include patterns are provided, link must match at least one
                let matches_include = if include_regexes.is_empty() {
                    println!("DEBUG: No include patterns, allowing all");
                    true
                } else {
                    let matched = include_regexes.iter().any(|regex| {
                        let matches = regex.is_match(link);
                        println!(
                            "DEBUG: Checking if '{}' matches include pattern '{}': {}",
                            link,
                            regex.as_str(),
                            matches
                        );
                        matches
                    });
                    println!("DEBUG: matches_include: {}", matched);
                    matched
                };

                // Link must NOT match any exclude pattern
                let matches_exclude = exclude_regexes.iter().any(|regex| {
                    let matches = regex.is_match(link);
                    println!(
                        "DEBUG: Checking if '{}' matches exclude pattern '{}': {}",
                        link,
                        regex.as_str(),
                        matches
                    );
                    matches
                });
                println!("DEBUG: matches_exclude: {}", matches_exclude);

                let result = matches_include && !matches_exclude;
                println!("DEBUG: Final result for '{}': {}", link, result);
                result
            })
            .collect();

        println!("DEBUG: Filtered links count: {}", filtered.len());
        filtered
    }
}
