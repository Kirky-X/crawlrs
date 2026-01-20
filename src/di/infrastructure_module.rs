// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure module for Shaku dependency injection.
//!
//! This module provides Shaku components for infrastructure layer dependencies
//! including database connection pool, Redis client, and repository implementations.

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::tasks_backlog_repository::TasksBacklogRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::database::connection::DatabasePool;
use crate::infrastructure::database::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::database::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::geo_restriction_repo_impl::InMemoryGeoRestrictionRepository;
use crate::infrastructure::database::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::database::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crate::infrastructure::database::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::infrastructure::storage::LocalStorage;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};

/// Trait for Database component
pub trait DatabasePoolTrait: Send + Sync {
    fn get_pool(&self) -> Arc<DatabasePool>;
}

/// Database component
pub struct DatabasePoolComponent {
    /// The actual database pool
    pool: Arc<DatabasePool>,
}

impl From<Arc<DatabasePool>> for DatabasePoolComponent {
    fn from(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

impl DatabasePoolTrait for DatabasePoolComponent {
    fn get_pool(&self) -> Arc<DatabasePool> {
        Arc::clone(&self.pool)
    }
}

/// Trait for RedisClient component
pub trait RedisClientTrait: Send + Sync {
    fn get_client(&self) -> Arc<RedisClient>;
}

/// RedisClient component
#[allow(dead_code)]
pub struct RedisClientComponent {
    /// Redis URL
    redis_url: String,
    /// Redis client
    client: Arc<RedisClient>,
}

impl RedisClientComponent {
    /// Create a new RedisClientComponent with explicit dependencies
    pub fn new(redis_url: String, client: Arc<RedisClient>) -> Self {
        Self { redis_url, client }
    }
}

impl RedisClientTrait for RedisClientComponent {
    fn get_client(&self) -> Arc<RedisClient> {
        self.client.clone()
    }
}

/// TaskRepository component
pub struct TaskRepositoryComponent {
    pool: Arc<DatabasePool>,
    /// Task lock duration in seconds
    task_lock_duration_seconds: i64,
}

impl TaskRepositoryComponent {
    /// Create a new TaskRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>, task_lock_duration_seconds: i64) -> Self {
        Self {
            pool,
            task_lock_duration_seconds,
        }
    }

    /// Create with default lock duration (300 seconds)
    pub fn with_pool(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            task_lock_duration_seconds: 300,
        }
    }
}

#[async_trait::async_trait]
impl TaskRepository for TaskRepositoryComponent {
    async fn create(
        &self,
        task: &crate::domain::models::task::Task,
    ) -> Result<
        crate::domain::models::task::Task,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.create(task).await
    }
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::task::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.find_by_id(id).await
    }
    async fn update(
        &self,
        task: &crate::domain::models::task::Task,
    ) -> Result<
        crate::domain::models::task::Task,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.update(task).await
    }
    async fn acquire_next(
        &self,
        worker_id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::task::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.acquire_next(worker_id).await
    }
    async fn mark_completed(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.mark_completed(id).await
    }
    async fn mark_failed(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.mark_failed(id).await
    }
    async fn mark_cancelled(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.mark_cancelled(id).await
    }
    async fn exists_by_url(
        &self,
        url: &str,
    ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.exists_by_url(url).await
    }
    async fn find_existing_urls(
        &self,
        urls: &[String],
    ) -> Result<
        std::collections::HashSet<String>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.find_existing_urls(urls).await
    }
    async fn reset_stuck_tasks(
        &self,
        timeout: chrono::Duration,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.reset_stuck_tasks(timeout).await
    }
    async fn cancel_tasks_by_crawl_id(
        &self,
        crawl_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.cancel_tasks_by_crawl_id(crawl_id).await
    }
    async fn expire_tasks(
        &self,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.expire_tasks().await
    }
    async fn find_by_crawl_id(
        &self,
        crawl_id: uuid::Uuid,
    ) -> Result<
        Vec<crate::domain::models::task::Task>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.find_by_crawl_id(crawl_id).await
    }
    async fn query_tasks(
        &self,
        params: crate::domain::repositories::task_repository::TaskQueryParams,
    ) -> Result<
        (Vec<crate::domain::models::task::Task>, u64),
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.query_tasks(params).await
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
        let repo = TaskRepositoryImpl::new(
            self.pool.0.clone(),
            chrono::Duration::seconds(self.task_lock_duration_seconds),
        );
        repo.batch_cancel(task_ids, team_id, force).await
    }
}

