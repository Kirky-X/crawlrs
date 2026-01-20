// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure initialization: database, Redis, HTTP client, and repositories.

use crate::config::settings::Settings;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::database::connection;
use crate::infrastructure::repositories::{
    crawl_repo_impl::CrawlRepositoryImpl, credits_repo_impl::CreditsRepositoryImpl,
    database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
    scrape_result_repo_impl::ScrapeResultRepositoryImpl, task_repo_impl::TaskRepositoryImpl,
    tasks_backlog_repo_impl::TasksBacklogRepositoryImpl,
    webhook_event_repo_impl::WebhookEventRepoImpl, webhook_repo_impl::WebhookRepoImpl,
};
use anyhow::Result;
use migration::{Migrator, MigratorTrait};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Database connection pool type.
pub type DatabasePool = sea_orm::DbConn;

/// All repository instances used by the application.
#[derive(Clone)]
pub struct Repositories {
    /// Task repository for task queue operations.
    pub task_repo: Arc<TaskRepositoryImpl>,
    /// Result repository for scrape results.
    pub result_repo: Arc<ScrapeResultRepositoryImpl>,
    /// Crawl repository for crawl operations.
    pub crawl_repo: Arc<CrawlRepositoryImpl>,
    /// Webhook event repository for webhook processing.
    pub webhook_event_repo: Arc<WebhookEventRepoImpl>,
    /// Webhook repository for webhook management.
    pub webhook_repo: Arc<WebhookRepoImpl>,
    /// Credits repository for credit management.
    pub credits_repo: Arc<CreditsRepositoryImpl>,
    /// Geo restriction repository.
    pub geo_restriction_repo: Arc<DatabaseGeoRestrictionRepository>,
    /// Tasks backlog repository for backlog processing.
    pub tasks_backlog_repo: Arc<TasksBacklogRepositoryImpl>,
}

/// Initialize database connection pool.
///
/// This function creates a connection pool to the database and runs
/// all pending migrations.
///
/// # Arguments
///
/// * `settings` - Application settings containing database configuration
///
/// # Returns
///
/// Returns a connected database pool.
pub async fn init_database(settings: &Settings) -> Result<Arc<DatabasePool>> {
    let db = connection::create_pool(&settings.database).await?;
    let db = Arc::new(db);
    info!("Database connection established");

    info!("Running database migrations...");
    Migrator::up(db.as_ref(), None).await?;
    info!("Database migrations applied");

    Ok(db)
}

/// Initialize Redis client.
///
/// This function creates a Redis client connection based on the
/// configured Redis URL.
///
/// # Arguments
///
/// * `settings` - Application settings containing Redis configuration
///
/// # Returns
///
/// Returns a connected Redis client.
pub async fn init_redis(settings: &Settings) -> Result<Arc<RedisClient>> {
    let redis_client = Arc::new(RedisClient::new(settings.redis.url())?);
    info!("Redis client initialized");
    Ok(redis_client)
}

/// Initialize HTTP client.
///
/// This function creates a shared HTTP client with configurable timeout
/// and proxy settings. The client is used throughout the application for
/// making HTTP requests.
///
/// # Arguments
///
/// * `settings` - Application settings containing timeout and proxy configuration
///
/// # Returns
///
/// Returns a configured HTTP client wrapped in Arc for sharing.
pub fn init_http_client(settings: &Settings) -> Result<Arc<reqwest::Client>> {
    // Default timeout: 30 seconds
    let timeout_secs = settings.timeouts.engines.default_timeout_seconds;
    let timeout = Duration::from_secs(timeout_secs as u64);

    // Build client builder with timeout
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90));

    // Configure proxy if enabled
    if settings.proxy.enabled {
        let proxy_url = settings.proxy.url();
        match reqwest::Proxy::all(proxy_url) {
            Ok(proxy) => {
                client_builder = client_builder.proxy(proxy);
                info!("HTTP client configured with proxy (credentials hidden)");
            }
            Err(e) => {
                tracing::warn!("Invalid proxy URL '{}', disabling proxy: {}", proxy_url, e);
            }
        }
    }

    let client = client_builder.build()?;
    let client = Arc::new(client);

    info!("HTTP client initialized (timeout: {}s)", timeout_secs);
    Ok(client)
}

/// Initialize all application repositories.
///
/// This function creates instances of all repositories used by the
/// application and returns them in a [`Repositories`] struct.
///
/// # Arguments
///
/// * `db` - Database connection pool
/// * `settings` - Application settings for configuring repositories
///
/// # Returns
///
/// Returns a struct containing all initialized repositories.
pub fn init_repositories(db: Arc<DatabasePool>, settings: &Settings) -> Repositories {
    let task_repo = Arc::new(TaskRepositoryImpl::new(
        db.clone(),
        chrono::Duration::seconds(settings.concurrency.task_lock_duration_seconds),
    ));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db.clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db.clone()));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db.clone()));
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db.clone()));
    let tasks_backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db.clone()));

    Repositories {
        task_repo,
        result_repo,
        crawl_repo,
        webhook_event_repo,
        webhook_repo,
        credits_repo,
        geo_restriction_repo,
        tasks_backlog_repo,
    }
}

/// Initialize storage repository.
///
/// This function creates the appropriate storage repository based on
/// the configuration (e.g., S3, local filesystem).
///
/// # Arguments
///
/// * `settings` - Application settings for storage configuration
///
/// # Returns
///
/// Returns an optional storage repository.
pub fn init_storage_repository(
    settings: &Settings,
) -> Result<Option<Arc<dyn StorageRepository + Send + Sync>>> {
    match crate::infrastructure::storage::create_storage_repository(&settings.storage) {
        Ok(repo) => Ok(Some(Arc::from(repo))),
        Err(e) => {
            tracing::error!("Failed to initialize storage repository: {}", e);
            Err(anyhow::anyhow!(
                "Failed to initialize storage repository: {}",
                e
            ))
        }
    }
}

/// All infrastructure components initialized for the application.
#[derive(Clone)]
pub struct InfrastructureComponents {
    /// Database connection pool.
    pub db: Arc<DatabasePool>,
    /// Redis client.
    pub redis_client: Arc<RedisClient>,
    /// HTTP client.
    pub http_client: Arc<reqwest::Client>,
    /// All application repositories.
    pub repositories: Repositories,
    /// Optional storage repository.
    pub storage_repo: Option<Arc<dyn StorageRepository + Send + Sync>>,
}

/// Initialize all infrastructure components.
///
/// This is a convenience function that combines database, Redis, HTTP client,
/// and repository initialization.
///
/// # Arguments
///
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns all initialized infrastructure components.
pub async fn init_infrastructure(settings: &Settings) -> Result<InfrastructureComponents> {
    let db = init_database(settings).await?;
    let redis_client = init_redis(settings).await?;
    let http_client = init_http_client(settings)?;
    let repositories = init_repositories(db.clone(), settings);
    let storage_repo = init_storage_repository(settings)?;

    Ok(InfrastructureComponents {
        db,
        redis_client,
        http_client,
        repositories,
        storage_repo,
    })
}
