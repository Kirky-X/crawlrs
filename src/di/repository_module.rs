// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Repository module for dependency injection.
//!
//! This module provides components for repository layer dependencies
//! with optimized instance caching using `OnceCell` for singleton pattern.
//!
//! # Performance Optimization
//!
//! Each repository component uses `OnceCell` to cache the underlying repository
//! implementation, avoiding repeated instantiation on every method call.

use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;

use dbnexus::DbPool;

use crate::domain::repositories::audit_log_repository::AuditLogRepository;
use crate::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
use crate::infrastructure::database::repositories::auth_scope_repo_impl::AuthScopeRepositoryImpl;
use crate::infrastructure::database::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::database::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::database::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crate::infrastructure::database::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use anyhow::Result;

// =============================================================================
// TaskRepository Component with Instance Caching
// =============================================================================

/// TaskRepository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying `TaskRepositoryImpl` instance,
/// avoiding repeated instantiation on every method call.
pub struct TaskRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Task lock duration in seconds
    task_lock_duration_seconds: i64,
    /// Cached repository instance
    repo_cache: OnceLock<TaskRepositoryImpl>,
}

impl TaskRepositoryComponent {
    /// Create a new TaskRepositoryComponent with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>, task_lock_duration_seconds: i64) -> Self {
        Self {
            pool,
            task_lock_duration_seconds,
            repo_cache: OnceLock::new(),
        }
    }

    /// Create with default lock duration (300 seconds).
    pub fn with_pool(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            task_lock_duration_seconds: 300,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &TaskRepositoryImpl {
        self.repo_cache.get_or_init(|| {
            TaskRepositoryImpl::new(
                self.pool.clone_inner(),
                chrono::Duration::seconds(self.task_lock_duration_seconds),
            )
        })
    }
}

impl Deref for TaskRepositoryComponent {
    type Target = TaskRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl TaskRepository for TaskRepositoryComponent {
    async fn create(
        &self,
        task: &crate::domain::models::Task,
    ) -> Result<
        crate::domain::models::Task,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().create(task).await
    }

    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_id(id).await
    }

    async fn update(
        &self,
        task: &crate::domain::models::Task,
    ) -> Result<
        crate::domain::models::Task,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().update(task).await
    }

    async fn acquire_next(
        &self,
        worker_id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().acquire_next(worker_id).await
    }

    async fn mark_completed(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().mark_completed(id).await
    }

    async fn mark_failed(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().mark_failed(id).await
    }

    async fn mark_cancelled(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().mark_cancelled(id).await
    }

    async fn exists_by_url(
        &self,
        url: &str,
    ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().exists_by_url(url).await
    }

    async fn find_existing_urls(
        &self,
        urls: &[String],
    ) -> Result<
        std::collections::HashSet<String>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_existing_urls(urls).await
    }

    async fn reset_stuck_tasks(
        &self,
        timeout: chrono::Duration,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().reset_stuck_tasks(timeout).await
    }

    async fn cancel_tasks_by_crawl_id(
        &self,
        crawl_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().cancel_tasks_by_crawl_id(crawl_id).await
    }

    async fn expire_tasks(
        &self,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().expire_tasks().await
    }

    async fn find_by_crawl_id(
        &self,
        crawl_id: uuid::Uuid,
    ) -> Result<
        Vec<crate::domain::models::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_crawl_id(crawl_id).await
    }

    async fn query_tasks(
        &self,
        params: crate::domain::repositories::task_repository::TaskQueryParams,
    ) -> Result<
        (Vec<crate::domain::models::Task>, u64),
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().query_tasks(params).await
    }

    async fn batch_cancel(
        &self,
        task_ids: Vec<uuid::Uuid>,
        team_id: uuid::Uuid,
        force: bool,
    ) -> Result<
        (Vec<uuid::Uuid>, Vec<(uuid::Uuid, String)>),
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().batch_cancel(task_ids, team_id, force).await
    }
}

// =============================================================================
// Standard Repository Components
// =============================================================================

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct CreditsRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<CreditsRepositoryImpl>,
}