/// CreditsRepository component
pub struct CreditsRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl CreditsRepositoryComponent {
    /// Create a new CreditsRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl CreditsRepository for CreditsRepositoryComponent {
    async fn get_balance(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        let repo = CreditsRepositoryImpl::new(self.pool.0.clone());
        repo.get_balance(team_id).await
    }
    async fn deduct_credits(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::credits::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<(), crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        let repo = CreditsRepositoryImpl::new(self.pool.0.clone());
        repo.deduct_credits(team_id, amount, transaction_type, description, reference_id)
            .await
    }
    async fn add_credits(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::credits::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        let repo = CreditsRepositoryImpl::new(self.pool.0.clone());
        repo.add_credits(team_id, amount, transaction_type, description, reference_id)
            .await
    }
    async fn get_transaction_history(
        &self,
        team_id: uuid::Uuid,
        limit: Option<u32>,
    ) -> Result<
        Vec<crate::domain::models::credits::CreditsTransaction>,
        crate::domain::repositories::credits_repository::CreditsRepositoryError,
    > {
        let repo = CreditsRepositoryImpl::new(self.pool.0.clone());
        repo.get_transaction_history(team_id, limit).await
    }
    async fn initialize_team_credits(
        &self,
        team_id: uuid::Uuid,
        initial_balance: i64,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        let repo = CreditsRepositoryImpl::new(self.pool.0.clone());
        repo.initialize_team_credits(team_id, initial_balance).await
    }
}

/// CrawlRepository component
pub struct CrawlRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl CrawlRepositoryComponent {
    /// Create a new CrawlRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl CrawlRepository for CrawlRepositoryComponent {
    async fn create(
        &self,
        crawl: &crate::domain::models::crawl::Crawl,
    ) -> Result<
        crate::domain::models::crawl::Crawl,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.create(crawl).await
    }
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::crawl::Crawl>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_id(id).await
    }
    async fn update(
        &self,
        crawl: &crate::domain::models::crawl::Crawl,
    ) -> Result<
        crate::domain::models::crawl::Crawl,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.update(crawl).await
    }
    async fn update_status(
        &self,
        id: uuid::Uuid,
        status: crate::domain::models::crawl::CrawlStatus,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.update_status(id, status).await
    }
    async fn increment_completed_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.increment_completed_tasks(id).await
    }
    async fn increment_failed_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.increment_failed_tasks(id).await
    }
    async fn increment_total_tasks(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.increment_total_tasks(id).await
    }
    async fn find_by_team_id_paginated(
        &self,
        team_id: uuid::Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<
        Vec<crate::domain::models::crawl::Crawl>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_team_id_paginated(team_id, limit, offset).await
    }
    async fn count_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = CrawlRepositoryImpl::new(self.pool.0.clone());
        repo.count_by_team_id(team_id).await
    }
}

