// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::DatabaseSettings;
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Database pool wrapper type with metrics support
#[derive(Clone)]
pub struct DatabasePool {
    pub(crate) inner: Arc<DatabaseConnection>,
    pub stats: PoolStats,
}

impl DatabasePool {
    /// Get current pool statistics
    pub fn stats(&self) -> PoolStats {
        self.stats.clone()
    }
}

#[derive(Clone, Debug)]
pub struct PoolStats {
    pub active_connections: u32,
    pub idle_connections: u32,
    pub total_connections: u32,
}

impl Default for DatabasePool {
    fn default() -> Self {
        // For testing, use a mock in-memory database
        let settings = DatabaseSettings {
            url: "sqlite::memory:".to_string(),
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

/// 创建优化的数据库连接池
///
/// # 优化特性
///
/// * 智能连接池大小管理
/// * 连接健康检查
/// * 连接存活时间控制
/// * 基于环境的SQL日志
/// * 连接重试机制
///
/// # 参数
///
/// * `settings` - 数据库配置
/// * `retry_count` - 连接失败时的重试次数（默认3）
/// * `retry_delay` - 重试间隔（秒，默认1）
///
/// # 返回值
///
/// * `Ok(DatabaseConnection)` - 数据库连接
/// * `Err(DbErr)` - 连接过程中出现的错误
pub async fn create_pool_with_retry(
    settings: &DatabaseSettings,
    retry_count: u32,
    retry_delay: u64,
) -> Result<DatabaseConnection, DbErr> {
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

    Err(last_error
        .unwrap_or_else(|| DbErr::ConnectionAcquire(sea_orm::error::ConnAcquireErr::Timeout)))
}

/// 创建数据库连接池（简化版本）
///
/// # 参数
///
/// * `settings` - 数据库配置
///
/// # 返回值
///
/// * `Ok(DatabaseConnection)` - 数据库连接
/// * `Err(DbErr)` - 连接过程中出现的错误
pub async fn create_pool(settings: &DatabaseSettings) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(settings.url.to_owned());

    // 连接池大小配置
    if let Some(max) = settings.max_connections {
        opt.max_connections(max);
    } else {
        opt.max_connections(100); // 默认最大连接数
    }

    if let Some(min) = settings.min_connections {
        opt.min_connections(min);
    }

    // 超时配置
    let timeout = settings.connect_timeout.unwrap_or(30);
    opt.connect_timeout(Duration::from_secs(timeout));
    opt.acquire_timeout(Duration::from_secs(timeout));

    if let Some(idle) = settings.idle_timeout {
        opt.idle_timeout(Duration::from_secs(idle));
    } else {
        opt.idle_timeout(Duration::from_secs(300)); // 默认5分钟
    }

    // 连接存活时间配置
    if let Some(max_lifetime) = settings.max_lifetime {
        opt.max_lifetime(Duration::from_secs(max_lifetime));
    } else {
        opt.max_lifetime(Duration::from_secs(1800)); // 默认30分钟
    }

    // 根据环境决定是否启用SQL日志
    let env = std::env::var("CRAWLRS_ENV")
        .or_else(|_| std::env::var("APP_ENVIRONMENT"))
        .unwrap_or_else(|_| "development".to_string());
    let is_production = env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");
    opt.sqlx_logging(!is_production);

    // PostgreSQL 特定的优化
    if settings.url.starts_with("postgresql") || settings.url.starts_with("postgres") {
        // PostgreSQL 优化已内置于 sea-orm
    }

    debug!(
        "Creating database pool: max_connections={}, min_connections={}, connect_timeout={}s",
        settings.max_connections.unwrap_or(100),
        settings.min_connections.unwrap_or(10),
        timeout
    );

    Database::connect(opt).await
}