impl CreditsRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &CreditsRepositoryImpl {
        self.repo_cache
            .get_or_init(|| CreditsRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for CreditsRepositoryComponent {
    type Target = CreditsRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl CreditsRepository for CreditsRepositoryComponent {
    async fn get_balance(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        self.get_repo().get_balance(team_id).await
    }

    async fn deduct_credits(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<(), crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        self.get_repo()
            .deduct_credits(team_id, amount, transaction_type, description, reference_id)
            .await
    }

    async fn add_credits(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        self.get_repo()
            .add_credits(team_id, amount, transaction_type, description, reference_id)
            .await
    }

    async fn get_transaction_history(
        &self,
        team_id: uuid::Uuid,
        limit: Option<u32>,
    ) -> Result<
        Vec<crate::domain::models::CreditsTransaction>,
        crate::domain::repositories::credits_repository::CreditsRepositoryError,
    > {
        self.get_repo()
            .get_transaction_history(team_id, limit)
            .await
    }

    async fn initialize_team_credits(
        &self,
        team_id: uuid::Uuid,
        initial_balance: i64,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        self.get_repo()
            .initialize_team_credits(team_id, initial_balance)
            .await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct CrawlRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<CrawlRepositoryImpl>,
}

impl CrawlRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &CrawlRepositoryImpl {
        self.repo_cache
            .get_or_init(|| CrawlRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for CrawlRepositoryComponent {
    type Target = CrawlRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl CrawlRepository for CrawlRepositoryComponent {
    async fn create(
        &self,
        crawl: &crate::domain::models::Crawl,
    ) -> Result<
        crate::domain::models::Crawl,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().create(crawl).await
    }

    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::Crawl>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_id(id).await
    }

    async fn update(
        &self,
        crawl: &crate::domain::models::Crawl,
    ) -> Result<
        crate::domain::models::Crawl,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().update(crawl).await
    }

    async fn update_status(
        &self,
        id: uuid::Uuid,
        status: crate::domain::models::CrawlStatus,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().update_status(id, status).await
    }

    async fn increment_completed_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().increment_completed_tasks(id).await
    }

    async fn increment_failed_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().increment_failed_tasks(id).await
    }

    async fn increment_total_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().increment_total_tasks(id).await
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: uuid::Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<
        Vec<crate::domain::models::Crawl>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo()
            .find_by_team_id_paginated(team_id, limit, offset)
            .await
    }

    async fn count_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().count_by_team_id(team_id).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct ScrapeResultRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<ScrapeResultRepositoryImpl>,
}

impl ScrapeResultRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &ScrapeResultRepositoryImpl {
        self.repo_cache
            .get_or_init(|| ScrapeResultRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for ScrapeResultRepositoryComponent {
    type Target = ScrapeResultRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryComponent {
    async fn save(
        &self,
        result: crate::domain::models::scrape_result::ScrapeResult,
    ) -> anyhow::Result<()> {
        self.get_repo().save(result).await
    }

    async fn find_by_task_id(
        &self,
        task_id: uuid::Uuid,
    ) -> anyhow::Result<Option<crate::domain::models::scrape_result::ScrapeResult>> {
        self.get_repo().find_by_task_id(task_id).await
    }

    async fn find_by_task_ids(
        &self,
        task_ids: &[uuid::Uuid],
    ) -> anyhow::Result<Vec<crate::domain::models::scrape_result::ScrapeResult>> {
        self.get_repo().find_by_task_ids(task_ids).await
    }

    async fn get_team_avg_response_time(&self, team_id: uuid::Uuid) -> anyhow::Result<f64> {
        self.get_repo().get_team_avg_response_time(team_id).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct WebhookRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<WebhookRepoImpl>,
}

impl WebhookRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &WebhookRepoImpl {
        self.repo_cache
            .get_or_init(|| WebhookRepoImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for WebhookRepositoryComponent {
    type Target = WebhookRepoImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl WebhookRepository for WebhookRepositoryComponent {
    async fn create(
        &self,
        webhook: &crate::domain::models::Webhook,
    ) -> Result<
        crate::domain::models::Webhook,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().create(webhook).await
    }

    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::Webhook>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_id(id).await
    }

    async fn find_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<
        Vec<crate::domain::models::Webhook>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_team_id(team_id).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct WebhookEventRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<WebhookEventRepoImpl>,
}

impl WebhookEventRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &WebhookEventRepoImpl {
        self.repo_cache
            .get_or_init(|| WebhookEventRepoImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for WebhookEventRepositoryComponent {
    type Target = WebhookEventRepoImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl WebhookEventRepository for WebhookEventRepositoryComponent {
    async fn create(
        &self,
        event: &crate::domain::models::WebhookEvent,
    ) -> Result<
        crate::domain::models::WebhookEvent,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().create(event).await
    }

    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_id(id).await
    }

    async fn find_pending(
        &self,
        limit: u64,
    ) -> Result<
        Vec<crate::domain::models::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_pending(limit).await
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: uuid::Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<
        Vec<crate::domain::models::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo()
            .find_by_team_id_paginated(team_id, limit, offset)
            .await
    }

    async fn count_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().count_by_team_id(team_id).await
    }

    async fn update(
        &self,
        event: &crate::domain::models::WebhookEvent,
    ) -> Result<
        crate::domain::models::WebhookEvent,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().update(event).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct TasksBacklogRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<TasksBacklogRepositoryImpl>,
}

impl TasksBacklogRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &TasksBacklogRepositoryImpl {
        self.repo_cache
            .get_or_init(|| TasksBacklogRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for TasksBacklogRepositoryComponent {
    type Target = TasksBacklogRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl TasksBacklogRepository for TasksBacklogRepositoryComponent {
    async fn create(
        &self,
        backlog: &crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<
        crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().create(backlog).await
    }

    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_id(id).await
    }

    async fn find_by_task_id(
        &self,
        task_id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().find_by_task_id(task_id).await
    }

    async fn update(
        &self,
        backlog: &crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<
        crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().update(backlog).await
    }

    async fn delete(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().delete(id).await
    }

    async fn get_pending_tasks(
        &self,
        team_id: Option<uuid::Uuid>,
        limit: Option<u64>,
    ) -> Result<
        Vec<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().get_pending_tasks(team_id, limit).await
    }

    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<
        Vec<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        self.get_repo().get_expired_tasks(limit).await
    }

    async fn count_by_status(
        &self,
        team_id: Option<uuid::Uuid>,
        status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
    ) -> Result<i64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().count_by_status(team_id, status).await
    }

    async fn update_status_batch(
        &self,
        ids: &[uuid::Uuid],
        status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().update_status_batch(ids, status).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct AuthScopeRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<AuthScopeRepositoryImpl>,
}

impl AuthScopeRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &AuthScopeRepositoryImpl {
        self.repo_cache
            .get_or_init(|| AuthScopeRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for AuthScopeRepositoryComponent {
    type Target = AuthScopeRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl AuthScopeRepository for AuthScopeRepositoryComponent {
    async fn find_by_api_key_id(
        &self,
        api_key_id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::auth::ApiKeyScope>,
        crate::domain::repositories::auth_scope_repository::RepositoryError,
    > {
        self.get_repo().find_by_api_key_id(api_key_id).await
    }

    async fn find_by_api_key(
        &self,
        key: &str,
    ) -> Result<
        Option<crate::domain::auth::ApiKeyScope>,
        crate::domain::repositories::auth_scope_repository::RepositoryError,
    > {
        self.get_repo().find_by_api_key(key).await
    }

    async fn upsert(
        &self,
        api_key_id: uuid::Uuid,
        scope: crate::domain::auth::ApiKeyScope,
    ) -> Result<
        crate::domain::auth::ApiKeyScope,
        crate::domain::repositories::auth_scope_repository::RepositoryError,
    > {
        self.get_repo().upsert(api_key_id, scope).await
    }

    async fn delete_by_api_key_id(
        &self,
        api_key_id: uuid::Uuid,
    ) -> Result<bool, crate::domain::repositories::auth_scope_repository::RepositoryError> {
        self.get_repo().delete_by_api_key_id(api_key_id).await
    }
}

/// Repository component with cached implementation instance.
///
/// Uses `OnceLock` to cache the underlying repository implementation,
/// avoiding repeated instantiation on every method call.
pub struct AuditLogRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Cached repository instance
    repo_cache: OnceLock<AuditLogRepositoryImpl>,
}

impl AuditLogRepositoryComponent {
    /// Create a new repository component with explicit dependencies.
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &AuditLogRepositoryImpl {
        self.repo_cache
            .get_or_init(|| AuditLogRepositoryImpl::new(self.pool.clone_inner()))
    }
}

impl Deref for AuditLogRepositoryComponent {
    type Target = AuditLogRepositoryImpl;

    fn deref(&self) -> &Self::Target {
        self.get_repo()
    }
}

#[async_trait::async_trait]
impl AuditLogRepository for AuditLogRepositoryComponent {
    async fn create(
        &self,
        entry: &crate::domain::auth::AuditLogEntry,
    ) -> Result<
        crate::domain::auth::AuditLogEntry,
        crate::domain::repositories::audit_log_repository::AuditRepositoryError,
    > {
        self.get_repo().create(entry).await
    }

    async fn find_by_api_key_id(
        &self,
        api_key_id: uuid::Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::repositories::audit_log_repository::AuditRepositoryError,
    > {
        self.get_repo()
            .find_by_api_key_id(api_key_id, limit, offset)
            .await
    }

    async fn find_by_team_id(
        &self,
        team_id: uuid::Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::repositories::audit_log_repository::AuditRepositoryError,
    > {
        self.get_repo()
            .find_by_team_id(team_id, limit, offset)
            .await
    }

    async fn find_denied_for_key(
        &self,
        api_key_id: uuid::Uuid,
        limit: u64,
    ) -> Result<
        Vec<crate::domain::auth::AuditLogEntry>,
        crate::domain::repositories::audit_log_repository::AuditRepositoryError,
    > {
        self.get_repo().find_denied_for_key(api_key_id, limit).await
    }

    async fn cleanup_old_logs(
        &self,
        retention_days: i64,
    ) -> Result<u64, crate::domain::repositories::audit_log_repository::AuditRepositoryError> {
        self.get_repo().cleanup_old_logs(retention_days).await
    }
}

// =============================================================================
// GeoRestrictionRepository Component (uses DbPool instead of DatabasePool)
// =============================================================================

/// GeoRestrictionRepository component with cached implementation instance.
#[allow(dead_code)]
pub struct GeoRestrictionRepositoryComponent {
    db: Arc<DbPool>,
    /// Cached repository instance
    repo_cache: OnceLock<DatabaseGeoRestrictionRepository>,
}

impl GeoRestrictionRepositoryComponent {
    /// Create a new GeoRestrictionRepositoryComponent with explicit dependencies.
    pub fn new(db: Arc<DbPool>) -> Self {
        Self {
            db,
            repo_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached repository instance.
    fn get_repo(&self) -> &DatabaseGeoRestrictionRepository {
        self.repo_cache
            .get_or_init(|| DatabaseGeoRestrictionRepository::new(self.db.clone()))
    }
}

#[async_trait::async_trait]
impl GeoRestrictionRepository for GeoRestrictionRepositoryComponent {
    async fn get_team_restrictions(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<
        crate::domain::services::team_service::TeamGeoRestrictions,
        crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        self.get_repo().get_team_restrictions(team_id).await
    }

    async fn update_team_restrictions(
        &self,
        team_id: uuid::Uuid,
        restrictions: &crate::domain::services::team_service::TeamGeoRestrictions,
    ) -> Result<
        (),
        crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        self.get_repo()
            .update_team_restrictions(team_id, restrictions)
            .await
    }

    async fn log_geo_restriction_action(
        &self,
        team_id: uuid::Uuid,
        ip_address: &str,
        country_code: &str,
        action: &str,
        reason: &str,
    ) -> Result<
        (),
        crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        self.get_repo()
            .log_geo_restriction_action(team_id, ip_address, country_code, action, reason)
            .await
    }
}

// =============================================================================
// TaskQueue Component
// =============================================================================

/// TaskQueue component using PostgresTaskQueue.
pub struct TaskQueueComponent {
    task_repo: Arc<dyn TaskRepository>,
}

impl TaskQueueComponent {
    /// Create a new TaskQueueComponent with explicit dependencies.
    pub fn new(task_repo: Arc<dyn TaskRepository>) -> Self {
        Self { task_repo }
    }
}

#[async_trait::async_trait]
impl TaskQueue for TaskQueueComponent {
    async fn enqueue(
        &self,
        task: crate::domain::models::Task,
    ) -> Result<crate::domain::models::Task, crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.enqueue(task).await
    }

    async fn dequeue(
        &self,
        worker_id: uuid::Uuid,
    ) -> Result<Option<crate::domain::models::Task>, crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.dequeue(worker_id).await
    }

    async fn complete(
        &self,
        task_id: uuid::Uuid,
    ) -> Result<(), crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.complete(task_id).await
    }

    async fn fail(&self, task_id: uuid::Uuid) -> Result<(), crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.fail(task_id).await
    }

    async fn cancel(
        &self,
        task_id: uuid::Uuid,
    ) -> Result<(), crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.cancel(task_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== TaskQueueComponent ==========
    // TaskQueueComponent 接受 Arc<dyn TaskRepository>，可用 mock 实现 TaskRepository
    // 来验证 enqueue/dequeue/complete/fail/cancel 的委托逻辑。

    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    use crate::domain::models::{Task, TaskType};
    use crate::domain::repositories::task_repository::RepositoryError;
    use crate::queue::task_queue::QueueError;

    /// Mock TaskRepository 用于测试 TaskQueueComponent 的委托逻辑。
    /// 使用原子计数器跟踪各方法调用次数，并支持返回失败以测试错误传播。
    struct MockTaskRepository {
        create_count: AtomicU32,
        acquire_next_count: AtomicU32,
        mark_completed_count: AtomicU32,
        mark_failed_count: AtomicU32,
        mark_cancelled_count: AtomicU32,
        should_fail: bool,
        dequeue_task: Mutex<Option<Task>>,
    }

    impl MockTaskRepository {
        fn success() -> Self {
            Self {
                create_count: AtomicU32::new(0),
                acquire_next_count: AtomicU32::new(0),
                mark_completed_count: AtomicU32::new(0),
                mark_failed_count: AtomicU32::new(0),
                mark_cancelled_count: AtomicU32::new(0),
                should_fail: false,
                dequeue_task: Mutex::new(None),
            }
        }

        fn failing() -> Self {
            Self {
                create_count: AtomicU32::new(0),
                acquire_next_count: AtomicU32::new(0),
                mark_completed_count: AtomicU32::new(0),
                mark_failed_count: AtomicU32::new(0),
                mark_cancelled_count: AtomicU32::new(0),
                should_fail: true,
                dequeue_task: Mutex::new(None),
            }
        }

        fn with_dequeue_task(task: Task) -> Self {
            Self {
                create_count: AtomicU32::new(0),
                acquire_next_count: AtomicU32::new(0),
                mark_completed_count: AtomicU32::new(0),
                mark_failed_count: AtomicU32::new(0),
                mark_cancelled_count: AtomicU32::new(0),
                should_fail: false,
                dequeue_task: Mutex::new(Some(task)),
            }
        }
    }

    #[async_trait::async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("create failed")));
            }
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: uuid::Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(
            &self,
            _worker_id: uuid::Uuid,
        ) -> Result<Option<Task>, RepositoryError> {
            self.acquire_next_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("acquire failed")));
            }
            let task = self
                .dequeue_task
                .lock()
                .expect("dequeue_task mutex poisoned")
                .take();
            Ok(task)
        }

        async fn mark_completed(&self, _id: uuid::Uuid) -> Result<(), RepositoryError> {
            self.mark_completed_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "completed failed"
                )));
            }
            Ok(())
        }

        async fn mark_failed(&self, _id: uuid::Uuid) -> Result<(), RepositoryError> {
            self.mark_failed_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("failed failed")));
            }
            Ok(())
        }

        async fn mark_cancelled(&self, _id: uuid::Uuid) -> Result<(), RepositoryError> {
            self.mark_cancelled_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "cancelled failed"
                )));
            }
            Ok(())
        }

        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }

        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<std::collections::HashSet<String>, RepositoryError> {
            Ok(std::collections::HashSet::new())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(
            &self,
            _crawl_id: uuid::Uuid,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(
            &self,
            _crawl_id: uuid::Uuid,
        ) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }

        async fn query_tasks(
            &self,
            _params: crate::domain::repositories::task_repository::TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<uuid::Uuid>,
            _team_id: uuid::Uuid,
            _force: bool,
        ) -> Result<(Vec<uuid::Uuid>, Vec<(uuid::Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    fn make_test_task() -> Task {
        Task::new(
            uuid::Uuid::new_v4(),
            TaskType::Scrape,
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::Value::Null,
        )
    }

    #[test]
    fn test_task_queue_component_new_stores_repository() {
        let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(repo);
        // 构造成功即可验证；trait 方法需要实际调用 repository，在异步测试中验证
        let _trait_obj: &dyn TaskQueue = &component;
    }

    #[tokio::test]
    async fn test_task_queue_component_enqueue_calls_create_and_returns_task() {
        let mock = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let task = make_test_task();
        let result = component.enqueue(task.clone()).await;
        assert!(result.is_ok(), "enqueue should succeed");
        let returned = result.expect("enqueue result");
        assert_eq!(returned.id, task.id);
        assert_eq!(mock.create_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_enqueue_propagates_repository_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let task = make_test_task();
        let result = component.enqueue(task).await;
        assert!(result.is_err());
        match result {
            Err(QueueError::Repository(_)) => {}
            other => panic!("Expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(mock.create_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_dequeue_returns_task_when_available() {
        let task = make_test_task();
        let task_id = task.id;
        let mock = Arc::new(MockTaskRepository::with_dequeue_task(task));
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.dequeue(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
        let dequeued = result.expect("dequeue result");
        assert!(dequeued.is_some());
        assert_eq!(dequeued.expect("task").id, task_id);
        assert_eq!(mock.acquire_next_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_dequeue_returns_none_when_empty() {
        let mock = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.dequeue(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert!(result.expect("dequeue result").is_none());
        assert_eq!(mock.acquire_next_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_complete_calls_mark_completed() {
        let mock = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.complete(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(mock.mark_completed_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_fail_calls_mark_failed() {
        let mock = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.fail(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(mock.mark_failed_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_cancel_calls_mark_cancelled() {
        let mock = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.cancel(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(mock.mark_cancelled_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_task_queue_component_complete_propagates_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.complete(uuid::Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_task_queue_component_fail_propagates_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.fail(uuid::Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_task_queue_component_cancel_propagates_error() {
        let mock = Arc::new(MockTaskRepository::failing());
        let component = TaskQueueComponent::new(mock.clone() as Arc<dyn TaskRepository>);
        let result = component.cancel(uuid::Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_task_queue_component_as_trait_object() {
        let mock: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::success());
        let component = TaskQueueComponent::new(mock);
        let trait_obj: &dyn TaskQueue = &component;
        let result = trait_obj.complete(uuid::Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    // ========== 跳过的组件 ==========
    // 以下组件的构造器需要 Arc<DatabasePool> 或 Arc<DbPool>，而 DatabasePool/DbPool
    // 必须连接真实数据库才能构造。无法在无数据库环境下测试，故跳过：
    // - TaskRepositoryComponent::new / with_pool — 需 Arc<DatabasePool>
    // - CreditsRepositoryComponent::new — 需 Arc<DatabasePool>
    // - CrawlRepositoryComponent::new — 需 Arc<DatabasePool>
    // - ScrapeResultRepositoryComponent::new — 需 Arc<DatabasePool>
    // - WebhookRepositoryComponent::new — 需 Arc<DatabasePool>
    // - WebhookEventRepositoryComponent::new — 需 Arc<DatabasePool>
    // - TasksBacklogRepositoryComponent::new — 需 Arc<DatabasePool>
    // - AuthScopeRepositoryComponent::new — 需 Arc<DatabasePool>
    // - AuditLogRepositoryComponent::new — 需 Arc<DatabasePool>
    // - GeoRestrictionRepositoryComponent::new — 需 Arc<DbPool>

    // ========== testcontainers integration tests ==========
    //
    // These tests exercise repository components that require a real
    // `Arc<DatabasePool>` (PostgreSQL). They early-return if Docker is
    // unavailable.

    use crate::bootstrap::infrastructure::init_database;
    use crate::common::test_support::testcontainers_fixtures as tcf;

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    #[tokio::test]
    async fn tc_task_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_task_repository_component_new_and_deref");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = TaskRepositoryComponent::new(db.clone(), 300);
        // Deref should give us a &TaskRepositoryImpl without panicking.
        let _impl: &TaskRepositoryImpl = &component;
        // Verify the component can be created with with_pool too.
        let component2 = TaskRepositoryComponent::with_pool(db);
        let _impl2: &TaskRepositoryImpl = &component2;
    }

    #[tokio::test]
    async fn tc_task_repository_component_get_repo_caches_instance() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_task_repository_component_get_repo_caches_instance"
            );
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = TaskRepositoryComponent::new(db, 300);
        // Accessing deref twice should return the same cached instance
        // (OnceLock guarantees single initialization).
        // Using &*component for raw pointer coercion; auto-deref doesn't
        // apply here, so we allow the clippy lint.
        #[allow(clippy::explicit_auto_deref)]
        let ptr1: *const TaskRepositoryImpl = &*component;
        #[allow(clippy::explicit_auto_deref)]
        let ptr2: *const TaskRepositoryImpl = &*component;
        assert_eq!(ptr1, ptr2, "get_repo should cache the repository instance");
    }

    #[tokio::test]
    async fn tc_credits_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_credits_repository_component_new_and_deref");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = CreditsRepositoryComponent::new(db);
        let _impl: &CreditsRepositoryImpl = &component;
    }

    #[tokio::test]
    async fn tc_crawl_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_crawl_repository_component_new_and_deref");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = CrawlRepositoryComponent::new(db);
        let _impl: &CrawlRepositoryImpl = &component;
    }

    #[tokio::test]
    async fn tc_scrape_result_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_scrape_result_repository_component_new_and_deref"
            );
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = ScrapeResultRepositoryComponent::new(db);
        let _impl: &ScrapeResultRepositoryImpl = &component;
    }

    #[tokio::test]
    async fn tc_webhook_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_webhook_repository_component_new_and_deref");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = WebhookRepositoryComponent::new(db);
        let _impl: &WebhookRepoImpl = &component;
    }

    #[tokio::test]
    async fn tc_webhook_event_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_webhook_event_repository_component_new_and_deref"
            );
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = WebhookEventRepositoryComponent::new(db);
        let _impl: &WebhookEventRepoImpl = &component;
    }

    #[tokio::test]
    async fn tc_tasks_backlog_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_tasks_backlog_repository_component_new_and_deref"
            );
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        let component = TasksBacklogRepositoryComponent::new(db);
        let _impl: &TasksBacklogRepositoryImpl = &component;
    }

    #[tokio::test]
    async fn tc_geo_restriction_repository_component_new_and_deref() {
        if !require_docker().await {
            eprintln!(
                "[skip] Docker unavailable — tc_geo_restriction_repository_component_new_and_deref"
            );
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };

        // GeoRestrictionRepositoryComponent uses Arc<DbPool> directly and
        // implements GeoRestrictionRepository trait (no Deref).
        let component = GeoRestrictionRepositoryComponent::new(db.inner().clone());
        // Verify it can be used as a trait object.
        let _trait_obj: &dyn GeoRestrictionRepository = &component;
    }
}
