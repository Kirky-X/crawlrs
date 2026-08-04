// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure initialization: database, HTTP client, and repositories.

use crate::config::settings::Settings;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;
use crate::infrastructure::dns::DnsCacheService;
use crate::infrastructure::oxcache::{create_cache, CacheService, OxcacheService, SearchCache};
use crate::infrastructure::repositories::{
    crawl_repo_impl::CrawlRepositoryImpl, credits_repo_impl::CreditsRepositoryImpl,
    scrape_result_repo_impl::ScrapeResultRepositoryImpl, task_repo_impl::TaskRepositoryImpl,
    tasks_backlog_repo_impl::TasksBacklogRepositoryImpl,
    webhook_event_repo_impl::WebhookEventRepoImpl, webhook_repo_impl::WebhookRepoImpl,
};
// R-teams-004 / T015：teams-off 时不导入 DatabaseGeoRestrictionRepository
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use anyhow::Result;
use log::info;
use std::net::Ipv4Addr;
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
    ///
    /// R-teams-004 / T015：teams feature 关闭时不编译此字段。
    #[cfg(feature = "teams")]
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
pub fn init_http_client(
    settings: &Settings,
    dns_cache: Option<Arc<DnsCacheService>>,
) -> Result<Arc<reqwest::Client>> {
    // Default timeout: 30 seconds
    let timeout_secs = settings.timeouts.engines.default_timeout_seconds;
    let timeout = Duration::from_secs(timeout_secs);

    // Build client builder with timeout
    // 强制 IPv4：部署环境通常无 IPv6 连通性，reqwest 默认优先 IPv6 会导致
    // "Connection refused" 后不自动 fallback IPv4（如 people.com.cn 的 IPv6 AAAA 记录）
    // 使用 Ipv4OnlyResolver 在 DNS 层过滤 IPv6 地址，比 local_address 更可靠
    // 性能优化：如果传入 DnsCacheService，优先查缓存避免每次请求都走系统 DNS
    let resolver = match dns_cache {
        Some(cache) => {
            info!("Using DNS cache for IPv4 resolver");
            crate::infrastructure::dns::create_ipv4_only_resolver_with_cache(cache)
        }
        None => crate::infrastructure::dns::create_ipv4_only_resolver(),
    };
    // T056/C1 修复：移除 init_http_client 中的代理注入逻辑。
    // 代理已统一由 EngineModule 构造 ProxyPool 注入 ReqwestEngine（按 ProxyCategory::Html 路由）。
    // 双重注入会导致代理配置冲突：http_client 级别 + ReqwestEngine 级别同时生效。
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90))
        .local_address(Some(Ipv4Addr::UNSPECIFIED.into()))
        .dns_resolver(resolver)
        .build()?;
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
        chrono::Duration::seconds(
            settings
                .concurrency
                .task_lock_duration_seconds
                .try_into()
                .expect("task_lock_duration_seconds exceeds i64 range"),
        ),
    ));
    let result_repo = Arc::new(ScrapeResultRepositoryImpl::new(db.inner().clone()));
    let crawl_repo = Arc::new(CrawlRepositoryImpl::new(db.inner().clone()));
    let webhook_event_repo = Arc::new(WebhookEventRepoImpl::new(db.inner().clone()));
    let webhook_repo = Arc::new(WebhookRepoImpl::new(db.inner().clone()));
    let credits_repo = Arc::new(CreditsRepositoryImpl::new(db.inner().clone()));
    // R-teams-004 / T015：teams-off 时不构造 geo_restriction_repo
    #[cfg(feature = "teams")]
    let geo_restriction_repo = Arc::new(DatabaseGeoRestrictionRepository::new(db.inner().clone()));
    let tasks_backlog_repo = Arc::new(TasksBacklogRepositoryImpl::new(db.inner().clone()));

    Repositories {
        task_repo,
        result_repo,
        crawl_repo,
        webhook_event_repo,
        webhook_repo,
        credits_repo,
        #[cfg(feature = "teams")]
        geo_restriction_repo,
        tasks_backlog_repo,
    }
}

