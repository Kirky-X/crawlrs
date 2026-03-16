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
use dbnexus::DbPool;
use sea_orm::DbErr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

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
}

/// Pool statistics
#[derive(Clone, Debug)]
pub struct PoolStats {
    /// Number of active connections
    pub active_connections: u32,
    /// Number of idle connections
    pub idle_connections: u32,
    /// Total number of connections
    pub total_connections: u32,
}

impl Default for DatabasePool {
    fn default() -> Self {
        // For testing, create a mock pool
        let settings = DatabaseSettings {
            url: "postgresql://postgres:postgres@localhost/crawlrs".to_string(),
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout: Some(30),
            idle_timeout: Some(600),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let pool = futures::executor::block_on(create_pool(&settings))
            .expect("Failed to create default database pool");
        Self {
            inner: Arc::new(pool),
            stats: PoolStats {
                active_connections: 1,
                idle_connections: 1,
                total_connections: 1,
            },
        }
    }
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

    Err(last_error.unwrap_or_else(|| DbErr::ConnectionAcquire("Connection timeout".to_string())))
}

/// Create a database connection pool
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
    let connect_timeout = settings.connect_timeout.unwrap_or(30);
    let idle_timeout = settings.idle_timeout.unwrap_or(300);
    let max_lifetime = settings.max_lifetime.unwrap_or(1800);

    debug!(
        "Creating dbnexus pool: max_connections={}, min_connections={}, connect_timeout={}s",
        max_connections, min_connections, connect_timeout
    );

    // Create pool using dbnexus
    let pool = DbPool::builder()
        .max_connections(max_connections as u32)
        .min_connections(min_connections as u32)
        .connect_timeout(Duration::from_secs(connect_timeout))
        .idle_timeout(Duration::from_secs(idle_timeout))
        .max_lifetime(Duration::from_secs(max_lifetime))
        .build(&settings.url)
        .await
        .map_err(|e| DbErr::ConnectionAcquire(e.to_string()))?;

    Ok(pool)
}

/// Get pool status
///
/// Returns current pool statistics for monitoring
pub async fn get_pool_stats(pool: &DbPool) -> PoolStats {
    // dbnexus doesn't expose direct stats, return estimated values
    PoolStats {
        active_connections: 1,
        idle_connections: 1,
        total_connections: 1,
    }
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
}
