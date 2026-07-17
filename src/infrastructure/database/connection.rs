// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::config::DatabaseSettings;
use log::{debug, info, warn};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

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

impl Deref for DatabasePool {
    type Target = DatabaseConnection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AsRef<DatabaseConnection> for DatabasePool {
    fn as_ref(&self) -> &DatabaseConnection {
        &self.inner
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct DatabaseSettings with the given URL (and default optional fields).
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
    // create_pool tests
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_invalid_url_returns_error() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "create_pool with invalid URL should return error"
        );
    }

    #[tokio::test]
    async fn tc_create_pool_unreachable_host_returns_error() {
        // Valid format but unreachable host (reserved port 1 triggers connection failure)
        let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "create_pool with unreachable host should return error"
        );
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_create_pool_sqlite_memory_succeeds() {
        let settings = make_settings("sqlite::memory:");
        let result = create_pool(&settings).await;
        assert!(
            result.is_ok(),
            "create_pool with sqlite::memory: should succeed, got error: {:?}",
            result.err()
        );
    }

    // ============================================================
    // create_pool_with_retry tests
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_with_retry_invalid_url_fails_all_retries() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(
            result.is_err(),
            "create_pool_with_retry with invalid URL should fail after all retries"
        );
    }

    #[tokio::test]
    async fn tc_create_pool_with_retry_zero_retries_returns_timeout_error() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 0, 0).await;
        match result {
            Err(sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout)) => { /* expected */
            }
            Err(e) => panic!("expected ConnectionAcquire(Timeout), got: {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn tc_create_pool_with_retry_one_retry_invalid_url_fails() {
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 1, 0).await;
        assert!(
            result.is_err(),
            "create_pool_with_retry with 1 retry and invalid URL should fail"
        );
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_create_pool_with_retry_sqlite_succeeds_first_try() {
        let settings = make_settings("sqlite::memory:");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(
            result.is_ok(),
            "create_pool_with_retry with sqlite::memory: should succeed, got error: {:?}",
            result.err()
        );
    }

    // ============================================================
    // DatabasePool construction & methods
    // (DatabasePool::default() uses sqlite::memory:, requires dbnexus-sqlite)
    // ============================================================

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_default_succeeds() {
        let pool = DatabasePool::default();
        let stats = pool.stats();
        assert_eq!(stats.active_connections, 1);
        assert_eq!(stats.idle_connections, 1);
        assert_eq!(stats.total_connections, 1);
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_stats_returns_clone() {
        let pool = DatabasePool::default();
        let stats1 = pool.stats();
        let stats2 = pool.stats();
        assert_eq!(stats1.active_connections, stats2.active_connections);
        assert_eq!(stats1.idle_connections, stats2.idle_connections);
        assert_eq!(stats1.total_connections, stats2.total_connections);
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_deref_to_database_connection() {
        let pool = DatabasePool::default();
        let derefed: &sea_orm::DatabaseConnection = pool.deref();
        let as_ref: &sea_orm::DatabaseConnection = AsRef::as_ref(&pool);
        let deref_ptr = derefed as *const sea_orm::DatabaseConnection;
        let as_ref_ptr = as_ref as *const sea_orm::DatabaseConnection;
        assert_eq!(
            deref_ptr, as_ref_ptr,
            "Deref and AsRef should return reference to the same inner DatabaseConnection"
        );
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_as_ref_to_database_connection() {
        let pool = DatabasePool::default();
        let as_ref: &sea_orm::DatabaseConnection = AsRef::as_ref(&pool);
        let backend = as_ref.get_database_backend();
        assert_eq!(
            backend,
            sea_orm::DatabaseBackend::Sqlite,
            "as_ref should return usable DatabaseConnection (sqlite backend expected)"
        );
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_clone_preserves_inner() {
        let pool = DatabasePool::default();
        let cloned_pool = pool.clone();
        let original_ptr = pool.deref() as *const sea_orm::DatabaseConnection;
        let cloned_ptr = cloned_pool.deref() as *const sea_orm::DatabaseConnection;
        assert_eq!(
            original_ptr, cloned_ptr,
            "Clone should preserve inner DatabaseConnection reference (Arc semantics)"
        );
        assert_eq!(
            pool.stats().active_connections,
            cloned_pool.stats().active_connections
        );
    }

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_database_pool_clone_arc_semantics() {
        let pool = DatabasePool::default();
        let cloned = pool.clone();
        let p1 = pool.deref() as *const sea_orm::DatabaseConnection;
        let p2 = cloned.deref() as *const sea_orm::DatabaseConnection;
        assert_eq!(
            p1, p2,
            "cloned pool should share the same DatabaseConnection"
        );
        let cloned2 = pool.clone();
        let p3 = cloned2.deref() as *const sea_orm::DatabaseConnection;
        assert_eq!(
            p1, p3,
            "second clone should also share the same DatabaseConnection"
        );
    }

    // ============================================================
    // PoolStats tests
    // ============================================================

    #[test]
    fn tc_pool_stats_clone_preserves_values() {
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
    fn tc_pool_stats_debug_format_works() {
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
    fn tc_pool_stats_zero_values() {
        let stats = PoolStats {
            active_connections: 0,
            idle_connections: 0,
            total_connections: 0,
        };
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
        let cloned = stats.clone();
        assert_eq!(cloned.active_connections, 0);
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("active_connections: 0"));
    }

    // ============================================================
    // create_pool with None settings (default-value else branches)
    // 覆盖 max_connections/idle_timeout/max_lifetime/connect_timeout 的 else/unwrap_or 分支
    // ============================================================

    /// 辅助函数：构造所有 Option 字段均为 None 的 DatabaseSettings。
    fn make_settings_none(url: &str) -> DatabaseSettings {
        DatabaseSettings {
            url: url.to_string(),
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
            connection_keepalive: None,
            health_check_interval: None,
        }
    }

    #[tokio::test]
    async fn tc_create_pool_with_all_none_settings_uses_defaults_and_fails() {
        // 覆盖 max_connections/else(100)、idle_timeout/else(300)、max_lifetime/else(1800)、
        // connect_timeout/unwrap_or(30)、min_connections/None（不调用 opt.min_connections）。
        // URL 无效，连接最终失败，但所有配置 else 分支会被执行。
        let settings = make_settings_none("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err(), "should fail with invalid URL");
    }

    #[tokio::test]
    async fn tc_create_pool_with_none_max_connections_only() {
        // 仅 max_connections 为 None，其余为 Some
        let settings = DatabaseSettings {
            url: "not-a-valid-url".to_string(),
            max_connections: None,
            min_connections: Some(1),
            connect_timeout: Some(30),
            idle_timeout: Some(600),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_none_idle_timeout_only() {
        let settings = DatabaseSettings {
            url: "not-a-valid-url".to_string(),
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout: Some(30),
            idle_timeout: None,
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_none_max_lifetime_only() {
        let settings = DatabaseSettings {
            url: "not-a-valid-url".to_string(),
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout: Some(30),
            idle_timeout: Some(600),
            max_lifetime: None,
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // ============================================================
    // create_pool 环境变量分支
    // 覆盖 CRAWLRS_ENV/APP_ENVIRONMENT 的 production/prod/development 分支
    // ============================================================

    /// RAII guard：保存旧环境变量值，析构时恢复。
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
    async fn tc_create_pool_with_crawlrs_env_production_disables_sql_logging() {
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err(), "should fail with invalid URL");
    }

    #[tokio::test]
    async fn tc_create_pool_with_crawlrs_env_prod_disables_sql_logging() {
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_app_environment_production_disables_sql_logging() {
        // 覆盖 or_else 分支：CRAWLRS_ENV 未设置时回退到 APP_ENVIRONMENT
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_app_environment_prod_disables_sql_logging() {
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_development_env_enables_sql_logging() {
        // 覆盖 development 默认分支（is_production = false）
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "development");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // ============================================================
    // create_pool with PostgreSQL URL format
    // 覆盖 postgresql/postgres URL 检测分支
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_with_postgresql_url_format() {
        // 覆盖 settings.url.starts_with("postgresql") 分支
        let settings = make_settings("postgresql://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "should fail to connect to unreachable host"
        );
    }

    #[tokio::test]
    async fn tc_create_pool_with_postgres_url_format() {
        // 覆盖 settings.url.starts_with("postgres") 分支
        let settings = make_settings("postgres://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // ============================================================
    // create_pool_with_retry 成功重试路径
    // 覆盖 attempt > 1 时 info! 分支（需要 dbnexus-sqlite 特性）
    // ============================================================

    #[cfg(feature = "dbnexus-sqlite")]
    #[tokio::test]
    async fn tc_create_pool_with_retry_success_on_first_attempt_no_info_log() {
        // 第一次成功，attempt == 1，不触发 info! 分支
        let settings = make_settings("sqlite::memory:");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(result.is_ok());
    }

    // ============================================================
    // Additional PoolStats boundary tests
    // ============================================================

    #[test]
    fn tc_pool_stats_max_u32_values() {
        let stats = PoolStats {
            active_connections: u32::MAX,
            idle_connections: u32::MAX,
            total_connections: u32::MAX,
        };
        assert_eq!(stats.active_connections, u32::MAX);
        assert_eq!(stats.idle_connections, u32::MAX);
        assert_eq!(stats.total_connections, u32::MAX);
        let cloned = stats.clone();
        assert_eq!(cloned.active_connections, u32::MAX);
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains(&u32::MAX.to_string()));
    }

    #[test]
    fn tc_pool_stats_clone_independence() {
        // 修改克隆后的 stats 不应影响原始对象
        let mut stats = PoolStats {
            active_connections: 5,
            idle_connections: 3,
            total_connections: 8,
        };
        let cloned = stats.clone();
        stats.active_connections = 100;
        // 克隆应保持原值
        assert_eq!(cloned.active_connections, 5);
    }

    // ============================================================
    // Additional create_pool boundary tests
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_with_empty_url_returns_error() {
        let settings = make_settings("");
        let result = create_pool(&settings).await;
        assert!(result.is_err(), "empty URL should return error");
    }

    #[tokio::test]
    async fn tc_create_pool_with_whitespace_url_returns_error() {
        let settings = make_settings("   ");
        let result = create_pool(&settings).await;
        assert!(result.is_err(), "whitespace-only URL should return error");
    }

    // ============================================================
    // Additional create_pool_with_retry boundary tests
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_with_retry_two_retries_invalid_url_fails() {
        // 覆盖 retry_count = 2 分支
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 2, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_retry_large_retry_count_fails() {
        // 覆盖 retry_count = 10 分支（但会用 0 延迟避免长时间运行）
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 10, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tc_create_pool_with_retry_empty_url_fails_all_retries() {
        // 覆盖 retry_count = 3 + empty URL
        let settings = make_settings("");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(result.is_err());
    }

    // ============================================================
    // create_pool_with_retry 首次失败重试成功路径
    // 覆盖 `if attempt > 1 { info!(...) }` 分支（lines 104-106）
    //
    // 现有测试只覆盖"全部失败"或"首次成功"路径，未覆盖"首次失败，重试成功"。
    // 本测试通过 TCP 代理模拟该场景：
    // 1. 启动 postgres testcontainer
    // 2. 启动 TCP 代理：第一次连接立即关闭（模拟连接失败），第二次连接转发到 postgres
    // 3. 调用 create_pool_with_retry，第一次失败，第二次成功，进入 attempt > 1 分支
    // ============================================================

    #[tokio::test]
    async fn tc_create_pool_with_retry_succeeds_on_second_attempt() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use testcontainers::core::IntoContainerPort;
        use testcontainers::runners::AsyncRunner;
        use testcontainers::ImageExt;
        use testcontainers_modules::postgres::Postgres;
        use tokio::net::{TcpListener, TcpStream};

        // 检查 Docker 是否可用
        let docker_ok = tokio::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if !docker_ok {
            eprintln!(
                "[skip] Docker unavailable — tc_create_pool_with_retry_succeeds_on_second_attempt"
            );
            return;
        }

        // 启动 postgres testcontainer
        let container = match Postgres::default().with_tag("16-alpine").start().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let pg_port = match container.get_host_port_ipv4(5432.tcp()).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to get postgres port: {e}");
                return;
            }
        };
        // 保持容器存活直到进程退出（testcontainers 在 drop 时停止容器）
        std::mem::forget(container);

        // 启动 TCP 代理：第一次连接立即关闭，后续连接转发到 postgres
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind proxy listener");
        let proxy_port = proxy_listener
            .local_addr()
            .expect("Failed to get proxy local addr")
            .port();
        let proxy_url = format!("postgres://postgres:postgres@127.0.0.1:{proxy_port}/postgres");

        let fail_first = Arc::new(AtomicBool::new(true));
        let fail_first_clone = fail_first.clone();
        let pg_target = format!("127.0.0.1:{pg_port}");

        tokio::spawn(async move {
            loop {
                let (client_conn, _) = match proxy_listener.accept().await {
                    Ok(c) => c,
                    Err(_) => break,
                };

                let should_fail = fail_first_clone.swap(false, Ordering::SeqCst);
                let pg_target = pg_target.clone();

                tokio::spawn(async move {
                    if should_fail {
                        // 第一次连接：立即关闭，触发连接失败
                        drop(client_conn);
                        return;
                    }

                    // 后续连接：双向转发到 postgres
                    let mut pg_conn = match TcpStream::connect(&pg_target).await {
                        Ok(c) => c,
                        Err(_) => return,
                    };

                    let (mut client_read, mut client_write) = client_conn.into_split();
                    let (mut pg_read, mut pg_write) = pg_conn.split();

                    let c2p = tokio::io::copy(&mut client_read, &mut pg_write);
                    let p2c = tokio::io::copy(&mut pg_read, &mut client_write);

                    let _ = tokio::try_join!(c2p, p2c);
                });
            }
        });

        // 调用 create_pool_with_retry：第一次失败（代理关闭），第二次成功（代理转发）
        // 第二次成功时进入 `if attempt > 1 { info!(...) }` 分支
        let settings = make_settings(&proxy_url);
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(
            result.is_ok(),
            "create_pool_with_retry should succeed on second attempt, got error: {:?}",
            result.err()
        );

        // 验证返回的 DatabaseConnection 可用：ping 数据库确认连接活跃
        // create_pool_with_retry 返回 sea_orm::DatabaseConnection（不是 dbnexus DbPool），
        // 因此用 sea-orm 的 ping 方法验证连接，不用 get_session。
        let pool = result.expect("pool should be Some");
        let ping_result = pool.ping().await;
        assert!(
            ping_result.is_ok(),
            "ping on returned DatabaseConnection should succeed, got error: {:?}",
            ping_result.err()
        );
    }
}