/// ScrapeResultRepository component
pub struct ScrapeResultRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl ScrapeResultRepositoryComponent {
    /// Create a new ScrapeResultRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryComponent {
    async fn save(
        &self,
        result: crate::domain::models::scrape_result::ScrapeResult,
    ) -> anyhow::Result<()> {
        let repo = ScrapeResultRepositoryImpl::new(self.pool.0.clone());
        repo.save(result).await
    }
    async fn find_by_task_id(
        &self,
        task_id: uuid::Uuid,
    ) -> anyhow::Result<Option<crate::domain::models::scrape_result::ScrapeResult>> {
        let repo = ScrapeResultRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_task_id(task_id).await
    }
    async fn find_by_task_ids(
        &self,
        task_ids: &[uuid::Uuid],
    ) -> anyhow::Result<Vec<crate::domain::models::scrape_result::ScrapeResult>> {
        let repo = ScrapeResultRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_task_ids(task_ids).await
    }
}

/// WebhookRepository component
pub struct WebhookRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl WebhookRepositoryComponent {
    /// Create a new WebhookRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl WebhookRepository for WebhookRepositoryComponent {
    async fn create(
        &self,
        webhook: &crate::domain::models::webhook::Webhook,
    ) -> Result<
        crate::domain::models::webhook::Webhook,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookRepoImpl::new(self.pool.0.clone());
        repo.create(webhook).await
    }
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::webhook::Webhook>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookRepoImpl::new(self.pool.0.clone());
        repo.find_by_id(id).await
    }
}

/// WebhookEventRepository component
pub struct WebhookEventRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl WebhookEventRepositoryComponent {
    /// Create a new WebhookEventRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl WebhookEventRepository for WebhookEventRepositoryComponent {
    async fn create(
        &self,
        event: &crate::domain::models::webhook::WebhookEvent,
    ) -> Result<
        crate::domain::models::webhook::WebhookEvent,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.create(event).await
    }
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::models::webhook::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.find_by_id(id).await
    }
    async fn find_pending(
        &self,
        limit: u64,
    ) -> Result<
        Vec<crate::domain::models::webhook::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.find_pending(limit).await
    }
    async fn find_by_team_id_paginated(
        &self,
        team_id: uuid::Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<
        Vec<crate::domain::models::webhook::WebhookEvent>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.find_by_team_id_paginated(team_id, limit, offset).await
    }
    async fn count_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.count_by_team_id(team_id).await
    }
    async fn update(
        &self,
        event: &crate::domain::models::webhook::WebhookEvent,
    ) -> Result<
        crate::domain::models::webhook::WebhookEvent,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = WebhookEventRepoImpl::new(self.pool.0.clone());
        repo.update(event).await
    }
}

/// TasksBacklogRepository component
pub struct TasksBacklogRepositoryComponent {
    pool: Arc<DatabasePool>,
}

impl TasksBacklogRepositoryComponent {
    /// Create a new TasksBacklogRepositoryComponent with explicit dependencies
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
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
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.create(backlog).await
    }
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_id(id).await
    }
    async fn find_by_task_id(
        &self,
        task_id: uuid::Uuid,
    ) -> Result<
        Option<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.find_by_task_id(task_id).await
    }
    async fn update(
        &self,
        backlog: &crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<
        crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.update(backlog).await
    }
    async fn delete(
        &self,
        id: uuid::Uuid,
    ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.delete(id).await
    }
    async fn get_pending_tasks(
        &self,
        team_id: Option<uuid::Uuid>,
        limit: Option<u64>,
    ) -> Result<
        Vec<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.get_pending_tasks(team_id, limit).await
    }
    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<
        Vec<crate::domain::repositories::tasks_backlog_repository::TasksBacklog>,
        crate::domain::repositories::task_repository::RepositoryError,
    > {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.get_expired_tasks(limit).await
    }
    async fn count_by_status(
        &self,
        team_id: Option<uuid::Uuid>,
        status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
    ) -> Result<i64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.count_by_status(team_id, status).await
    }
    async fn update_status_batch(
        &self,
        ids: &[uuid::Uuid],
        status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        let repo = TasksBacklogRepositoryImpl::new(self.pool.0.clone());
        repo.update_status_batch(ids, status).await
    }
}

/// GeoRestrictionRepository component
#[allow(dead_code)]
pub struct GeoRestrictionRepositoryComponent {
    db: Arc<DatabaseConnection>,
}

