// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Database connection pool implementation using dbnexus.
//!
//! This module provides a PostgreSQL connection pool wrapper that integrates
//! with the Shaku dependency injection framework and replaces the Sea-ORM
//! based implementation.

use crate::config::DatabaseSettings;
use dbnexus::{CacheConfig, DbConfig, DbPool, Session};
use sea_orm::{ConnAcquireErr, DbErr};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use log::{debug, info, warn};

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

    #[tokio::test]
    async fn test_create_pool() {
        // This test requires a running PostgreSQL instance
        // Skip in CI without database
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let settings = DatabaseSettings {
            url: "postgresql://postgres:postgres@localhost/crawlrs".to_string(),
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
    async fn test_get_session() {
        if std::env::var("SKIP_DATABASE_TESTS").is_ok() {
            return;
        }

        let settings = DatabaseSettings {
            url: "postgresql://postgres:postgres@localhost/crawlrs".to_string(),
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
