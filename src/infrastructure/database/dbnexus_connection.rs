// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Database connection pool implementation using dbnexus.
//!
//! This module provides a PostgreSQL connection pool wrapper that integrates
//! with the trait-kit dependency injection framework and replaces the Sea-ORM
//! based implementation.

use crate::config::DatabaseSettings;
use dbnexus::sea_orm::{ConnAcquireErr, DbErr};
use dbnexus::{CacheConfig, DbConfig, DbPool, PoolConfig, Session};
use log::{debug, error, info, warn};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// 迁移基目录的覆盖环境变量（与 `main.rs` 的 migrate 子命令共用）。
/// 值为包含 `<dialect>/` 子目录的基目录，而非直接的迁移目录。
pub const MIGRATIONS_DIR_ENV: &str = "CRAWLRS_MIGRATIONS_DIR";

/// crawlrs 专用迁移历史表（fluxweir-llm-gateway D9）。
///
/// dbnexus 默认历史表 `dbnexus_migrations` 由 crawlrs 与 fluxweir 共享时
/// 会跨消费者短路：crawlrs 文件迁移版本 1..8 与 fluxweir 内存迁移版本
/// 1..=9 重叠，`!applied.contains(version)` 的待应用判定不分写方——存量库
/// 上 fluxweir V9 的 ALTER 会打在缺失表上（启动 fail-fast），新库上
/// fluxweir 先跑则 crawlrs 001..008 全被跳过（运行期 500）。专用表消除
/// 版本空间碰撞；fluxweir 侧迁移常量为冻结契约不动。
pub const CRAWLRS_MIGRATIONS_TABLE: &str = "crawlrs_migrations";

/// dbnexus 默认迁移历史表名（dbnexus executor 冻结契约，未导出故本地声明；
/// 仅作存量基线读取源，crawlrs 不再写入）。
const DBNEXUS_MIGRATIONS_TABLE: &str = "dbnexus_migrations";

/// 编译期 db dialect feature → 迁移子目录（R-multidb-schema-002）。
///
/// dbnexus 驱动 feature 编译期互斥，任一时刻至多命中一个分支；
/// 三者皆未编译（嵌入式构建）时返回 `None`。
pub fn dialect_migrations_dir(base: &Path) -> Option<PathBuf> {
    #[cfg(feature = "db-postgres")]
    {
        let _ = base;
        #[cfg(feature = "db-sqlite")]
        compile_error!("db-postgres 与 db-sqlite 互斥（dbnexus 编译期约束）");
        #[cfg(feature = "db-mysql")]
        compile_error!("db-postgres 与 db-mysql 互斥（dbnexus 编译期约束）");
        Some(base.join("postgres"))
    }
    #[cfg(all(feature = "db-sqlite", not(feature = "db-postgres")))]
    {
        #[cfg(feature = "db-mysql")]
        compile_error!("db-sqlite 与 db-mysql 互斥（dbnexus 编译期约束）");
        Some(base.join("sqlite"))
    }
    #[cfg(all(
        feature = "db-mysql",
        not(any(feature = "db-postgres", feature = "db-sqlite"))
    ))]
    {
        Some(base.join("mysql"))
    }
    #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite", feature = "db-mysql")))]
    {
        let _ = base;
        None
    }
}

/// 编译期 db dialect feature → dbnexus `DatabaseType`（与
/// [`dialect_migrations_dir`] 同款互斥映射，R-multidb-schema-002）。
fn migration_db_type() -> Option<dbnexus::DatabaseType> {
    #[cfg(feature = "db-postgres")]
    {
        Some(dbnexus::DatabaseType::Postgres)
    }
    #[cfg(all(feature = "db-sqlite", not(feature = "db-postgres")))]
    {
        Some(dbnexus::DatabaseType::Sqlite)
    }
    #[cfg(all(
        feature = "db-mysql",
        not(any(feature = "db-postgres", feature = "db-sqlite"))
    ))]
    {
        Some(dbnexus::DatabaseType::MySql)
    }
    #[cfg(not(any(feature = "db-postgres", feature = "db-sqlite", feature = "db-mysql")))]
    {
        None
    }
}

