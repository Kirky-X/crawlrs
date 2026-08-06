// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl Link Extractor
//!
//! 从 `scrape_worker.rs` 提取的爬取链接处理函数。
//! 包含 robots.txt 检查、完成状态更新和链接提取入队。

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use log::{error, info, warn};
use scraper::{Html, Selector};
use serde_json::json;
use url::Url;
use uuid::Uuid;

use chrono::Utc;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::domain::models::{CrawlStatus, Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::engines::engine_client::ScrapeResponse;
use crate::utils::dedup::{DedupResult, Deduplicator};
use crate::utils::robots::RobotsCheckerTrait;
use crate::workers::crawl::{
    FilterContext, Frontier, PathDepthScorer, ScoringContext, UrlFilter, UrlPatternFilter,
    UrlScorer,
};
use crate::workers::errors::ScrapeWorkerError;

/// 检查 Robots.txt 并返回是否允许访问
///
/// # Arguments
/// * `task` - 目标任务
/// * `robots_checker` - robots.txt 检查器
pub async fn check_robots_txt(task: &Task, robots_checker: &dyn RobotsCheckerTrait) -> bool {
    let user_agent = "crawlrs-bot";

    if !robots_checker
        .is_allowed(&task.url, user_agent)
        .await
        .unwrap_or(true)
    {
        info!("Access denied by robots.txt for {}", task.url);
        return false;
    }

    if let Some(delay) = robots_checker
        .get_crawl_delay(&task.url, user_agent)
        .await
        .unwrap_or(None)
    {
        info!("Respecting crawl delay of {:?} for {}", delay, task.url);
        tokio::time::sleep(delay).await;
    }

    true
}

/// 更新 Crawl 完成状态（检查是否所有任务都已完成）
///
/// 当 completed + failed == total 时，将 crawl 状态标记为 Completed。
///
/// # Arguments
/// * `crawl_id` - 爬取任务 ID
/// * `crawl_repository` - 爬取仓库
pub async fn update_crawl_completion_status(
    crawl_id: Uuid,
    crawl_repository: &dyn CrawlRepository,
) {
    match crawl_repository.find_by_id(crawl_id).await {
        Ok(Some(c)) => {
            if c.completed_tasks() + c.failed_tasks() == c.total_tasks() {
                info!(
                    "All tasks completed for crawl {}, marking as completed",
                    crawl_id
                );
                if let Err(e) = crawl_repository
                    .update_status(crawl_id, CrawlStatus::Completed)
                    .await
                {
                    error!(
                        "Failed to update crawl status to completed for crawl {}: {}",
                        crawl_id, e
                    );
                }
            }
        }
        Ok(None) => {
            error!("Crawl not found for id {}", crawl_id);
        }
        Err(e) => {
            error!("Failed to fetch crawl {}: {}", crawl_id, e);
        }
    }
}

/// 从抓取响应中提取链接并加入爬取队列
///
/// 三层去重：Bloom 预筛 → DB 校验 → 二次 Bloom 防 race。
/// 最终经 Frontier 评分排序后入队。
///
/// # Arguments
/// * `task` - 当前任务
/// * `response` - 抓取响应
/// * `crawl_id` - 爬取 ID
/// * `current_depth` - 当前深度
/// * `config` - 爬取配置
/// * `repository` - 任务仓库
/// * `crawl_repository` - 爬取仓库
/// * `deduplicator` - URL 去重器
#[allow(clippy::too_many_arguments)]
pub async fn extract_and_queue_links(
    task: &Task,
    response: &ScrapeResponse,
    crawl_id: Uuid,
    current_depth: u32,
    config: &CrawlConfigDto,
    repository: &dyn TaskRepository,
    crawl_repository: &dyn CrawlRepository,
    deduplicator: &Arc<parking_lot::RwLock<Deduplicator>>,
) -> Result<()> {
    // 只解析 HTML 内容
    if !response.content_type.contains("text/html") {
        return Ok(());
    }

    // 性能审查 H-3 修复：循环外构造一次 UrlPatternFilter
    let empty_include_short_circuit =
        matches!(&config.include_patterns, Some(patterns) if patterns.is_empty());
    let pattern_filter = if empty_include_short_circuit {
        None
    } else {
        Some(UrlPatternFilter::new(
            config.include_patterns.clone().unwrap_or_default(),
            config.exclude_patterns.clone().unwrap_or_default(),
        ))
    };
    let filter_ctx = FilterContext::default();

    let unique_links = {
        let document = Html::parse_document(&response.content);
        let selector =
            Selector::parse("a").map_err(|e| ScrapeWorkerError::SelectorError(e.to_string()))?;
        let base_url = Url::parse(&task.url)?;

        let mut links = HashSet::new();

        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                if let Ok(absolute_url) = base_url.join(href) {
                    let url_str = absolute_url.to_string();

                    if !url_str.starts_with("http") {
                        continue;
                    }
                    if url_str == task.url {
                        continue;
                    }

                    if let Some(filter) = &pattern_filter {
                        if !filter.accept(&url_str, &filter_ctx) {
                            continue;
                        }
                    } else if empty_include_short_circuit {
                        continue;
                    }

                    links.insert(url_str);
                }
            }
        }
        links
    };

    info!("Found {} unique links on {}", unique_links.len(), task.url);

    // T053/R-frontier-001：URL 分层去重
    let mut to_enqueue: Vec<String> = Vec::with_capacity(unique_links.len());
    let mut db_check: Vec<String> = Vec::with_capacity(unique_links.len());

    {
        let mut dedup = deduplicator.write();
        for link in &unique_links {
            match dedup.check_and_insert(link) {
                Ok(DedupResult::DefinitelyNew { normalized }) => {
                    to_enqueue.push(normalized);
                }
                Ok(DedupResult::MaybeExisting { normalized, .. }) => {
                    db_check.push(normalized);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "URL dedup check failed for {}: {}",
                        link,
                        e
                    ));
                }
            }
        }
    }

    // T066/R-frontier-003：Frontier 评分排序
    let mut new_urls: Vec<String> = Vec::with_capacity(unique_links.len());
    new_urls.extend(std::mem::take(&mut to_enqueue));

    if !db_check.is_empty() {
        db_check.sort_unstable();
        db_check.dedup();

        let existing_urls = repository.find_existing_urls(&db_check).await?;
        let existing_url_set: HashSet<String> = existing_urls.into_iter().collect();

        let mut to_db_insert: Vec<&String> = Vec::with_capacity(db_check.len());
        for normalized in &db_check {
            if !existing_url_set.contains(normalized) {
                to_db_insert.push(normalized);
            }
        }

        let mut to_db_enqueue: Vec<String> = Vec::with_capacity(to_db_insert.len());
        if !to_db_insert.is_empty() {
            let mut dedup = deduplicator.write();
            for normalized in &to_db_insert {
                match dedup.check_and_insert(normalized) {
                    Ok(DedupResult::DefinitelyNew { normalized: n }) => {
                        to_db_enqueue.push(n);
                    }
                    Ok(DedupResult::MaybeExisting { .. }) => {
                        continue;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "URL dedup check_and_insert failed for {}: {}",
                            normalized,
                            e
                        ));
                    }
                }
            }
        }

        new_urls.extend(std::mem::take(&mut to_db_enqueue));
    }

    // Frontier 评分排序入队
    if !new_urls.is_empty() {
        let scorer = PathDepthScorer::new();
        let scoring_ctx = ScoringContext::default();
        let frontier = Frontier::new();

        for url in &new_urls {
            let base_score = scorer.score(url, &scoring_ctx);
            // T082 / R-frontier-006：KG 结构空洞提升因子
            // kg_priority_boost > 1.0 表示 URL 可能填补结构空洞，应获得更高优先级
            let score =
                (base_score as f64 * scoring_ctx.kg_priority_boost).clamp(0.0, 1.0) as f32;
            match crate::workers::crawl::ScoredUrl::new(url.clone(), score) {
                Ok(scored) => frontier.push(scored),
                Err(e) => {
                    warn!("task_id: {}, URL 评分失败跳过: {} ({})", task.id, url, e);
                }
            }
        }

        info!(
            "task_id: {}, {} URLs 入 Frontier（{} 域名），按评分出队",
            task.id,
            frontier.len(),
            frontier.domain_count()
        );

        while let Some(scored) = frontier.pop() {
            let mut priority = task.priority;
            if let Some(strategy) = &config.strategy {
                if strategy.to_lowercase() == "dfs" {
                    priority = priority.saturating_add(1);
                }
            }

            let new_task = Task {
                id: Uuid::new_v4(),
                task_type: TaskType::Crawl,
                status: TaskStatus::Queued,
                priority,
                team_id: task.team_id,
                api_key_id: task.api_key_id,
                url: scored.url,
                payload: json!({
                    "crawl_id": crawl_id.to_string(),
                    "depth": current_depth + 1,
                    "config": config
                }),
                retry_count: 0,
                attempt_count: 0,
                max_retries: 3,
                scheduled_at: None,
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
                crawl_id: Some(crawl_id),
                updated_at: Utc::now(),
                lock_token: None,
                lock_expires_at: None,
                expires_at: None,
            };

            repository.create(&new_task).await?;
            crawl_repository.increment_total_tasks(crawl_id).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Crawl;
    use crate::domain::repositories::task_repository::RepositoryError;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn make_task(url: &str) -> Task {
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: url.to_string(),
            payload: json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: Utc::now(),
            lock_token: None,
            lock_expires_at: None,
            expires_at: None,
        }
    }

    // ---- Mock types ----

    struct MockRobotsChecker {
        allowed: bool,
    }

    #[async_trait::async_trait]
    impl RobotsCheckerTrait for MockRobotsChecker {
        async fn is_allowed(&self, _url: &str, _user_agent: &str) -> Result<bool> {
            Ok(self.allowed)
        }
        async fn get_crawl_delay(
            &self,
            _url: &str,
            _user_agent: &str,
        ) -> Result<Option<std::time::Duration>> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct MockCrawlRepo {
        update_status_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl CrawlRepository for MockCrawlRepo {
        async fn create(&self, _crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            unimplemented!()
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, _crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            unimplemented!()
        }
        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: CrawlStatus,
        ) -> Result<(), RepositoryError> {
            self.update_status_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    // ---- check_robots_txt ----

    #[tokio::test]
    async fn test_check_robots_txt_allowed_returns_true() {
        let task = make_task("https://example.com");
        let checker = MockRobotsChecker { allowed: true };
        assert!(check_robots_txt(&task, &checker).await);
    }

    #[tokio::test]
    async fn test_check_robots_txt_denied_returns_false() {
        let task = make_task("https://example.com");
        let checker = MockRobotsChecker { allowed: false };
        assert!(!check_robots_txt(&task, &checker).await);
    }

    // ---- update_crawl_completion_status ----

    #[tokio::test]
    async fn test_update_crawl_completion_status_crawl_not_found() {
        // MockCrawlRepo.find_by_id returns Ok(None) → logs error, no panic
        let repo = MockCrawlRepo::default();
        update_crawl_completion_status(Uuid::new_v4(), &repo).await;
        // No assertion needed — just verify it doesn't panic
    }
}
