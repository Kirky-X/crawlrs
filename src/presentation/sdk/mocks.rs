// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Mock implementations of SDK-layer service traits.
//!
//! Used by:
//! - `src/presentation/sdk/tests.rs` `#[cfg(test)] mod tests` (unit tests)
//! - `tests/sdk_api_test.rs` (integration tests, via
//!   `crawlrs::presentation::sdk::mocks::*`)
//!
//! # Layout
//!
//! - Success mocks (`MockSearchService`, `MockTaskQueue`, `MockCrawlRepository`)
//!   echo inputs back or return benign defaults — used to exercise happy paths.
//! - Error mocks (`MockSearchServiceError`, `MockTaskQueueError`,
//!   `MockCrawlRepositoryError`) always return a fixed error — used to exercise
//!   error mapping in SDK handlers. 当前在主测试套件中未直接引用，保留作为
//!   SDK 错误路径回归测试的预留基础设施。
//!
//! # 可见性门禁
//!
//! `mod.rs` 通过 `#[cfg(any(test, feature = "test-mocks"))] pub mod mocks;`
//! 暴露本模块：
//! - unit test 自动通过 `cfg(test)` 启用（`cargo test --lib`）；
//! - integration test 通过 `cargo test --features test-mocks` 显式启用
//!   （`tests/sdk_api_test.rs` 通过 `crawlrs::presentation::sdk::mocks::*`
//!   引用，反转依赖方向，消除 `include!` hack）；
//! - production binary 默认不启用 `test-mocks`，mocks 完全不参与编译。

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::models::{Crawl, CrawlStatus, Task};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::services::search_service::{
    SearchQuery, SearchResponse, SearchResult, SearchServiceError, SearchServiceTrait,
};
use crate::queue::task_queue::{QueueError, TaskQueue};

// ============================================================================
// Success mocks
// ============================================================================

/// Success `SearchServiceTrait` mock — returns a fixed `SearchResponse`.
pub struct MockSearchService;

#[async_trait]
impl SearchServiceTrait for MockSearchService {
    async fn search(
        &self,
        _team_id: Uuid,
        _api_key_id: Uuid,
        query: SearchQuery,
    ) -> Result<SearchResponse, SearchServiceError> {
        Ok(SearchResponse {
            query: query.query,
            results: vec![SearchResult {
                title: "Mock Title".to_string(),
                url: "https://example.com".to_string(),
                description: Some("Mock desc".to_string()),
                engine: "mock".to_string(),
            }],
            crawl_id: None,
            credits_used: 1,
        })
    }
}

/// Success `TaskQueue` mock — `enqueue` echoes the input task; other methods are no-ops.
pub struct MockTaskQueue;

#[async_trait]
impl TaskQueue for MockTaskQueue {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        Ok(task)
    }
    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        Ok(None)
    }
    async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
    async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
    async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
}

/// Success `CrawlRepository` mock — `create` echoes input; other methods are no-ops.
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
// Error mocks
// ============================================================================

/// Error `SearchServiceTrait` mock — `search` always returns `SearchEngine` error.
#[allow(dead_code)]
pub struct MockSearchServiceError;

#[async_trait]
impl SearchServiceTrait for MockSearchServiceError {
    async fn search(
        &self,
        _team_id: Uuid,
        _api_key_id: Uuid,
        _query: SearchQuery,
    ) -> Result<SearchResponse, SearchServiceError> {
        Err(SearchServiceError::SearchEngine(
            "mock search engine error".to_string(),
        ))
    }
}

/// Error `TaskQueue` mock — `enqueue` always returns `Repository` error.
#[allow(dead_code)]
pub struct MockTaskQueueError;

#[async_trait]
impl TaskQueue for MockTaskQueueError {
    async fn enqueue(&self, _task: Task) -> Result<Task, QueueError> {
        Err(QueueError::Repository(RepositoryError::Database(
            anyhow::anyhow!("mock queue enqueue error"),
        )))
    }
    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        Ok(None)
    }
    async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
    async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
    async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
}

/// Error `CrawlRepository` mock — `create` always returns `Database` error.
#[allow(dead_code)]
pub struct MockCrawlRepositoryError;

#[async_trait]
impl CrawlRepository for MockCrawlRepositoryError {
    async fn create(&self, _crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        Err(RepositoryError::Database(anyhow::anyhow!(
            "mock crawl repo create error"
        )))
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
