// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Repository module for Shaku dependency injection.
//!
//! This module provides Shaku components for repository layer dependencies
//! with optimized instance caching using `OnceCell` for singleton pattern.
//!
//! # Performance Optimization
//!
//! Each repository component uses `OnceCell` to cache the underlying repository
//! implementation, avoiding repeated instantiation on every method call.
//!
//! # Macro-based Component Generation
//!
//! The `impl_repository_component!` macro generates boilerplate code for
//! repository components, significantly reducing code duplication.

use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;

use shaku::{Component, HasComponent, Module, ModuleBuildContext};

use dbnexus::DbPool;

use crate::domain::repositories::audit_log_repository::AuditLogRepository;
use crate::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
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
use crate::infrastructure::storage::LocalStorage;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use anyhow::Result;

use super::database_module::DatabasePoolTrait;

// =============================================================================
// Repository Component Macro
// =============================================================================

/// Macro to generate repository component with cached implementation.
///
/// This macro generates:
/// - Struct definition with `pool` and `repo_cache` fields
/// - `Component` implementation for Shaku DI
/// - `new()` constructor
/// - `get_repo()` method for lazy initialization
/// - `Deref` implementation for transparent delegation
///
/// # Arguments
///
/// * `$component_name` - Name of the component struct
/// * `$impl_type` - The concrete repository implementation type
/// * `$trait_type` - The trait type this component implements
/// * `$pool_field` - The field name in DatabasePool to use (usually `inner`)
///
/// # Example
///
/// ```ignore
/// impl_repository_component!(
///     CreditsRepositoryComponent,
///     CreditsRepositoryImpl,
///     CreditsRepository
/// );
/// ```
macro_rules! impl_repository_component {
    ($component_name:ident, $impl_type:ty, $trait_type:path) => {
        /// Repository component with cached implementation instance.
        ///
        /// Uses `OnceLock` to cache the underlying repository implementation,
        /// avoiding repeated instantiation on every method call.
        pub struct $component_name {
            pool: Arc<DatabasePool>,
            /// Cached repository instance
            repo_cache: OnceLock<$impl_type>,
        }

        impl<M: Module + HasComponent<dyn DatabasePoolTrait>> Component<M> for $component_name {
            type Interface = dyn $trait_type;
            type Parameters = ();

            fn build(context: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
                let pool_component: Arc<dyn DatabasePoolTrait> = M::build_component(context);
                Box::new(Self::new(pool_component.get_pool()))
            }
        }

        impl $component_name {
            /// Create a new repository component with explicit dependencies.
            pub fn new(pool: Arc<DatabasePool>) -> Self {
                Self {
                    pool,
                    repo_cache: OnceLock::new(),
                }
            }

            /// Get or create the cached repository instance.
            fn get_repo(&self) -> &$impl_type {
                self.repo_cache.get_or_init(|| <$impl_type>::new(self.pool.clone_inner()))
            }
        }

        impl Deref for $component_name {
            type Target = $impl_type;

            fn deref(&self) -> &Self::Target {
                self.get_repo()
            }
        }
    };
}

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

impl<M: Module + HasComponent<dyn DatabasePoolTrait>> Component<M> for TaskRepositoryComponent {
    type Interface = dyn TaskRepository;
    type Parameters = i64;

    fn build(context: &mut ModuleBuildContext<M>, lock_duration: Self::Parameters) -> Box<Self::Interface> {
        let pool_component: Arc<dyn DatabasePoolTrait> = M::build_component(context);
        let pool = pool_component.get_pool();
        Box::new(Self::new(pool, lock_duration))
    }
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
// Standard Repository Components (using macro)
// =============================================================================

impl_repository_component!(
    CreditsRepositoryComponent,
    CreditsRepositoryImpl,
    CreditsRepository
);

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
        self.get_repo().get_transaction_history(team_id, limit).await
    }

    async fn initialize_team_credits(
        &self,
        team_id: uuid::Uuid,
        initial_balance: i64,
    ) -> Result<i64, crate::domain::repositories::credits_repository::CreditsRepositoryError> {
        self.get_repo().initialize_team_credits(team_id, initial_balance).await
    }
}