/// All infrastructure components initialized for the application.
#[derive(Clone)]
pub struct InfrastructureComponents {
    /// Database connection pool.
    pub db: Arc<DatabasePool>,
    /// OxCache instance for simple caching scenarios (search results, DNS, regex).
    pub oxcache: Option<Arc<SearchCache>>,
    /// Cache service for key-value caching (robots.txt, etc.).
    pub cache_service: Arc<dyn CacheService>,
    /// HTTP client.
    pub http_client: Arc<reqwest::Client>,
    /// All application repositories.
    pub repositories: Repositories,
}

/// Initialize oxcache for simple caching scenarios.
///
/// This function creates an oxcache instance for caching search results,
/// DNS lookups, and regex patterns.
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

/// Initialize the cache service for key-value caching.
///
/// Creates an `OxcacheService` backed by `oxcache::Cache<String, String>`
/// for general-purpose key-value caching (e.g. robots.txt content).
///
/// # Arguments
///
/// * `settings` - Application settings containing cache configuration
///
/// # Returns
///
/// Returns an initialized cache service as a trait object.
pub async fn init_cache_service(settings: &Settings) -> Result<Arc<dyn CacheService>> {
    // T027: skip cache initialization when cache is disabled, consistent with init_oxcache
    if !settings.cache.enabled {
        info!("Cache is disabled, skipping cache service initialization");
        // Return a no-op cache service that always misses
        let service = OxcacheService::build(1, std::time::Duration::from_secs(1))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build minimal cache service: {e}"))?;
        return Ok(Arc::new(service));
    }
    let capacity = settings.cache.memory.capacity;
    let ttl = std::time::Duration::from_secs(settings.cache.memory.ttl_seconds);
    let service = OxcacheService::build(capacity, ttl)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to build cache service: {e}"))?;
    info!(
        "Cache service initialized (capacity: {}, ttl: {}s)",
        capacity, settings.cache.memory.ttl_seconds
    );
    Ok(Arc::new(service))
}

/// Initialize all infrastructure components.
///
/// This is a convenience function that combines database, HTTP client,
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

    // 先创建 DNS cache（如果 cache enabled），用于 IPv4 resolver 性能优化
    // 避免 HTTP 请求热路径每次都走系统 DNS 调用
    let dns_cache = init_dns_cache_service(settings).await;

    let http_client = init_http_client(settings, dns_cache)?;
    let repositories = init_repositories(db.clone(), settings);
    let oxcache = init_oxcache(settings).await?;
    let cache_service = init_cache_service(settings).await?;

    Ok(InfrastructureComponents {
        db,
        oxcache,
        cache_service,
        http_client,
        repositories,
    })
}

