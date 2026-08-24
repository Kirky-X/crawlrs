// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Centralized test helpers for `src/` internal `#[cfg(test)] mod tests` blocks.
//!
//! This module is only compiled under `#[cfg(test)]` and provides shared
//! utilities that would otherwise be duplicated across 16+ `src/` modules.
//!
//! ## Database resolution order
//!
//! 1. `TEST_DATABASE_URL` environment variable (preferred)
//! 2. `DATABASE_URL` environment variable (CI convention)
//! 3. **Automatic testcontainers PostgreSQL** — when neither env var is set
//!    and Docker is available, a PostgreSQL 16-alpine container is started
//!    automatically with the project's SQL migrations applied. The container
//!    is shared process-wide and stopped on exit.

#![cfg(test)]

use std::sync::Arc;
use std::sync::OnceLock;

use dbnexus::{DbConfig, DbPool};
use tokio::sync::Mutex;

use testcontainers::core::IntoContainerPort;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres;

// ---------------------------------------------------------------------------
// Testcontainers PostgreSQL — process-wide shared container
// ---------------------------------------------------------------------------

/// Holds the running testcontainers PostgreSQL instance.
///
/// Stored in a `OnceLock` so the container lives for the entire test process
/// and is shared across all tests that need a database.
static TC_CONTAINER: OnceLock<testcontainers::ContainerAsync<Postgres>> = OnceLock::new();

/// Cached database URL from the testcontainers container.
static TC_URL: OnceLock<String> = OnceLock::new();

/// Check whether Docker is available on the host.
fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Start a testcontainers PostgreSQL, run SQL migrations, and return the URL.
///
/// Called at most once per process (cached via `OnceLock`).
fn start_testcontainers_pg() -> String {
    // Use a dedicated thread with its own tokio runtime to avoid conflicts
    // with the test runner's runtime.
    std::thread::scope(|s| {
        let handle = s.spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for testcontainers");
            rt.block_on(async {
                // Start PostgreSQL 16-alpine container
                let image = Postgres::default().with_tag("16-alpine");
                let container = image
                    .start()
                    .await
                    .expect("failed to start testcontainers PostgreSQL");
                let port = container
                    .get_host_port_ipv4(5432.tcp())
                    .await
                    .expect("failed to get postgres port");
                let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

                // Connect and run SQL migrations
                use sea_orm::{ConnectOptions, ConnectionTrait, Database};
                let mut opt = ConnectOptions::new(&url);
                opt.sqlx_logging(false);
                let conn = Database::connect(opt)
                    .await
                    .expect("failed to connect to testcontainers PostgreSQL for migration");

                let migration_files = [
                    "migrations/001_initial_schema.sql",
                    "migrations/002_add_crawl_url_fields.sql",
                    "migrations/003_add_acquire_next_indexes.sql",
                    "migrations/004_drop_feature_flags.sql",
                    "migrations/005_deprecate_legacy_api_keys.sql",
                ];
                for file in &migration_files {
                    let sql = std::fs::read_to_string(file)
                        .unwrap_or_else(|e| panic!("failed to read migration {file}: {e}"));
                    conn.execute_unprepared(&sql)
                        .await
                        .unwrap_or_else(|e| panic!("failed to run migration {file}: {e}"));
                }

                // Store container handle (kept alive for process lifetime)
                let _ = TC_CONTAINER.set(container);

                eprintln!(
                    "[testcontainers] PostgreSQL started at 127.0.0.1:{} with migrations applied",
                    port
                );
                url
            })
        });
        handle.join().expect("testcontainers thread panicked")
    })
}

/// Ensure the testcontainers URL is available, starting the container if needed.
///
/// Returns `None` if Docker is not available or container startup fails.
fn ensure_testcontainers_url() -> Option<&'static str> {
    if !docker_available() {
        return None;
    }
    TC_URL.get_or_init(start_testcontainers_pg);
    TC_URL.get().map(|s| s.as_str())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve the test database URL from the environment or testcontainers.
///
/// Resolution order:
/// 1. `TEST_DATABASE_URL` environment variable (preferred)
/// 2. `DATABASE_URL` environment variable (CI convention)
/// 3. Automatic testcontainers PostgreSQL (when Docker is available)
///
/// Returns `None` only when no database source is available (no env var AND
/// no Docker), signaling the caller to skip the test.
pub fn resolve_test_database_url() -> Option<String> {
    // 1. Try environment variables first (fast path, no container needed)
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return Some(url);
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return Some(url);
    }
    // 2. Fall back to testcontainers (starts container on first call)
    ensure_testcontainers_url().map(|s| s.to_string())
}

/// Returns `true` when no test database is available and the caller should
/// skip execution. Prints a `[skip]` notice to stderr so skipped tests are
/// visible in CI logs.
///
/// A database is considered available when either:
/// - `TEST_DATABASE_URL` / `DATABASE_URL` is set, OR
/// - Docker is available (testcontainers will auto-start PostgreSQL)
pub fn skip_if_no_test_db() -> bool {
    if resolve_test_database_url().is_none() {
        eprintln!(
            "[skip] No test database available (TEST_DATABASE_URL not set and Docker unavailable)"
        );
        return true;
    }
    false
}