impl_repository_component!(
    CrawlRepositoryComponent,
    CrawlRepositoryImpl,
    CrawlRepository
);

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
        self.get_repo().find_by_team_id_paginated(team_id, limit, offset).await
    }

    async fn count_by_team_id(
        &self,
        team_id: uuid::Uuid,
    ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError> {
        self.get_repo().count_by_team_id(team_id).await
    }
}

impl_repository_component!(
    ScrapeResultRepositoryComponent,
    ScrapeResultRepositoryImpl,
    ScrapeResultRepository
);

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

impl_repository_component!(
    WebhookRepositoryComponent,
    WebhookRepoImpl,
    WebhookRepository
);

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

impl_repository_component!(
    WebhookEventRepositoryComponent,
    WebhookEventRepoImpl,
    WebhookEventRepository
);

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
        self.get_repo().find_by_team_id_paginated(team_id, limit, offset).await
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

impl_repository_component!(
    TasksBacklogRepositoryComponent,
    TasksBacklogRepositoryImpl,
    TasksBacklogRepository
);

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

impl_repository_component!(
    AuthScopeRepositoryComponent,
    AuthScopeRepositoryImpl,
    AuthScopeRepository
);

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

impl_repository_component!(
    AuditLogRepositoryComponent,
    AuditLogRepositoryImpl,
    AuditLogRepository
);

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
        self.get_repo().find_by_api_key_id(api_key_id, limit, offset).await
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
        self.get_repo().find_by_team_id(team_id, limit, offset).await
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
#[derive(Component)]
#[shaku(interface = GeoRestrictionRepository)]
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
        self.repo_cache.get_or_init(|| DatabaseGeoRestrictionRepository::new(self.db.clone()))
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
        self.get_repo().update_team_restrictions(team_id, restrictions).await
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
// StorageRepository Component
// =============================================================================

/// StorageRepository component using LocalStorage with cached instance.
pub struct StorageRepositoryComponent {
    /// Storage path
    storage_path: String,
    /// Cached storage instance
    storage_cache: OnceLock<LocalStorage>,
}

impl<M: Module> Component<M> for StorageRepositoryComponent {
    type Interface = dyn StorageRepository;
    type Parameters = ();

    fn build(_: &mut ModuleBuildContext<M>, _: Self::Parameters) -> Box<Self::Interface> {
        Box::new(Self::with_default_path())
    }
}

impl StorageRepositoryComponent {
    /// Create a new StorageRepositoryComponent with explicit path.
    pub fn new(storage_path: String) -> Self {
        Self {
            storage_path,
            storage_cache: OnceLock::new(),
        }
    }

    /// Create with default storage path ("./storage").
    pub fn with_default_path() -> Self {
        Self {
            storage_path: "./storage".to_string(),
            storage_cache: OnceLock::new(),
        }
    }

    /// Get or create the cached storage instance.
    fn get_storage(&self) -> &LocalStorage {
        self.storage_cache.get_or_init(|| LocalStorage::new(self.storage_path.clone()))
    }
}

#[async_trait::async_trait]
impl StorageRepository for StorageRepositoryComponent {
    async fn save(
        &self,
        key: &str,
        data: &[u8],
    ) -> Result<(), crate::domain::repositories::storage_repository::StorageError> {
        self.get_storage().save(key, data).await
    }

    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Vec<u8>>, crate::domain::repositories::storage_repository::StorageError>
    {
        self.get_storage().get(key).await
    }

    async fn delete(
        &self,
        key: &str,
    ) -> Result<(), crate::domain::repositories::storage_repository::StorageError> {
        self.get_storage().delete(key).await
    }

    async fn exists(
        &self,
        key: &str,
    ) -> Result<bool, crate::domain::repositories::storage_repository::StorageError> {
        self.get_storage().exists(key).await
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
    ) -> Result<Option<crate::domain::models::Task>, crate::queue::task_queue::QueueError>
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