impl GeoRestrictionRepositoryComponent {
    /// Create a new GeoRestrictionRepositoryComponent with explicit dependencies
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
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
        let repo = DatabaseGeoRestrictionRepository::new(self.db.clone());
        repo.get_team_restrictions(team_id).await
    }
    async fn update_team_restrictions(
        &self,
        team_id: uuid::Uuid,
        restrictions: &crate::domain::services::team_service::TeamGeoRestrictions,
    ) -> Result<
        (),
        crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
    > {
        let repo = DatabaseGeoRestrictionRepository::new(self.db.clone());
        repo.update_team_restrictions(team_id, restrictions).await
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
        let repo = DatabaseGeoRestrictionRepository::new(self.db.clone());
        repo.log_geo_restriction_action(team_id, ip_address, country_code, action, reason)
            .await
    }
}

/// StorageRepository component using LocalStorage
pub struct StorageRepositoryComponent {
    /// Storage path
    storage_path: String,
}

impl StorageRepositoryComponent {
    /// Create a new StorageRepositoryComponent with explicit path
    pub fn new(storage_path: String) -> Self {
        Self { storage_path }
    }

    /// Create with default storage path ("./storage")
    pub fn with_default_path() -> Self {
        Self {
            storage_path: "./storage".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl StorageRepository for StorageRepositoryComponent {
    async fn save(
        &self,
        key: &str,
        data: &[u8],
    ) -> Result<(), crate::domain::repositories::storage_repository::StorageError> {
        let storage = LocalStorage::new(self.storage_path.clone());
        storage.save(key, data).await
    }
    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Vec<u8>>, crate::domain::repositories::storage_repository::StorageError>
    {
        let storage = LocalStorage::new(self.storage_path.clone());
        storage.get(key).await
    }
    async fn delete(
        &self,
        key: &str,
    ) -> Result<(), crate::domain::repositories::storage_repository::StorageError> {
        let storage = LocalStorage::new(self.storage_path.clone());
        storage.delete(key).await
    }
    async fn exists(
        &self,
        key: &str,
    ) -> Result<bool, crate::domain::repositories::storage_repository::StorageError> {
        let storage = LocalStorage::new(self.storage_path.clone());
        storage.exists(key).await
    }
}

/// TaskQueue component using PostgresTaskQueue
pub struct TaskQueueComponent {
    task_repo: Arc<dyn TaskRepository>,
}

impl TaskQueueComponent {
    /// Create a new TaskQueueComponent with explicit dependencies
    pub fn new(task_repo: Arc<dyn TaskRepository>) -> Self {
        Self { task_repo }
    }
}

#[async_trait::async_trait]
impl TaskQueue for TaskQueueComponent {
    async fn enqueue(
        &self,
        task: crate::domain::models::task::Task,
    ) -> Result<crate::domain::models::task::Task, crate::queue::task_queue::QueueError> {
        let queue = PostgresTaskQueue::new(self.task_repo.clone());
        queue.enqueue(task).await
    }
    async fn dequeue(
        &self,
        worker_id: uuid::Uuid,
    ) -> Result<Option<crate::domain::models::task::Task>, crate::queue::task_queue::QueueError>
    {
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

// Infrastructure module components - for Shaku DI

/// Trait for HttpClient component
pub trait HttpClientTrait: Send + Sync {
    fn get_client(&self) -> Arc<reqwest::Client>;
}

/// HttpClient component for unified HTTP client management
pub struct HttpClientComponent {
    /// The HTTP client
    client: Arc<reqwest::Client>,
}

impl HttpClientComponent {
    /// Create a new HttpClientComponent with explicit dependencies
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self { client }
    }
}

impl HttpClientTrait for HttpClientComponent {
    fn get_client(&self) -> Arc<reqwest::Client> {
        self.client.clone()
    }
}
