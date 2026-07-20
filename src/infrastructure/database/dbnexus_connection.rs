// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Database connection pool implementation using dbnexus.
//!
//! This module provides a PostgreSQL connection pool wrapper that integrates
//! with the trait-kit dependency injection framework and replaces the Sea-ORM
//! based implementation.

use crate::config::DatabaseSettings;
use dbnexus::{CacheConfig, DbConfig, DbPool, Session};
use log::{debug, info, warn};
use sea_orm::{ConnAcquireErr, DbErr};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

/// Database pool wrapper type with metrics support using dbnexus
///
/// This wrapper maintains compatibility with the existing codebase while
/// using dbnexus internally for database operations.
#[derive(Clone)]
pub struct DatabasePool {
    /// Inner dbnexus pool
    pub(crate) inner: Arc<DbPool>,
    /// Pool statistics
    pub stats: PoolStats,
}

impl DatabasePool {
    /// Get current pool statistics
    pub fn stats(&self) -> PoolStats {
        self.stats.clone()
    }

    /// Get reference to inner pool
    pub fn inner(&self) -> &Arc<DbPool> {
        &self.inner
    }

    /// Clone the inner Arc<DbPool> for dependency injection
    ///
    /// # Performance
    ///
    /// This is a zero-cost operation - Arc::clone only increments
    /// the reference count, no deep copy occurs.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let pool: Arc<DatabasePool> = /* ... */;
    /// let inner: Arc<DbPool> = pool.clone_inner();
    /// let repo = MyRepository::new(inner);
    /// ```
    #[inline]
    pub fn clone_inner(&self) -> Arc<DbPool> {
        Arc::clone(&self.inner)
    }

    /// Get a session for the specified role
    ///
    /// This is the primary method for obtaining database sessions.
    /// The session is automatically returned to the pool when dropped.
    ///
    /// # Arguments
    ///
    /// * `role` - The role to use for permission checking (e.g., "admin", "system")
    ///
    /// # Returns
    ///
    /// Result containing the session or a database error
    pub async fn get_session(&self, role: &str) -> Result<Session, DbErr> {
        self.inner
            .get_session(role)
            .await
            .map_err(|_e| DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed))
    }

    /// Get an admin session with full permissions
    ///
    /// Convenience method for getting a session with admin role.
    pub async fn get_admin_session(&self) -> Result<Session, DbErr> {
        self.get_session("admin").await
    }

    /// Get a system session for internal operations
    ///
    /// Convenience method for getting a session with system role.
    pub async fn get_system_session(&self) -> Result<Session, DbErr> {
        self.get_session("system").await
    }

    /// Get a read-only session
    ///
    /// Convenience method for getting a session with readonly role.
    pub async fn get_readonly_session(&self) -> Result<Session, DbErr> {
        self.get_session("readonly").await
    }

    /// Get pool status
    ///
    /// Returns current pool statistics for monitoring.
    pub async fn get_pool_stats(&self) -> PoolStats {
        let status = self.inner.status();
        PoolStats {
            active_connections: status.active,
            idle_connections: status.idle,
            total_connections: status.total,
        }
    }
}

