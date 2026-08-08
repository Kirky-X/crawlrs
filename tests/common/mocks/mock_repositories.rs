// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! T042: 统一 mock 仓库实现 — 消除 25+ 重复定义
//!
//! 所有 mock 默认返回 noop/空值。需要特定行为的测试可自行扩展或配置。

#![allow(dead_code)]

use async_trait::async_trait;
use std::collections::HashSet;
use uuid::Uuid;

use crawlrs::domain::models::{Crawl, CrawlStatus, ScrapeResult, Task};
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::domain::repositories::credits_repository::{
    CreditsRepository, CreditsRepositoryError, CreditsTransaction, CreditsTransactionType,
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
    async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
        Ok(None)
    }
    async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
        Ok(vec![])
    }
    async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
        Ok(0.0)
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
    async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
        Ok(None)
    }
    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        Ok(crawl.clone())
    }
    async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
        Ok(())
    }
    async fn update_status(&self, _id: Uuid, _status: CrawlStatus) -> Result<(), RepositoryError> {
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

// ============================================================================
// MockCreditsRepository
// ============================================================================

/// Noop `CreditsRepository` — returns default balance (100), deductions are discarded.
pub struct MockCreditsRepository;

#[async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
    async fn deduct_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        Ok(())
    }
    async fn add_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }
    async fn initialize_team_credits(
        &self,
        _team_id: Uuid,
        _initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
}