/// Build a real `DbPool` against the resolved test database URL.
///
/// The URL is resolved in the following order:
/// 1. `TEST_DATABASE_URL` / `DATABASE_URL` environment variables
/// 2. Automatic testcontainers PostgreSQL (when Docker is available)
///
/// All repositories in this project consistently use the `admin` role
/// (see `src/infrastructure/database/repositories/*.rs`), which dbnexus
/// grants full access without requiring `permissions.yaml`. Tests therefore
/// do not load `permissions.yaml`, avoiding YAML/JSON parsing differences
/// in dbnexus 0.4.0.
///
/// # Panics
///
/// Panics if no database source is available (no env var AND no Docker),
/// or if pool construction fails.
pub fn create_test_db_pool() -> Arc<DbPool> {
    // 池内连接的 IO 绑定构造时所在 runtime 的 driver。此前每次构造都在 scoped
    // 临时 current_thread runtime 中进行，block_on 返回后 driver 随 runtime 销毁，
    // 池中已建立的连接在测试自身的 runtime 上 acquire 必然超时（表现为
    // "Connection pool timed out"）。改为进程级保活的 multi-thread runtime：
    // driver 与进程同生命周期；block_on 在独立线程上执行（禁止在异步上下文内
    // 阻塞当前线程驱动另一 runtime）。
    static POOL_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let rt = POOL_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build persistent runtime for DbPool construction")
    });
    let url = resolve_test_database_url()
        .expect("No test database available: set TEST_DATABASE_URL or ensure Docker is running");
    std::thread::scope(|s| {
        let handle = s.spawn(move || {
            let _guard = rt.enter();
            rt.block_on(async {
                let cfg = DbConfig {
                    url,
                    ..Default::default()
                };
                DbPool::with_config(cfg).await
            })
            .expect("failed to create DbPool for test")
        });
        Arc::new(handle.join().expect("DbPool construction thread panicked"))
    })
}

/// 全局 mutex 用于序列化所有 `acquire_next` 相关测试。
///
/// `acquire_next` 获取任何 `queued` task（不按 team_id 过滤），共享测试数据库
/// 以及并行测试会导致测试间相互干扰：一个测试创建的 task 可能被另一个测试的
/// `acquire_next` 获取，导致返回 `None`（flaky test）。此 mutex 确保同一时间
/// 只有一个 `acquire_next` 测试在运行，消除竞争条件。
///
/// 用法：
/// ```ignore
/// #[tokio::test]
/// async fn test_acquire_next() {
///     let _guard = acquire_next_test_mutex().lock().await;
///     // 测试逻辑
/// }
/// ```
pub fn acquire_next_test_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 持有 garrison 全局态锁的 RAII guard。
///
/// T034 修复：`init_services` 调用 `set_garrison_dao` + `set_audit_service`
/// 注入全局态，若其他测试已注入会导致 fail-fast panic。此 guard 在 setup
/// 阶段持有两把 `tokio::sync::Mutex` 锁并 reset 全局态，确保 `init_services` 调用安全。
///
/// # 用法
///
/// ```ignore
/// #[tokio::test]
/// async fn test_init_services() {
///     let _guard = acquire_garrison_global_state().await;
///     // 安全调用 init_services / build_test_state / ServiceModule
/// }
/// ```
///
/// # 锁顺序
///
/// 先获取 DAO 锁，再获取 AUDIT_SERVICE 锁。两把锁都是 `tokio::sync::Mutex`，
/// async-aware，安全跨 await 持有（避免 `std::sync::Mutex` 阻塞 runtime 线程）。
///
/// # cfg 门控（架构审查 M1 修复）
///
/// auth feature 关闭时，`garrison_dao`/`garrison_listener` 模块不编译。
/// 此 guard 与 [`acquire_garrison_global_state`] 在 auth-off 时退化为 no-op
/// （[`NoopGarrisonGuard`]），调用方 `let _guard = acquire_garrison_global_state().await;`
/// 在两种 feature 组合下都能编译，避免散布的 `#[cfg(feature = "auth")]`。
#[cfg(feature = "auth")]
pub struct GarrisonGlobalStateGuard {
    _dao_guard: tokio::sync::MutexGuard<'static, ()>,
    _audit_guard: tokio::sync::MutexGuard<'static, ()>,
}

/// auth-off 时的 no-op guard（架构审查 M1 修复）。
///
/// 当 `auth` feature 关闭时，`garrison_dao`/`garrison_listener` 模块不编译，
/// 不存在需要保护的全局态。此空 struct 让调用方代码在两种 feature 下都能编译。
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
pub struct NoopGarrisonGuard;

/// 获取 garrison 全局态锁并 reset DAO + AUDIT_SERVICE。
///
/// 返回 [`GarrisonGlobalStateGuard`]，调用方持有 guard 直到测试结束。
///
/// auth feature 关闭时退化为 no-op（返回 [`NoopGarrisonGuard`]），见上文 cfg 门控说明。
#[cfg(feature = "auth")]
pub async fn acquire_garrison_global_state() -> GarrisonGlobalStateGuard {
    let dao_guard = crate::infrastructure::auth::garrison_dao::test_mutex()
        .lock()
        .await;
    crate::infrastructure::auth::garrison_dao::reset_garrison_dao_for_test();
    let audit_guard = crate::infrastructure::auth::garrison_listener::test_mutex()
        .lock()
        .await;
    crate::infrastructure::auth::garrison_listener::reset_audit_service_for_test();
    GarrisonGlobalStateGuard {
        _dao_guard: dao_guard,
        _audit_guard: audit_guard,
    }
}

/// auth-off 时的 no-op 实现（架构审查 M1 修复）。
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
pub async fn acquire_garrison_global_state() -> NoopGarrisonGuard {
    NoopGarrisonGuard
}