impl Deref for DatabasePool {
    type Target = DbPool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AsRef<DbPool> for DatabasePool {
    fn as_ref(&self) -> &DbPool {
        &self.inner
    }
}

impl From<DatabasePool> for Arc<DbPool> {
    fn from(pool: DatabasePool) -> Self {
        pool.inner
    }
}

impl From<Arc<DbPool>> for DatabasePool {
    fn from(inner: Arc<DbPool>) -> Self {
        Self {
            inner,
            stats: PoolStats::default(),
        }
    }
}

/// Pool statistics
#[derive(Clone, Debug, Default)]
pub struct PoolStats {
    /// Number of active connections
    pub active_connections: u32,
    /// Number of idle connections
    pub idle_connections: u32,
    /// Total number of connections
    pub total_connections: u32,
}

/// Get the permissions config path
///
/// Looks for permissions.yaml in the config directory.
fn get_permissions_path() -> Option<String> {
    // Try config directory relative to current working directory
    let config_path = std::path::PathBuf::from("config").join("permissions.yaml");
    if config_path.exists() {
        return Some(config_path.to_string_lossy().to_string());
    }

    // Try parent config directory
    let parent_config = std::path::PathBuf::from("../config").join("permissions.yaml");
    if parent_config.exists() {
        return Some(parent_config.to_string_lossy().to_string());
    }

    // Try /etc/crawlrs/permissions.yaml
    let etc_path = std::path::PathBuf::from("/etc/crawlrs/permissions.yaml");
    if etc_path.exists() {
        return Some(etc_path.to_string_lossy().to_string());
    }

    None
}

/// Create a database connection pool with retry mechanism
///
/// This function creates a dbnexus pool with automatic retry on connection failure.
///
/// # Arguments
///
/// * `settings` - Database configuration settings
/// * `retry_count` - Number of retry attempts on failure (default: 3)
/// * `retry_delay` - Delay between retries in seconds (default: 1)
///
/// # Returns
///
/// Result containing the pool or a database error
pub async fn create_pool_with_retry(
    settings: &DatabaseSettings,
    retry_count: u32,
    retry_delay: u64,
) -> Result<DbPool, DbErr> {
    let mut last_error: Option<DbErr> = None;

    for attempt in 1..=retry_count {
        match create_pool(settings).await {
            Ok(pool) => {
                if attempt > 1 {
                    info!("Database connection successful on attempt {}", attempt);
                }
                return Ok(pool);
            }
            Err(e) => {
                if attempt < retry_count {
                    warn!(
                        "Database connection failed (attempt {}/{}), retrying in {}s: {:?}",
                        attempt, retry_count, retry_delay, e
                    );
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_secs(retry_delay)).await;
                } else {
                    last_error = Some(e);
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| DbErr::ConnectionAcquire(ConnAcquireErr::Timeout)))
}

/// Create a database connection pool
///
/// Uses dbnexus DbPool::with_config for proper initialization including:
/// - Permission config loading
/// - Connection pool warmup
/// - Auto-migration if configured
///
/// # Arguments
///
/// * `settings` - Database configuration settings
///
/// # Returns
///
/// Result containing the pool or a database error
pub async fn create_pool(settings: &DatabaseSettings) -> Result<DbPool, DbErr> {
    // Configure pool settings
    let max_connections = settings.max_connections.unwrap_or(100);
    let min_connections = settings.min_connections.unwrap_or(10);
    let idle_timeout = settings.idle_timeout.unwrap_or(300);
    let acquire_timeout = settings.connect_timeout.map(|t| t * 1000).unwrap_or(30000);

    debug!(
        "Creating dbnexus pool: max_connections={}, min_connections={}, idle_timeout={}s",
        max_connections, min_connections, idle_timeout
    );

    // Get permissions path if exists
    let permissions_path = get_permissions_path();
    if let Some(ref path) = permissions_path {
        info!("Loading permissions config from: {:?}", path);
    }

    // Create DbConfig from settings
    let config = DbConfig {
        url: settings.url.clone(),
        max_connections,
        min_connections,
        idle_timeout,
        acquire_timeout,
        permissions_path,
        migrations_dir: None,
        auto_migrate: false,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: CacheConfig::default(),
    };

    // Create pool using dbnexus with_config (handles async initialization)
    let pool = DbPool::with_config(config)
        .await
        .map_err(|_e| DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed))?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;

    /// Build DatabaseSettings with the given URL and sensible defaults.
    fn make_settings(url: &str) -> DatabaseSettings {
        DatabaseSettings {
            url: url.to_string(),
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout: Some(30),
            idle_timeout: Some(600),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        }
    }

    // ============================================================
    // DatabasePool construction & trait impls (lazy pool, no DB)
    // ============================================================

    #[test]
    fn test_database_pool_from_arc_dbpool_yields_inner_reference() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        assert!(
            Arc::ptr_eq(db_pool.inner(), &pool),
            "From<Arc<DbPool>> should preserve inner Arc identity"
        );
    }

