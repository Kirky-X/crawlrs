// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure initialization: database, Redis, HTTP client, and repositories.

use crate::config::settings::Settings;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;
use crate::infrastructure::oxcache::{create_cache, SearchCache};
use crate::infrastructure::repositories::{
    crawl_repo_impl::CrawlRepositoryImpl, credits_repo_impl::CreditsRepositoryImpl,
    database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
    scrape_result_repo_impl::ScrapeResultRepositoryImpl, task_repo_impl::TaskRepositoryImpl,
    tasks_backlog_repo_impl::TasksBacklogRepositoryImpl,
    webhook_event_repo_impl::WebhookEventRepoImpl, webhook_repo_impl::WebhookRepoImpl,
};
use anyhow::Result;
use log::info;
use std::sync::Arc;
use std::time::Duration;

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
    use crate::infrastructure::database::dbnexus_connection::create_pool;

    let pool = create_pool(&settings.database).await?;
    let db = DatabasePool {
        inner: Arc::new(pool),
        stats: Default::default(),
    };
    let db = Arc::new(db);
    info!("Database connection established");

    Ok(db)
}

/// Initialize Redis client.
///
/// This function creates a Redis client connection based on the
/// configured Redis URL and connection pool settings.
///
/// # Arguments
///
/// * `settings` - Application settings containing Redis configuration
///
/// # Returns
///
/// Returns a connected Redis client with connection pool.
pub async fn init_redis(settings: &Settings) -> Result<Arc<RedisClient>> {
    let redis_client = Arc::new(RedisClient::from_settings(&settings.redis)?);
    info!(
        "Redis client initialized with connection pool (max: {}, connection_timeout: {}s, recycle_timeout: {}s)",
        settings.redis.max_connections(),
        settings.redis.connection_timeout(),
        settings.redis.idle_timeout()
    );
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
    let timeout = Duration::from_secs(timeout_secs);

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
                log::warn!("Invalid proxy URL '{}', disabling proxy: {}", proxy_url, e);
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
        db.inner().clone(),
        chrono::Duration::seconds(settings.concurrency.task_lock_duration_seconds),
    ));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.inner().clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.inner().clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db.inner().clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db.inner().clone()));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db.inner().clone()));
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db.inner().clone()));
    let tasks_backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db.inner().clone()));

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
            log::error!("Failed to initialize storage repository: {}", e);
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
    /// Redis client (used for complex operations like rate limiting with Lua scripts).
    pub redis_client: Arc<RedisClient>,
    /// OxCache instance for simple caching scenarios (search results, DNS, regex).
    pub oxcache: Option<Arc<SearchCache>>,
    /// HTTP client.
    pub http_client: Arc<reqwest::Client>,
    /// All application repositories.
    pub repositories: Repositories,
    /// Optional storage repository.
    pub storage_repo: Option<Arc<dyn StorageRepository + Send + Sync>>,
}

/// Initialize oxcache for simple caching scenarios.
///
/// This function creates an oxcache instance for caching search results,
/// DNS lookups, and regex patterns. For complex rate limiting and
/// distributed semaphore operations, RedisClient is still required.
///
/// # Arguments
///
/// * `settings` - Application settings containing cache configuration
///
/// # Returns
///
/// Returns an initialized oxcache instance wrapped in Arc.
pub async fn init_oxcache(settings: &Settings) -> Result<Option<Arc<SearchCache>>> {
    if !settings.cache.enabled {
        info!("Cache is disabled, skipping oxcache initialization");
        return Ok(None);
    }

    match create_cache(&settings.cache).await {
        Ok(cache) => {
            info!(
                "OxCache initialized (capacity: {}, ttl: {}s)",
                settings.cache.memory.capacity, settings.cache.memory.ttl_seconds
            );
            Ok(Some(cache))
        }
        Err(e) => {
            log::warn!(
                "Failed to initialize oxcache: {}. Cache will be disabled.",
                e
            );
            Ok(None)
        }
    }
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
    let oxcache = init_oxcache(settings).await?;

    Ok(InfrastructureComponents {
        db,
        redis_client,
        oxcache,
        http_client,
        repositories,
        storage_repo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== init_http_client tests ==========

    #[test]
    fn test_init_http_client_returns_ok_with_default_settings() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let result = init_http_client(&settings);
        assert!(
            result.is_ok(),
            "init_http_client should succeed with default settings"
        );
    }

    #[test]
    fn test_init_http_client_returns_arc_client() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let client = init_http_client(&settings).expect("Should create HTTP client");
        // Verify the client is usable (can build a request without sending)
        let _req = client.get("http://localhost");
    }

    #[test]
    fn test_init_http_client_with_proxy_disabled() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.proxy.enabled = false;
        let result = init_http_client(&settings);
        assert!(
            result.is_ok(),
            "init_http_client should succeed with proxy disabled"
        );
    }

    #[test]
    fn test_init_http_client_with_invalid_proxy_url_does_not_panic() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.proxy.enabled = true;
        // Set an invalid proxy URL to test error handling path
        settings.proxy.url = "not-a-valid-url".to_string();
        // init_http_client should handle the invalid proxy gracefully (warn + continue)
        let result = init_http_client(&settings);
        assert!(
            result.is_ok(),
            "init_http_client should succeed even with invalid proxy URL (just disables proxy)"
        );
    }

    #[test]
    fn test_init_http_client_with_valid_proxy_url() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.proxy.enabled = true;
        settings.proxy.url = "http://localhost:10808".to_string();
        let result = init_http_client(&settings);
        assert!(
            result.is_ok(),
            "init_http_client should succeed with a valid proxy URL"
        );
    }

    // ========== init_oxcache tests ==========

    #[tokio::test]
    async fn test_init_oxcache_returns_ok_when_cache_disabled() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.cache.enabled = false;
        let result = init_oxcache(&settings).await;
        assert!(
            result.is_ok(),
            "init_oxcache should return Ok when cache is disabled"
        );
        let cache = result.expect("init_oxcache should succeed");
        assert!(
            cache.is_none(),
            "oxcache should be None when cache is disabled"
        );
    }

    // ========== init_storage_repository tests ==========

    #[test]
    fn test_init_storage_repository_with_local_storage() {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        // Force local storage type (no external S3 dependency)
        settings.storage.storage_type = "local".to_string();
        let result = init_storage_repository(&settings);
        assert!(
            result.is_ok(),
            "init_storage_repository should succeed with local storage type, got err: {:?}",
            result.err()
        );
    }
}
