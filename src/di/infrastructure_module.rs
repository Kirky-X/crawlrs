// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure module for Shaku dependency injection.
//!
//! This module provides Shaku components for infrastructure layer dependencies
//! including database connection pool, Redis client, and repository implementations.

use shaku::Component;
use std::sync::Arc;

use crate::config::settings::Settings;
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
use crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl;
use crate::infrastructure::repositories::credits_repo_impl::CreditsRepositoryImpl;
use crate::infrastructure::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::infrastructure::repositories::tasks_backlog_repo_impl::TasksBacklogRepositoryImpl;
use crate::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::infrastructure::storage::LocalStorage;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};

/// Component parameters for InfrastructureModule
#[derive(shaku::ComponentParameters)]
pub struct InfrastructureModuleParameters {
    /// Application settings
    pub settings: Arc<Settings>,
}

/// DatabasePool component
#[derive(Component)]
#[shaku(interface = DatabasePool)]
pub struct DatabasePoolComponent {
    /// The actual database pool
    pool: Arc<DatabasePool>,
}

impl DatabasePool for DatabasePoolComponent {
    // Delegate to the internal pool implementation
}

impl From<Arc<DatabasePool>> for DatabasePoolComponent {
    fn from(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// RedisClient component
#[derive(Component)]
#[shaku(interface = RedisClient)]
pub struct RedisClientComponent {
    /// The actual Redis client
    client: Arc<RedisClient>,
}

impl RedisClient for RedisClientComponent {
    // Delegate to the internal client implementation
}

impl From<Arc<RedisClient>> for RedisClientComponent {
    fn from(client: Arc<RedisClient>) -> Self {
        Self { client }
    }
}

/// TaskRepository component
#[derive(Component)]
#[shaku(interface = TaskRepository)]
pub struct TaskRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,

    /// Task lock duration in seconds
    task_lock_duration_seconds: i64,
}

impl TaskRepository for TaskRepositoryComponent {
    // Implementation delegated to repository impl
}

impl TaskRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>, settings: &Settings) -> Self {
        Self {
            pool,
            task_lock_duration_seconds: settings.concurrency.task_lock_duration_seconds,
        }
    }
}

/// CreditsRepository component
#[derive(Component)]
#[shaku(interface = CreditsRepository)]
pub struct CreditsRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl CreditsRepository for CreditsRepositoryComponent {
    // Implementation delegated to repository impl
}

impl CreditsRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// CrawlRepository component
#[derive(Component)]
#[shaku(interface = CrawlRepository)]
pub struct CrawlRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl CrawlRepository for CrawlRepositoryComponent {
    // Implementation delegated to repository impl
}

impl CrawlRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// ScrapeResultRepository component
#[derive(Component)]
#[shaku(interface = ScrapeResultRepository)]
pub struct ScrapeResultRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl ScrapeResultRepository for ScrapeResultRepositoryComponent {
    // Implementation delegated to repository impl
}

impl ScrapeResultRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// WebhookRepository component
#[derive(Component)]
#[shaku(interface = WebhookRepository)]
pub struct WebhookRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl WebhookRepository for WebhookRepositoryComponent {
    // Implementation delegated to repository impl
}

impl WebhookRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// WebhookEventRepository component
#[derive(Component)]
#[shaku(interface = WebhookEventRepository)]
pub struct WebhookEventRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl WebhookEventRepository for WebhookEventRepositoryComponent {
    // Implementation delegated to repository impl
}

impl WebhookEventRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// TasksBacklogRepository component
#[derive(Component)]
#[shaku(interface = TasksBacklogRepository)]
pub struct TasksBacklogRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl TasksBacklogRepository for TasksBacklogRepositoryComponent {
    // Implementation delegated to repository impl
}

impl TasksBacklogRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// GeoRestrictionRepository component
#[derive(Component)]
#[shaku(interface = GeoRestrictionRepository)]
pub struct GeoRestrictionRepositoryComponent {
    #[shaku(inject)]
    pool: Arc<DatabasePool>,
}

impl GeoRestrictionRepository for GeoRestrictionRepositoryComponent {
    // Implementation delegated to repository impl
}

impl GeoRestrictionRepositoryComponent {
    pub fn create(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }
}

/// StorageRepository component using LocalStorage
#[derive(Component)]
#[shaku(interface = StorageRepository)]
pub struct StorageRepositoryComponent {
    /// Local storage implementation
    storage: Arc<LocalStorage>,
}

impl StorageRepository for StorageRepositoryComponent {
    // Implementation delegated to LocalStorage
}

impl StorageRepositoryComponent {
    pub fn create() -> Self {
        Self {
            storage: Arc::new(LocalStorage::new("./storage".to_string())),
        }
    }
}

/// TaskQueue component using PostgresTaskQueue
#[derive(Component)]
#[shaku(interface = TaskQueue)]
pub struct TaskQueueComponent {
    #[shaku(inject)]
    task_repo: Arc<dyn TaskRepository>,
}

impl TaskQueue for TaskQueueComponent {
    // Implementation delegated to PostgresTaskQueue
}

impl TaskQueueComponent {
    pub fn create(task_repo: Arc<dyn TaskRepository>) -> Self {
        Self { task_repo }
    }
}

/// Infrastructure module for Shaku DI
///
/// This module provides all infrastructure components including:
/// - Database connection pool
/// - Redis client
/// - All repository implementations
shaku::module! {
    pub InfrastructureModule {
        components = [
            DatabasePoolComponent,
            RedisClientComponent,
            TaskRepositoryComponent,
            CreditsRepositoryComponent,
            CrawlRepositoryComponent,
            ScrapeResultRepositoryComponent,
            WebhookRepositoryComponent,
            WebhookEventRepositoryComponent,
            TasksBacklogRepositoryComponent,
            GeoRestrictionRepositoryComponent,
            StorageRepositoryComponent,
            TaskQueueComponent,
        ],
        providers = []
    }
}
