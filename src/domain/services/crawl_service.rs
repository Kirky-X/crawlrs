// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::models::task::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::utils::robots::{RobotsChecker, RobotsCheckerTrait};
use anyhow::Result;
use chrono::Utc;
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
            robots_checker: RobotsChecker::new(),
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

        // Extract links
        let links = LinkDiscoverer::extract_links(html_content, &parent_task.url)?;

        // Filter links
        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        let mut created_tasks = Vec::new();

        for link in filtered {
            // Robots.txt check
            if !self
                .robots_checker
                .is_allowed(&link, "Crawlrs/0.1.0")
                .await?
            {
                continue;
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
            let priority = if strategy.to_lowercase() == "dfs" {
                parent_task.priority + 10
            } else {
                parent_task.priority
            };

            let delay_ms = parent_task
                .payload
                .get("config")
                .and_then(|c| c.get("crawl_delay_ms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

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
                expires_at: None,
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
        let fragment = Html::parse_document(html_content);
        let selector =
            Selector::parse("a").map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;
        let base = Url::parse(base_url)?;
        let mut links = HashSet::new();

        for element in fragment.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                // Ignore fragment identifiers, mailto and javascript links
                if href.starts_with('#')
                    || href.starts_with("mailto:")
                    || href.starts_with("javascript:")
                {
                    continue;
                }

                if let Ok(url) = base.join(href) {
                    // Only keep http/https links
                    if url.scheme() == "http" || url.scheme() == "https" {
                        // Remove fragment to improve deduplication
                        let mut url_clean = url.clone();
                        url_clean.set_fragment(None);
                        links.insert(url_clean.to_string());
                    }
                }
            }
        }

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
        links
            .into_iter()
            .filter(|link| {
                // If include patterns are provided, link must match at least one
                let matches_include = if include_patterns.is_empty() {
                    true
                } else {
                    include_patterns.iter().any(|p| link.contains(p))
                };

                // Link must NOT match any exclude pattern
                let matches_exclude = exclude_patterns.iter().any(|p| link.contains(p));

                matches_include && !matches_exclude
            })
            .collect()
    }
}