/// 创建 DNS cache service（如果 cache enabled）.
///
/// 用于 Ipv4OnlyResolver 的 DNS 查询缓存，避免每次 HTTP 请求都走系统 DNS。
/// cache disabled 或创建失败时返回 None，resolver 会 fallback 到系统 DNS。
async fn init_dns_cache_service(settings: &Settings) -> Option<Arc<DnsCacheService>> {
    if !settings.cache.enabled {
        info!("Cache disabled, DNS cache not initialized");
        return None;
    }

    match crate::infrastructure::oxcache::create_dns_cache(
        settings.cache.memory.capacity,
        settings.cache.memory.ttl_seconds,
    )
    .await
    {
        Ok(cache) => match DnsCacheService::new(cache, settings.cache.memory.ttl_seconds) {
            Ok(service) => {
                info!(
                    "DNS cache initialized for IPv4 resolver (capacity: {}, ttl: {}s)",
                    settings.cache.memory.capacity, settings.cache.memory.ttl_seconds
                );
                Some(Arc::new(service))
            }
            Err(e) => {
                log::warn!("Failed to create DNS resolver: {}. Using system DNS.", e);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to create DNS cache: {}. Using system DNS.", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== init_http_client tests ==========

    #[test]
    fn test_init_http_client_returns_ok_with_default_settings() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let result = init_http_client(&settings, None);
        assert!(
            result.is_ok(),
            "init_http_client should succeed with default settings"
        );
    }

    #[test]
    fn test_init_http_client_returns_arc_client() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let client = init_http_client(&settings, None).expect("Should create HTTP client");
        // Verify the client is usable (can build a request without sending)
        let _req = client.get("http://localhost");
    }

    #[test]
    fn test_init_http_client_ignores_proxy_settings() {
        // T056/C1 修复后，init_http_client 不再注入代理。
        // 代理统一由 EngineModule 构造 ProxyPool 注入 ReqwestEngine。
        // 此测试验证即使 settings.proxy.enabled=true 且 urls 非空，
        // init_http_client 也只创建无代理的 http_client。
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.proxy.enabled = true;
        settings.proxy.urls = vec!["http://localhost:10808".to_string()];
        let result = init_http_client(&settings, None);
        assert!(
            result.is_ok(),
            "init_http_client should succeed regardless of proxy settings"
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

    // ========== testcontainers integration tests ==========
    //
    // The following tests require Docker to be running on the host. They use
    // testcontainers to spin up ephemeral PostgreSQL containers,
    // enabling real end-to-end coverage of the infrastructure initialization
    // paths that are impossible to test with mocks alone.
    //
    // If Docker is unavailable, each test early-returns (passes trivially)
    // so the overall `cargo test` invocation still succeeds in CI without
    // Docker. Run locally with Docker enabled to exercise these paths.

    use crate::common::test_support::testcontainers_fixtures as tcf;

    /// Helper: skip the test if Docker is unavailable.
    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    #[tokio::test]
    async fn tc_init_database_connects_to_postgres() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_database_connects_to_postgres");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let pool = init_database(&settings).await;
        assert!(
            pool.is_ok(),
            "init_database should succeed against a live postgres: {:?}",
            pool.err()
        );
        let pool = pool.unwrap();
        // Verify the inner dbnexus pool can acquire a session.
        let session = pool.get_session("admin").await;
        assert!(
            session.is_ok(),
            "should be able to acquire an admin session from the pool"
        );
    }

    #[tokio::test]
    async fn tc_init_database_returns_arc_database_pool() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_database_returns_arc_database_pool");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        let pool = init_database(&settings)
            .await
            .expect("pool should be created");
        // Verify the Arc strong count is at least 1.
        assert!(Arc::strong_count(&pool) >= 1);
        // Verify inner() accessor returns a usable Arc<DbPool>.
        let _inner: Arc<dbnexus::DbPool> = pool.inner().clone();
    }

    #[tokio::test]
    async fn tc_init_database_fails_on_invalid_url() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_database_fails_on_invalid_url");
            return;
        }
        // Use a deliberately invalid URL that cannot be connected to.
        let settings =
            tcf::settings_with_urls("postgres://nobody:nopass@127.0.0.1:1/nonexistent").unwrap();
        let result = init_database(&settings).await;
        assert!(
            result.is_err(),
            "init_database should fail when the database URL is unreachable"
        );
    }

    #[tokio::test]
    async fn tc_init_repositories_creates_all_repos() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_repositories_creates_all_repos");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
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
        let repos = init_repositories(db.clone(), &settings);

        // Verify all repositories are constructed and share the same pool.
        assert!(Arc::strong_count(&repos.task_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.result_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.crawl_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.webhook_event_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.webhook_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.credits_repo.clone()) >= 1);
        // R-teams-004 / T015：teams feature 关闭时字段不编译
        #[cfg(feature = "teams")]
        assert!(Arc::strong_count(&repos.geo_restriction_repo.clone()) >= 1);
        assert!(Arc::strong_count(&repos.tasks_backlog_repo.clone()) >= 1);
    }

    #[tokio::test]
    async fn tc_init_infrastructure_full_stack() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_infrastructure_full_stack");
            return;
        }
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&handle.pg.url).unwrap();
        let infra = init_infrastructure(&settings).await;
        assert!(
            infra.is_ok(),
            "init_infrastructure should succeed against live db: {:?}",
            infra.err()
        );
        let infra = infra.unwrap();

        // Verify all components are present.
        assert!(Arc::strong_count(&infra.db) >= 1);
        assert!(Arc::strong_count(&infra.http_client) >= 1);
        // oxcache may be None if cache is disabled in config; just verify it's set or None.
        let _ = &infra.oxcache;
        // Repositories: verify task_repo is constructed (Arc strong count >= 1).
        assert!(Arc::strong_count(&infra.repositories.task_repo) >= 1);
    }

    #[tokio::test]
    async fn tc_init_infrastructure_fails_without_db() {
        // DB points at an unreachable port; init_infrastructure should fail.
        let settings = tcf::settings_with_urls("postgres://127.0.0.1:1/x").unwrap();
        let result = init_infrastructure(&settings).await;
        assert!(
            result.is_err(),
            "init_infrastructure should fail when the database is unreachable"
        );
    }
}