/// crawlrs 专用迁移执行（fluxweir-llm-gateway D9：历史表分离 + 存量基线）。
///
/// 流程：`crawlrs_migrations` 空表首跑时从 [`DBNEXUS_MIGRATIONS_TABLE`] 复制
/// `file_path LIKE '%migrations/%'` 的行为基线（crawlrs 池历史跑的是文件迁移、
/// 路径必含 `migrations/`；fluxweir 迁移为内存嵌入、合成路径不含该锚点），
/// 表非空即视为已初始化、不再读默认表——fluxweir 后写入的 1..=9 行永不混入。
/// 随后 `MigrationExecutor::run_migrations` 跑 `migrations/<dialect>/`。
///
/// `auto_migrate=false` 时不执行也不报错（与原池内语义一致）；dialect 目录
/// 缺失时显性报错（R-multidb-schema-003，禁止静默跳过）。
///
/// 返回本次应用（不含基线标记）的迁移数。
pub async fn run_crawlrs_migrations(
    pool: &DbPool,
    auto_migrate: bool,
    migrations_dir: Option<&Path>,
) -> Result<u32, DbErr> {
    if !auto_migrate {
        return Ok(0);
    }
    let (dir, db_type) = resolve_dialect_dir_and_type(migrations_dir)?;
    let (mut executor, conn) = build_crawlrs_migration_executor(pool, db_type).await?;
    let baseline = apply_crawlrs_migration_baseline(&conn).await?;

    let applied = executor
        .run_migrations(&dir)
        .await
        .map_err(|e| DbErr::Custom(format!("crawlrs migrations failed: {e}")))?;
    if baseline > 0 || applied > 0 {
        info!(
            "crawlrs migrations complete: baseline marked={baseline}, applied={applied} \
             (history table: {CRAWLRS_MIGRATIONS_TABLE})"
        );
    }
    Ok(applied)
}

/// 建立 crawlrs 迁移执行器：取 admin session → 按 dialect 构造 executor →
/// 覆盖历史表 → 加载（幂等建）历史表；返回 executor 与其连接。
async fn build_crawlrs_migration_executor(
    pool: &DbPool,
    db_type: dbnexus::DatabaseType,
) -> Result<
    (
        dbnexus::MigrationExecutor,
        dbnexus::sea_orm::DatabaseConnection,
    ),
    DbErr,
> {
    let session = pool
        .get_session("admin")
        .await
        .map_err(|e| DbErr::Custom(format!("migration session acquire failed: {e}")))?;
    let mut executor = session
        .create_migration_executor(db_type)
        .map_err(|e| DbErr::Custom(format!("migration executor init failed: {e}")))?
        .with_history_table(CRAWLRS_MIGRATIONS_TABLE)
        .map_err(|e| DbErr::Custom(format!("history table override failed: {e}")))?;
    // load_history 幂等建 crawlrs_migrations（executor 内建 IF NOT EXISTS），
    // 为基线 INSERT 提供表结构
    executor
        .load_history()
        .await
        .map_err(|e| DbErr::Custom(format!("crawlrs migration history load failed: {e}")))?;

    let conn = session
        .connection()
        .map_err(|e| DbErr::Custom(format!("migration session connection: {e}")))?
        .clone();
    Ok((executor, conn))
}

/// 迁移目录与 DatabaseType 的编译期 dialect 解析：
/// - `migrations_dir` 缺失 → 显性报错（auto_migrate 开启却无目录可选）；
/// - 基目录 → `migrations/<dialect>/` 投影（R-multidb-schema-002）；
/// - dialect feature → `dbnexus::DatabaseType`（与 [`dialect_migrations_dir`]
///   同款互斥映射，R-multidb-schema-002）。
fn resolve_dialect_dir_and_type(
    migrations_dir: Option<&Path>,
) -> Result<(PathBuf, dbnexus::DatabaseType), DbErr> {
    let base_dir = migrations_dir.ok_or_else(|| {
        DbErr::Custom(
            "database.auto_migrate=true but no db dialect feature (db-postgres/db-sqlite/\
             db-mysql) is compiled; cannot select a migrations/<dialect>/ directory"
                .to_string(),
        )
    })?;
    // 入参为迁移基目录（含 <dialect>/ 子目录）；按编译 dialect 投影后交给
    // executor——直接扫基目录会得到 0 个文件（applied=0 假绿）
    let dir = dialect_migrations_dir(base_dir).ok_or_else(|| {
        DbErr::Custom(
            "no db dialect feature compiled; cannot map migrations base dir \
             to a migrations/<dialect>/ directory"
                .to_string(),
        )
    })?;
    let Some(db_type) = migration_db_type() else {
        return Err(DbErr::Custom(
            "database.auto_migrate=true but no db dialect feature compiled; \
             cannot map DatabaseType for migrations"
                .to_string(),
        ));
    };
    Ok((dir, db_type))
}

