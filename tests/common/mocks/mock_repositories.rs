// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 统一 mock 仓库实现 — 消除 25+ 重复定义
//!
//! 所有 mock 默认返回 noop/空值。需要特定行为的测试可自行扩展或配置。

#![allow(dead_code)]

use async_trait::async_trait;
use std::collections::HashSet;

use crawlrs::domain::models::{Crawl, CrawlStatus, ScrapeResult, Task};
use crawlrs::domain::models::{CreditsTransaction, CreditsTransactionType};
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::domain::repositories::credits_repository::{
    CreditsRepository, CreditsRepositoryError,
};
use crawlrs::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crawlrs::domain::repositories::task_repository::{
    RepositoryError, TaskQueryParams, TaskRepository,
};

// ============================================================================
// MockTaskRepository
// ============================================================================

/// Noop `TaskRepository` — all methods return Ok with default values.
pub struct MockTaskRepository;

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
        Ok(task.clone())
    }
    async fn find_by_id(
        &self,
        _id: crawlrs::domain::id::Id,
    ) -> Result<Option<Task>, RepositoryError> {
        Ok(None)
    }
    async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
        Ok(task.clone())
    }
    async fn acquire_next(
        &self,
        _worker_id: crawlrs::domain::id::Id,
    ) -> Result<Option<Task>, RepositoryError> {
        Ok(None)
    }
    async fn mark_completed(
        &self,
        _id: crawlrs::domain::id::Id,
        _lock_token: Option<crawlrs::domain::id::Id>,
    ) -> Result<u64, RepositoryError> {
        Ok(1)
    }
    async fn mark_failed(
        &self,
        _id: crawlrs::domain::id::Id,
        _lock_token: Option<crawlrs::domain::id::Id>,
    ) -> Result<u64, RepositoryError> {
        Ok(1)
    }
    async fn mark_cancelled(&self, _id: crawlrs::domain::id::Id) -> Result<u64, RepositoryError> {
        Ok(1)
    }
    async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
        Ok(false)
    }
    async fn find_existing_urls(
        &self,
        _urls: &[String],
    ) -> Result<HashSet<String>, RepositoryError> {
        Ok(HashSet::new())
    }
    async fn reset_stuck_tasks(&self, _timeout: chrono::Duration) -> Result<u64, RepositoryError> {
        Ok(0)
    }
    async fn cancel_tasks_by_crawl_id(
        &self,
        _crawl_id: crawlrs::domain::id::Id,
    ) -> Result<u64, RepositoryError> {
        Ok(0)
    }
    async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
        Ok(0)
    }
    async fn find_by_crawl_id(
        &self,
        _crawl_id: crawlrs::domain::id::Id,
    ) -> Result<Vec<Task>, RepositoryError> {
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
        _task_ids: Vec<crawlrs::domain::id::Id>,
        _team_id: crawlrs::domain::id::Id,
        _force: bool,
    ) -> Result<
        (
            Vec<crawlrs::domain::id::Id>,
            Vec<(crawlrs::domain::id::Id, String)>,
        ),
        RepositoryError,
    > {
        Ok((vec![], vec![]))
    }

    async fn renew_lock(
        &self,
        _task_id: crawlrs::domain::id::Id,
        _worker_id: crawlrs::domain::id::Id,
        _extend_seconds: i64,
    ) -> Result<bool, RepositoryError> {
        Ok(true)
    }
}

// ============================================================================
// MockScrapeResultRepository
// ============================================================================

/// Noop `ScrapeResultRepository` — all methods return Ok with default values.
pub struct MockScrapeResultRepository;

#[async_trait]
impl ScrapeResultRepository for MockScrapeResultRepository {
    async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
        Ok(())
    }
    async fn find_by_task_id(
        &self,
        _task_id: crawlrs::domain::id::Id,
    ) -> anyhow::Result<Option<ScrapeResult>> {
        Ok(None)
    }
    async fn find_by_task_ids(
        &self,
        _task_ids: &[crawlrs::domain::id::Id],
    ) -> anyhow::Result<Vec<ScrapeResult>> {
        Ok(vec![])
    }
    async fn get_team_avg_response_time(
        &self,
        _team_id: crawlrs::domain::id::Id,
    ) -> anyhow::Result<f64> {
        Ok(0.0)
    }

    async fn cleanup_expired(&self, _retention_days: i64) -> anyhow::Result<u64> {
        Ok(0)
    }
}

// ============================================================================
// MockCrawlRepository
// ============================================================================

/// Noop `CrawlRepository` — all methods return Ok with default values.
pub struct MockCrawlRepository;

#[async_trait]
impl CrawlRepository for MockCrawlRepository {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        Ok(crawl.clone())
    }
    async fn find_by_id(
        &self,
        _id: crawlrs::domain::id::Id,
    ) -> Result<Option<Crawl>, RepositoryError> {
        Ok(None)
    }
    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        Ok(crawl.clone())
    }
    async fn increment_completed_tasks(
        &self,
        _id: crawlrs::domain::id::Id,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn increment_failed_tasks(
        &self,
        _id: crawlrs::domain::id::Id,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn update_status(
        &self,
        _id: crawlrs::domain::id::Id,
        _status: CrawlStatus,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn increment_total_tasks(
        &self,
        _id: crawlrs::domain::id::Id,
    ) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn find_by_team_id_paginated(
        &self,
        _team_id: crawlrs::domain::id::Id,
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<Crawl>, RepositoryError> {
        Ok(vec![])
    }
    async fn count_by_team_id(
        &self,
        _team_id: crawlrs::domain::id::Id,
    ) -> Result<u64, RepositoryError> {
        Ok(0)
    }
}

// ============================================================================
// MockCreditsRepository
// ============================================================================

/// Noop `CreditsRepository` — returns default balance (100), deductions are discarded.
pub struct MockCreditsRepository;

#[async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn get_balance(
        &self,
        _team_id: crawlrs::domain::id::Id,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
    async fn deduct_credits(
        &self,
        _team_id: crawlrs::domain::id::Id,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<crawlrs::domain::id::Id>,
    ) -> Result<(), CreditsRepositoryError> {
        Ok(())
    }
    async fn add_credits(
        &self,
        _team_id: crawlrs::domain::id::Id,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<crawlrs::domain::id::Id>,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
    async fn get_transaction_history(
        &self,
        _team_id: crawlrs::domain::id::Id,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }
    async fn initialize_team_credits(
        &self,
        _team_id: crawlrs::domain::id::Id,
        _initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
}