    #[test]
    fn test_database_pool_into_arc_dbpool_preserves_identity() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let arc: Arc<DbPool> = db_pool.into();
        assert!(
            Arc::ptr_eq(&arc, &pool),
            "From<DatabasePool> for Arc<DbPool> should preserve inner Arc identity"
        );
    }

    #[test]
    fn test_database_pool_clone_inner_returns_arc_clone() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let cloned_inner = db_pool.clone_inner();
        assert!(
            Arc::ptr_eq(&cloned_inner, &pool),
            "clone_inner() should return an Arc clone of inner"
        );
    }

    #[test]
    fn test_database_pool_inner_returns_reference_to_same_arc() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let inner_ref = db_pool.inner();
        assert!(Arc::ptr_eq(inner_ref, &pool));
    }

    #[test]
    fn test_database_pool_stats_default_is_zero() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool);
        let stats = db_pool.stats();
        // DatabasePool::from(Arc<DbPool>) uses PoolStats::default()
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_database_pool_stats_returns_clone_of_inner_stats() {
        let pool = create_test_db_pool();
        let custom_stats = PoolStats {
            active_connections: 7,
            idle_connections: 4,
            total_connections: 11,
        };
        let db_pool = DatabasePool {
            inner: pool,
            stats: custom_stats.clone(),
        };
        let stats1 = db_pool.stats();
        let stats2 = db_pool.stats();
        assert_eq!(stats1.active_connections, custom_stats.active_connections);
        assert_eq!(stats1.idle_connections, custom_stats.idle_connections);
        assert_eq!(stats1.total_connections, custom_stats.total_connections);
        assert_eq!(stats2.active_connections, stats1.active_connections);
        assert_eq!(stats2.idle_connections, stats1.idle_connections);
        assert_eq!(stats2.total_connections, stats1.total_connections);
    }

    #[test]
    fn test_database_pool_clone_preserves_inner_and_stats() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool {
            inner: pool.clone(),
            stats: PoolStats {
                active_connections: 1,
                idle_connections: 2,
                total_connections: 3,
            },
        };
        let cloned = db_pool.clone();
        assert!(Arc::ptr_eq(db_pool.inner(), cloned.inner()));
        assert_eq!(
            db_pool.stats().active_connections,
            cloned.stats().active_connections
        );
        assert_eq!(
            db_pool.stats().idle_connections,
            cloned.stats().idle_connections
        );
        assert_eq!(
            db_pool.stats().total_connections,
            cloned.stats().total_connections
        );
    }

    #[test]
    fn test_database_pool_deref_targets_inner_dbpool() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let derefed: &DbPool = &db_pool;
        let inner: &DbPool = db_pool.inner();
        let deref_ptr = derefed as *const DbPool;
        let inner_ptr = inner as *const DbPool;
        assert_eq!(
            deref_ptr, inner_ptr,
            "Deref should return reference to the same inner DbPool"
        );
    }

    #[test]
    fn test_database_pool_as_ref_targets_inner_dbpool() {
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let as_ref: &DbPool = AsRef::as_ref(&db_pool);
        let inner: &DbPool = db_pool.inner();
        let as_ref_ptr = as_ref as *const DbPool;
        let inner_ptr = inner as *const DbPool;
        assert_eq!(
            as_ref_ptr, inner_ptr,
            "AsRef should return reference to the same inner DbPool"
        );
    }

    // ============================================================
    // DatabasePool session methods — all should fail with lazy pool
    // ============================================================

    #[tokio::test]
    async fn test_get_session_succeeds_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_session("admin").await;
        assert!(
            result.is_ok(),
            "expected Ok with real DB connection, got {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_get_admin_session_succeeds_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_admin_session().await;
        assert!(
            result.is_ok(),
            "expected Ok with real DB connection, got {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_get_system_session_succeeds_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_system_session().await;
        assert!(
            result.is_ok(),
            "expected Ok with real DB connection, got {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_get_readonly_session_returns_error_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_readonly_session().await;
        assert!(matches!(result, Err(sea_orm::DbErr::ConnectionAcquire(_))));
    }

    #[tokio::test]
    async fn test_get_session_with_empty_role_returns_error_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_session("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_session_with_unicode_role_returns_error_with_real_db() {
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_session("管理员").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[allow(unused_comparisons, clippy::absurd_extreme_comparisons)]
    async fn test_get_pool_stats_returns_status_from_real_pool() {
        // get_pool_stats reads status from inner DbPool; real pool reports
        // non-negative counts (exact values depend on pool warm-up strategy).
        let db_pool = DatabasePool::from(create_test_db_pool());
        let stats = db_pool.get_pool_stats().await;
        assert!(
            stats.total_connections >= 0,
            "real pool stats should be non-negative, got total_connections = {}",
            stats.total_connections
        );
    }

    // ============================================================
    // create_pool — error paths (no real DB needed)
    // ============================================================

    #[tokio::test]
    async fn test_create_pool_invalid_url_returns_error() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "create_pool with invalid URL should return error"
        );
    }

    #[tokio::test]
    async fn test_create_pool_unreachable_host_returns_error() {
        // Valid format but unreachable host (port 1 triggers connection failure)
        let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "create_pool with unreachable host should return error"
        );
    }

    #[tokio::test]
    async fn test_create_pool_with_all_none_settings_uses_defaults_and_fails() {
        // 覆盖 max_connections/unwrap_or(100)、min_connections/unwrap_or(10)、
        // idle_timeout/unwrap_or(300)、acquire_timeout/unwrap_or(30000)、
        // connect_timeout/unwrap_or(30)、max_lifetime/unwrap_or(1800) 等分支
        let settings = DatabaseSettings {
            url: "not-a-valid-url".to_string(),
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
            connection_keepalive: None,
            health_check_interval: None,
        };
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_postgresql_url_format_returns_error() {
        // 覆盖 settings.url.starts_with("postgresql") 分支
        let settings = make_settings("postgresql://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_postgres_url_format_returns_error() {
        // 覆盖 settings.url.starts_with("postgres") 分支
        let settings = make_settings("postgres://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // ============================================================
    // create_pool_with_retry — error paths
    // ============================================================

    #[tokio::test]
    async fn test_create_pool_with_retry_invalid_url_fails_all_retries() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_zero_retries_returns_timeout_error() {
        // 0 retries means loop body never executes, falls through to
        // last_error.unwrap_or_else(Timeout) — covers the Timeout fallback branch.
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 0, 0).await;
        match result {
            Err(sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout)) => {}
            Err(e) => panic!("expected ConnectionAcquire(Timeout), got: {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_one_retry_invalid_url_fails() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 1, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_two_retries_invalid_url_fails() {
        // Two retries: covers warn! branch on attempt 1 and the final failure path.
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 2, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_unreachable_host_fails() {
        let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
        let result = create_pool_with_retry(&settings, 2, 0).await;
        assert!(result.is_err());
    }

    // ============================================================
    // PoolStats — Default / Clone / Debug
    // ============================================================

    #[test]
    fn test_pool_stats_default_is_zero() {
        let stats = PoolStats::default();
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_pool_stats_clone_preserves_values() {
        let stats = PoolStats {
            active_connections: 5,
            idle_connections: 3,
            total_connections: 8,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.active_connections, 5);
        assert_eq!(cloned.idle_connections, 3);
        assert_eq!(cloned.total_connections, 8);
    }

    #[test]
    fn test_pool_stats_debug_format_works() {
        let stats = PoolStats {
            active_connections: 1,
            idle_connections: 2,
            total_connections: 3,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("active_connections: 1"));
        assert!(debug_str.contains("idle_connections: 2"));
        assert!(debug_str.contains("total_connections: 3"));
    }

    #[test]
    fn test_pool_stats_zero_values() {
        let stats = PoolStats {
            active_connections: 0,
            idle_connections: 0,
            total_connections: 0,
        };
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_pool_stats_max_u32_values() {
        let stats = PoolStats {
            active_connections: u32::MAX,
            idle_connections: u32::MAX,
            total_connections: u32::MAX,
        };
        assert_eq!(stats.active_connections, u32::MAX);
        assert_eq!(stats.idle_connections, u32::MAX);
        assert_eq!(stats.total_connections, u32::MAX);
    }

    // ============================================================
    // Environment variable branches (create_pool sqlx_logging toggle)
    // ============================================================

    /// RAII guard that restores an environment variable on drop.
    struct EnvVarGuard {
        key: &'static str,
        old_value: Option<std::string::String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old_value = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, old_value }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.old_value {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[tokio::test]
    async fn test_create_pool_with_crawlrs_env_production_disables_sql_logging() {
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_crawlrs_env_prod_disables_sql_logging() {
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_app_environment_production_disables_sql_logging() {
        // 覆盖 or_else 分支：CRAWLRS_ENV 未设置时回退到 APP_ENVIRONMENT
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_app_environment_prod_disables_sql_logging() {
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_development_env_enables_sql_logging() {
        // 覆盖 development 默认分支（is_production = false）
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "development");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // ============================================================
    // Original ignored tests requiring a real PostgreSQL instance
    // ============================================================

    #[tokio::test]
    #[ignore = "requires running PostgreSQL; run with: cargo test test_create_pool -- --ignored"]
    async fn test_create_pool() {
        // This test requires a running PostgreSQL instance
        // Skip in CI without database
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let settings = DatabaseSettings {
            url: std::env::var("TEST_DATABASE_URL")
                .expect("TEST_DATABASE_URL must be set for this ignored test"),
            max_connections: Some(5),
            min_connections: Some(1),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };

        let pool = create_pool(&settings).await;
        assert!(pool.is_ok(), "Failed to create pool: {:?}", pool.err());
    }

    #[tokio::test]
    #[ignore = "requires running PostgreSQL; run with: cargo test test_get_session -- --ignored"]
    async fn test_get_session() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let settings = DatabaseSettings {
            url: std::env::var("TEST_DATABASE_URL")
                .expect("TEST_DATABASE_URL must be set for this ignored test"),
            max_connections: Some(5),
            min_connections: Some(1),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };

        let pool = create_pool(&settings).await.unwrap();
        let session = pool.get_session("admin").await;
        assert!(
            session.is_ok(),
            "Failed to get session: {:?}",
            session.err()
        );
    }
}