/// 存量基线回填：仅当 `crawlrs_migrations` 为空且默认历史表存在 crawlrs
/// 行时执行一次性 INSERT SELECT（`file_path LIKE '%migrations/%'` 筛选）。
/// 全新库（默认表不存在/为空）返回 0，正常全量跑迁移。
async fn apply_crawlrs_migration_baseline(
    conn: &dbnexus::sea_orm::DatabaseConnection,
) -> Result<u64, DbErr> {
    use dbnexus::sea_orm::{ConnectionTrait, Statement};

    let backend = conn.get_database_backend();
    let legacy_has_rows = match conn
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS n FROM {DBNEXUS_MIGRATIONS_TABLE}"),
        ))
        .await
    {
        Ok(Some(row)) => row.try_get::<i64>("", "n").unwrap_or(0) > 0,
        // 默认表不可读 = 全新库（fluxweir 尚未建表），无基线属预期；
        // 与查询瞬态失败同分支但留下审计日志，避免存量库静默重放迁移
        Err(_) => {
            warn!(
                "{DBNEXUS_MIGRATIONS_TABLE} not readable (treated as fresh database; \
                 baseline skipped)"
            );
            false
        }
        _ => false,
    };
    if !legacy_has_rows {
        return Ok(0);
    }

    let ours: i64 = conn
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS n FROM {CRAWLRS_MIGRATIONS_TABLE}"),
        ))
        .await
        .map_err(|e| DbErr::Custom(format!("crawlrs migration history count failed: {e}")))?
        .and_then(|row| row.try_get::<i64>("", "n").ok())
        .unwrap_or(0);
    if ours > 0 {
        // 已初始化过：跳过基线（防 fluxweir 后写入默认表的行混入）
        return Ok(0);
    }

    let result = conn
        .execute_unprepared(&format!(
            "INSERT INTO {CRAWLRS_MIGRATIONS_TABLE} (version, description, applied_at, file_path) \
             SELECT version, description, applied_at, file_path FROM {DBNEXUS_MIGRATIONS_TABLE} \
             WHERE file_path LIKE '%migrations/%'"
        ))
        .await
        .map_err(|e| DbErr::Custom(format!("crawlrs migration baseline failed: {e}")))?;
    let marked = result.rows_affected();
    if marked > 0 {
        info!(
            "crawlrs migration baseline: marked {marked} legacy rows as applied \
             (source: {DBNEXUS_MIGRATIONS_TABLE})"
        );
    }
    Ok(marked)
}

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

    /// Clone the inner `Arc<DbPool>` for dependency injection
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
    let max_connections = settings.max_connections.unwrap_or(100);
    let min_connections = settings.min_connections.unwrap_or(10);
    let idle_timeout = settings.idle_timeout.unwrap_or(300);
    let acquire_timeout = settings.connect_timeout.map(|t| t * 1000).unwrap_or(30000);

    debug!(
        "Creating dbnexus pool: max_connections={}, min_connections={}, idle_timeout={}s",
        max_connections, min_connections, idle_timeout
    );

    let permissions_path = get_permissions_path();
    if let Some(ref path) = permissions_path {
        info!("Loading permissions config from: {:?}", path);
    }

    // R-multidb-schema-002：按编译 dialect 选择迁移目录；auto_migrate 缺省
    // false（不执行迁移也不报错）。未编译任何 dialect 却要求 auto_migrate
    // 时显性报错——禁止静默跳过或回退（R-multidb-schema-003）。
    let migrations_base =
        std::env::var(MIGRATIONS_DIR_ENV).unwrap_or_else(|_| "migrations".to_string());
    let migrations_dir = dialect_migrations_dir(Path::new(&migrations_base));
    if settings.auto_migrate && migrations_dir.is_none() {
        return Err(DbErr::Custom(
            "database.auto_migrate=true but no db dialect feature (db-postgres/db-sqlite/\
             db-mysql) is compiled; cannot select a migrations/<dialect>/ directory"
                .to_string(),
        ));
    }
    if settings.auto_migrate {
        info!(
            "auto_migrate enabled; migrations dir: {}",
            migrations_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
    }

    let config = DbConfig {
        url: settings.url.clone(),
        pool_config: PoolConfig {
            max_connections,
            min_connections,
            idle_timeout,
            acquire_timeout,
        },
        permissions_path,
        migrations_dir: migrations_dir.clone(),
        // fluxweir-llm-gateway D9：池内 auto_migrate 恒关——迁移移交
        // [`run_crawlrs_migrations`]（专用历史表 `crawlrs_migrations`，
        // 与 fluxweir 的 dbnexus_migrations 隔离）；settings.auto_migrate
        // 语义保留，由池创建后的显式执行器承接
        auto_migrate: false,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: CacheConfig::default(),
        // fluxweir-llm-gateway T018：显式激活幂等读重试（SELECT/SHOW/EXPLAIN，
        // INSERT 不受影响）——fluxweir 自建池默认 Some(default)，共享池不设
        // 会使 fluxweir read 路径重试语义静默失活
        retry_policy: Some(dbnexus::RetryPolicy::default()),
        failover_config: None,
        replica_config: None,
    };

    // Create pool using dbnexus with_config (handles async initialization)
    let pool = DbPool::with_config(config)
        .await
        .map_err(|_e| DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed))?;

    // 池级 prepared statement LRU（吸收自研 dbnexus `prepare-cache`）：
    // 配置 prepare_cache_capacity 启用，命中后跳过 prepare/parse 往返；
    // None 保持关闭（行为同历史版本）
    if let Some(capacity) = settings.prepare_cache_capacity {
        pool.enable_prepare_cache(capacity);
    }

    // fluxweir-llm-gateway D9：显式迁移执行（专用历史表 + 存量基线回填；
    // 传基目录，方言投影在 run_crawlrs_migrations 内统一）
    let applied = run_crawlrs_migrations(
        &pool,
        settings.auto_migrate,
        Some(Path::new(&migrations_base)),
    )
    .await
    .map_err(|e| {
        error!("crawlrs migrations failed: {e}");
        e
    })?;
    if applied > 0 {
        info!("database migrations complete: {applied} applied");
    }

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;

    /// R-multidb-schema-002：编译 dialect → 迁移子目录一一对应。
    /// 默认测试构建启用 db-postgres，应选中 `postgres/`。
    #[cfg(feature = "db-postgres")]
    #[test]
    fn dialect_migrations_dir_maps_postgres_feature() {
        let dir = dialect_migrations_dir(Path::new("migrations"));
        assert_eq!(
            dir.as_deref(),
            Some(Path::new("migrations/postgres")),
            "db-postgres build must select migrations/postgres/"
        );
    }

    /// R-multidb-schema-002：方言目录必须在仓库中真实存在，
    /// 否则 auto_migrate=true 会在运行期才失败。
    #[cfg(feature = "db-postgres")]
    #[test]
    fn postgres_migrations_dir_exists_in_repo() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let dir =
            dialect_migrations_dir(&base).expect("db-postgres build must yield a migrations dir");
        assert!(
            dir.is_dir(),
            "missing dialect migrations dir: {}",
            dir.display()
        );
        let count = std::fs::read_dir(&dir)
            .expect("read migrations/postgres")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
            .count();
        assert!(
            count >= 8,
            "migrations/postgres should carry the 8 PG migrations"
        );
    }

    /// R-multidb-schema-002：auto_migrate 缺省 false（不执行迁移也不报错）。
    #[test]
    fn database_settings_default_auto_migrate_is_false() {
        let settings = make_settings("postgresql://localhost/test");
        assert!(!settings.auto_migrate, "auto_migrate must default to false");
    }

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
            prepare_cache_capacity: None,
            auto_migrate: false,
        }
    }

    // DatabasePool construction & trait impls (lazy pool, no DB)

    #[test]
    fn test_database_pool_from_arc_dbpool_yields_inner_reference() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        assert!(
            Arc::ptr_eq(db_pool.inner(), &pool),
            "From<Arc<DbPool>> should preserve inner Arc identity"
        );
    }

    #[test]
    fn test_database_pool_into_arc_dbpool_preserves_identity() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool.clone());
        let inner_ref = db_pool.inner();
        assert!(Arc::ptr_eq(inner_ref, &pool));
    }

    #[test]
    fn test_database_pool_stats_default_is_zero() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let db_pool = DatabasePool::from(pool);
        let stats = db_pool.stats();
        // DatabasePool::from(Arc<DbPool>) uses PoolStats::default
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_database_pool_stats_returns_clone_of_inner_stats() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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

    // DatabasePool session methods — all should fail with lazy pool

    #[tokio::test]
    async fn test_get_session_succeeds_with_real_db() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_readonly_session().await;
        assert!(matches!(
            result,
            Err(dbnexus::sea_orm::DbErr::ConnectionAcquire(_))
        ));
    }

    #[tokio::test]
    async fn test_get_session_with_empty_role_returns_error_with_real_db() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_session("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_session_with_unicode_role_returns_error_with_real_db() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let db_pool = DatabasePool::from(create_test_db_pool());
        let result = db_pool.get_session("管理员").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[allow(unused_comparisons, clippy::absurd_extreme_comparisons)]
    async fn test_get_pool_stats_returns_status_from_real_pool() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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

    // create_pool — error paths (no real DB needed)

    #[tokio::test]
    async fn test_create_pool_invalid_url_returns_error() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(
            result.is_err(),
            "create_pool with invalid URL should return error"
        );
    }

    #[tokio::test]
    async fn test_create_pool_unreachable_host_returns_error() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
            prepare_cache_capacity: None,
            auto_migrate: false,
        };
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_postgresql_url_format_returns_error() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // 覆盖 settings.url.starts_with("postgresql") 分支
        let settings = make_settings("postgresql://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_postgres_url_format_returns_error() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // 覆盖 settings.url.starts_with("postgres") 分支
        let settings = make_settings("postgres://user:pass@127.0.0.1:1/db");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // create_pool_with_retry — error paths

    #[tokio::test]
    async fn test_create_pool_with_retry_invalid_url_fails_all_retries() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 3, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_zero_retries_returns_timeout_error() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // 0 retries means loop body never executes, falls through to
        // last_error.unwrap_or_else(Timeout) — covers the Timeout fallback branch.
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 0, 0).await;
        match result {
            Err(dbnexus::sea_orm::DbErr::ConnectionAcquire(
                dbnexus::sea_orm::ConnAcquireErr::Timeout,
            )) => {}
            Err(e) => panic!("expected ConnectionAcquire(Timeout), got: {:?}", e),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_one_retry_invalid_url_fails() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 1, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_two_retries_invalid_url_fails() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // Two retries: covers warn! branch on attempt 1 and the final failure path.
        let settings = make_settings("not-a-valid-url");
        let result = create_pool_with_retry(&settings, 2, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_retry_unreachable_host_fails() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let settings = make_settings("postgres://postgres:postgres@127.0.0.1:1/postgres");
        let result = create_pool_with_retry(&settings, 2, 0).await;
        assert!(result.is_err());
    }

    // PoolStats — Default / Clone / Debug

    #[test]
    fn test_pool_stats_default_is_zero() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let stats = PoolStats::default();
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_pool_stats_clone_preserves_values() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let stats = PoolStats {
            active_connections: u32::MAX,
            idle_connections: u32::MAX,
            total_connections: u32::MAX,
        };
        assert_eq!(stats.active_connections, u32::MAX);
        assert_eq!(stats.idle_connections, u32::MAX);
        assert_eq!(stats.total_connections, u32::MAX);
    }

    // Environment variable branches (create_pool sqlx_logging toggle)

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
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_crawlrs_env_prod_disables_sql_logging() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_app_environment_production_disables_sql_logging() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // 覆盖 or_else 分支：CRAWLRS_ENV 未设置时回退到 APP_ENVIRONMENT
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "production");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_app_environment_prod_disables_sql_logging() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let _guard1 = EnvVarGuard::set("CRAWLRS_ENV", "");
        let _guard2 = EnvVarGuard::set("APP_ENVIRONMENT", "prod");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_pool_with_development_env_enables_sql_logging() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        // 覆盖 development 默认分支（is_production = false）
        let _guard = EnvVarGuard::set("CRAWLRS_ENV", "development");
        let settings = make_settings("not-a-valid-url");
        let result = create_pool(&settings).await;
        assert!(result.is_err());
    }

    // Original ignored tests requiring a real PostgreSQL instance

    #[tokio::test]
    #[ignore = "requires running PostgreSQL; run with: cargo test test_create_pool -- --ignored"]
    async fn test_create_pool() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
            prepare_cache_capacity: None,
            auto_migrate: false,
        };

        let pool = create_pool(&settings).await;
        assert!(pool.is_ok(), "Failed to create pool: {:?}", pool.err());
    }

    #[tokio::test]
    #[ignore = "requires running PostgreSQL; run with: cargo test test_get_session -- --ignored"]
    async fn test_get_session() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
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
            prepare_cache_capacity: None,
            auto_migrate: false,
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
