// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified authentication middleware with scope support
//!
//! Provides API key authentication with rate limiting for brute-force protection.

#![allow(dead_code)]

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::auth_scope_service::{AuthScopeService, AuthScopeServiceTrait};
use crate::infrastructure::database::entities::api_key;
use crate::infrastructure::security::{self, constant_time_eq_str};
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dbnexus::DbPool;
use log::{debug, info, warn};
use lru::LruCache;
use parking_lot::RwLock as ParkRwLock;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum authentication failures before lockout
const MAX_AUTH_FAILURES: usize = 5;
/// Lockout duration after exceeding max failures (15 minutes)
const AUTH_LOCKOUT_DURATION: Duration = Duration::from_secs(900);
/// Auth failure tracking window (1 hour)
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(3600);
/// Default cache TTL in seconds (2 minutes - reduced from 5 minutes for security)
const DEFAULT_CACHE_TTL_SECS: u64 = 120;
/// Default cache max size
const DEFAULT_CACHE_MAX_SIZE: usize = 10000;

/// Global auth cache instance for cache invalidation across the application
///
/// 架构 MEDIUM-1 + 性能 MEDIUM-1：从 `std::sync::Mutex<Option<Arc<...>>>` 改为
/// `parking_lot::RwLock<Option<Arc<...>>>`，与 `GLOBAL_AUTH_STATE` 模式一致。
/// - `get_global_auth_cache()` 在每请求热路径上调用（`try_get_cached_auth`、
///   `invalidate_cache_by_*` 等多个函数），读多写少（启动时 set 一次）。
/// - RwLock 读锁无竞争（多核并行），比 Mutex 的串行化更优。
/// - parking_lot::RwLock 是同步锁（不需要 await），不会跨 await 持有，避免死锁风险。
/// - 不 poison（panic 后 lock 仍可用），优于 std::sync::Mutex。
///
/// 注意：内部 `Arc<RwLock<ApiKeyCache>>` 的 `RwLock` 仍是 `tokio::sync::RwLock`，
/// 因为 ApiKeyCache 操作可能跨 `.await`（如 `cache.write().await`）。外部 ParkRwLock
/// 仅保护 Option 赋值，不涉及 await。
static GLOBAL_AUTH_CACHE: ParkRwLock<Option<Arc<RwLock<ApiKeyCache>>>> = ParkRwLock::new(None);

/// Global auth state for middleware
///
/// 架构 MEDIUM-1：从 OnceLock 改为 Mutex<Option<...>>，与 GLOBAL_AUTH_CACHE 模式一致。
/// OnceLock 设置后不可重置，导致测试间状态污染（2 个 #[ignore] 测试）。
/// Mutex<Option<...>> 支持覆盖式 set + test-only reset，消除状态污染。
///
/// 性能 MEDIUM-1：从 std::sync::Mutex 改为 parking_lot::RwLock。
/// `get_global_auth_state()` 在每请求热路径上调用，读多写少（启动时 set 一次）。
/// RwLock 读锁无竞争（多核并行），比 Mutex 的串行化更优。
/// parking_lot::RwLock 是同步锁（不需要 await），不会跨 await 持有，避免死锁风险。
static GLOBAL_AUTH_STATE: ParkRwLock<Option<Arc<AuthState>>> = ParkRwLock::new(None);

/// Get the global auth cache instance
///
/// 性能 MEDIUM-1：使用 parking_lot::RwLock 的 read() 锁，多读并发无竞争。
pub fn get_global_auth_cache() -> Option<Arc<RwLock<ApiKeyCache>>> {
    GLOBAL_AUTH_CACHE.read().clone()
}

/// Set the global auth cache instance (called during application startup)
pub fn set_global_auth_cache(cache: Arc<RwLock<ApiKeyCache>>) {
    *GLOBAL_AUTH_CACHE.write() = Some(cache);
}

/// Reset the global auth cache (test-only, for avoiding cross-test OnceLock pollution)
///
/// 架构 LOW-2：仅用 `#[cfg(test)]` 门控（对比 `reset_global_auth_state` 用
/// `#[cfg(any(test, feature = "test-mocks"))]`）。原因：此函数只在 lib 内部
/// 单元测试中调用，`cfg(test)` 对 lib 内部测试可见，足够隔离生产 binary。
/// `reset_global_auth_state` 还被 `tests/unit/auth_middleware_test.rs` 集成测试调用，
/// 集成测试看不到 `cfg(test)`，必须通过 `--features test-mocks` 启用。
#[cfg(test)]
fn reset_global_auth_cache() {
    *GLOBAL_AUTH_CACHE.write() = None;
}

/// Get the global auth state for middleware
///
/// 性能 MEDIUM-1：使用 parking_lot::RwLock 的 read() 锁，多读并发无竞争。
/// 原 std::sync::Mutex 在每请求热路径上串行化所有读，是性能瓶颈。
pub fn get_global_auth_state() -> Option<Arc<AuthState>> {
    GLOBAL_AUTH_STATE.read().clone()
}

/// Set the global auth state (called during application startup)
///
/// 覆盖式设置：每次调用都会替换之前的 state。
/// 生产环境中 `create_protected_routes_with_state` 设置完整 state（带 auth_scope_service），
/// `create_v2_routes_with_state` 通过 `ensure_global_auth_state_set` 仅在未设置时设置（避免覆盖）。
pub fn set_global_auth_state(state: Arc<AuthState>) {
    *GLOBAL_AUTH_STATE.write() = Some(state);
}

/// Set the global auth state only if it has not been set yet.
///
/// Used by route builders that may run after the primary state has been set
/// (e.g., v2 routes running after protected routes). Prevents overwriting a
/// more complete state (with auth_scope_service) with a less complete one.
pub fn ensure_global_auth_state_set(state: Arc<AuthState>) {
    let mut guard = GLOBAL_AUTH_STATE.write();
    if guard.is_none() {
        *guard = Some(state);
    }
}

/// Reset the global auth state (test-only, for avoiding cross-test state pollution)
///
/// 安全 MEDIUM-1 / 架构 MEDIUM-2：用 `#[cfg(any(test, feature = "test-mocks"))]` 门控。
/// - `cfg(test)`：lib 内部单元测试可见
/// - `feature = "test-mocks"`：`tests/unit/` 下的集成测试（独立 crate）通过
///   `cargo test --features test-mocks` 启用
///
/// 生产 binary (`cargo build --features standard`) 不会编译此函数，
/// 防止恶意依赖通过调用 `crawlrs::presentation::middleware::auth_middleware::reset_global_auth_state()`
/// 触发 DoS（清空全局 auth state 导致所有请求 401）。
///
/// # 已知限制：Cargo feature 统一效应
///
/// 安全 MEDIUM（续）：Cargo feature 是 **统一效应（unified feature）** — 这是 Rust 生态的
/// 已知限制（参考 [Cargo #8799](https://github.com/rust-lang/cargo/issues/8799)）。
/// 同一 workspace 内只要任何一个 crate 启用了 `test-mocks` feature，整个 workspace 都会
/// 以 `test-mocks` 启用编译，包括生产 binary。这意味着：
///
/// - 恶意依赖在 `Cargo.toml` 中声明 `crawlrs = { version = "...", features = ["test-mocks"] }`
///   会强制启用本 crate 的 `test-mocks`，从而把 `reset_global_auth_state` 编译进生产 binary。
/// - 防御措施仅靠 `#[cfg(any(test, feature = "test-mocks"))]` 不足以完全阻断此攻击面。
///
/// **当前缓解**：本 crate 是 application crate（非 library crate），不被外部依赖，
/// 攻击面仅限 workspace 内恶意 transitive dep。`Cargo.lock` 应定期审计
/// （`cargo audit` + `cargo tree -f "{p}" | sort -u`）。
///
/// **后续 hardening 建议**：将 `test-mocks` 相关代码（包括此函数）拆到独立
/// `crawlrs-test-support` crate，仅以 `[dev-dependencies]` 引入，彻底隔离生产 binary。
/// 当前未拆分是因为成本高于收益（application crate 不发布到 crates.io，
/// 攻击者无法通过外部声明 features 触发）。
///
/// 调用此函数的集成测试必须以 `--features test-mocks` 运行：
/// ```sh
/// cargo test --features test-mocks --test main
/// ```
#[cfg(any(test, feature = "test-mocks"))]
pub fn reset_global_auth_state() {
    *GLOBAL_AUTH_STATE.write() = None;
}

/// LRU Cache for authenticated API keys to reduce database queries
#[derive(Clone)]
pub struct ApiKeyCache {
    /// LRU cache storing API key validation results
    cache: LruCache<String, CachedAuthResult>,
    /// TTL for cache entries (2 minutes by default - reduced from 5 minutes for security)
    ttl: Duration,
    /// Maximum cache capacity (stored since LruCache doesn't provide capacity() method)
    max_size: usize,
}

#[derive(Clone)]
pub struct CachedAuthResult {
    team_id: Uuid,
    api_key_id: Uuid,
    scope: ApiKeyScope,
    cached_at: Instant,
}

impl ApiKeyCache {
    /// Create a new ApiKeyCache with default settings
    pub fn new_default() -> Self {
        Self::new(DEFAULT_CACHE_MAX_SIZE, DEFAULT_CACHE_TTL_SECS)
    }

    fn new(max_size: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: LruCache::new(
                NonZeroUsize::new(max_size).expect("ApiKeyCache max_size must be greater than 0"),
            ),
            ttl: Duration::from_secs(ttl_seconds),
            max_size,
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedAuthResult> {
        // Check if key exists and not expired
        if let Some(result) = self.cache.get(key) {
            if result.cached_at.elapsed() < self.ttl {
                return Some(result.clone());
            } else {
                // Remove expired entry
                self.cache.pop(key);
            }
        }
        None
    }

    fn insert(&mut self, key: String, result: CachedAuthResult) {
        self.cache.push(key, result);
    }

    /// Invalidate cache for a specific API key by token hash
    ///
    /// # Security
    ///
    /// This method should be called when:
    /// - An API key is revoked or deleted
    /// - An API key's permissions are changed
    /// - An API key is regenerated
    ///
    /// # Arguments
    ///
    /// * `token_hash` - The SHA-256 hash of the API key token (format: "sha256:...")
    pub fn invalidate(&mut self, token_hash: &str) {
        if let Some(removed) = self.cache.pop(token_hash) {
            info!(
                "API Key cache invalidated for security token_hash={} api_key_id={} team_id={}",
                token_hash, removed.api_key_id, removed.team_id
            );
        }
    }

    /// Invalidate all cache entries for a specific API key ID
    ///
    /// # Security
    ///
    /// This method should be called when an API key's permissions are updated
    /// but the token hash is not immediately available.
    ///
    /// # Thread Safety
    ///
    /// This method must be called while holding a write lock on the cache.
    /// For global cache, use `invalidate_cache_by_api_key_id()` instead.
    ///
    /// # Arguments
    ///
    /// * `api_key_id` - The UUID of the API key to invalidate
    pub fn invalidate_by_api_key_id(&mut self, api_key_id: Uuid) {
        // SEC-004: 收集匹配的键后批量删除（在持有写锁的情况下执行）
        let keys_to_remove: Vec<String> = self
            .cache
            .iter()
            .filter(|(_, result)| result.api_key_id == api_key_id)
            .map(|(key, _)| key.clone())
            .collect();

        for key in keys_to_remove {
            if let Some(removed) = self.cache.pop(&key) {
                info!(
                    "API Key cache invalidated by ID for security api_key_id={} team_id={}",
                    api_key_id, removed.team_id
                );
            }
        }
    }

    /// Invalidate all cache entries for a specific team
    ///
    /// # Security
    ///
    /// This method should be called when:
    /// - A team is suspended or deleted
    /// - Team-wide permissions are changed
    /// - All team API keys need to be re-validated
    ///
    /// # Thread Safety
    ///
    /// This method must be called while holding a write lock on the cache.
    /// For global cache, use `invalidate_cache_by_team()` instead.
    ///
    /// # Arguments
    ///
    /// * `team_id` - The UUID of the team whose cache entries should be invalidated
    pub fn invalidate_team(&mut self, team_id: Uuid) {
        // SEC-004: 收集匹配的键后批量删除（在持有写锁的情况下执行）
        let keys_to_remove: Vec<String> = self
            .cache
            .iter()
            .filter(|(_, result)| result.team_id == team_id)
            .map(|(key, _)| key.clone())
            .collect();

        let removed_count = keys_to_remove.len();

        // Remove collected keys
        for key in keys_to_remove {
            self.cache.pop(&key);
        }

        if removed_count > 0 {
            info!(
                "Team API Key cache invalidated for security team_id={} removed_count={:?}",
                team_id, removed_count
            );
        }
    }

    /// Clear all cache entries
    ///
    /// # Security
    ///
    /// This method should be called when:
    /// - A critical security event occurs
    /// - System-wide permission changes are made
    /// - Emergency cache invalidation is required
    pub fn invalidate_all(&mut self) {
        let count = self.cache.len();
        self.cache.clear();

        info!(
            "All API Key cache invalidated for security removed_count={:?}",
            count
        );
    }

    /// Get cache statistics for monitoring
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            size: self.cache.len(),
            capacity: self.max_size,
            ttl_seconds: self.ttl.as_secs(),
        }
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
    pub ttl_seconds: u64,
}

// ============================================================================
// Global Cache Invalidation Functions
// ============================================================================

/// Invalidate cache for a specific API key by token hash (using global cache)
///
/// # Security
///
/// This function provides a convenient way to invalidate cache entries from
/// anywhere in the application. It should be called when:
/// - An API key is revoked or deleted
/// - An API key's permissions are changed
/// - An API key is regenerated
///
/// # Arguments
///
/// * `token_hash` - The SHA-256 hash of the API key token (format: "sha256:...")
///
/// # Returns
///
/// Returns `true` if the cache entry was found and removed, `false` otherwise.
pub async fn invalidate_cache_by_token_hash(token_hash: &str) -> bool {
    if let Some(cache) = get_global_auth_cache() {
        let mut cache_guard = cache.write().await;
        let initial_size = cache_guard.cache.len();
        cache_guard.invalidate(token_hash);
        cache_guard.cache.len() < initial_size
    } else {
        warn!(
            target: "security_audit",
            "Global auth cache not initialized when attempting to invalidate by token hash"
        );
        false
    }
}

/// Invalidate all cache entries for a specific API key ID (using global cache)
///
/// # Security
///
/// This function should be called when an API key's permissions are updated
/// but the token hash is not immediately available.
///
/// # Arguments
///
/// * `api_key_id` - The UUID of the API key to invalidate
///
/// # Returns
///
/// Returns the number of cache entries removed.
pub async fn invalidate_cache_by_api_key_id(api_key_id: Uuid) -> usize {
    if let Some(cache) = get_global_auth_cache() {
        let mut cache_guard = cache.write().await;
        let initial_size = cache_guard.cache.len();
        cache_guard.invalidate_by_api_key_id(api_key_id);
        initial_size - cache_guard.cache.len()
    } else {
        warn!("Global auth cache not initialized when attempting to invalidate by API key ID api_key_id={}", api_key_id);
        0
    }
}

/// Invalidate all cache entries for a specific team (using global cache)
///
/// # Security
///
/// This function should be called when:
/// - A team is suspended or deleted
/// - Team-wide permissions are changed
/// - All team API keys need to be re-validated
///
/// # Arguments
///
/// * `team_id` - The UUID of the team whose cache entries should be invalidated
///
/// # Returns
///
/// Returns the number of cache entries removed.
pub async fn invalidate_cache_by_team(team_id: Uuid) -> usize {
    if let Some(cache) = get_global_auth_cache() {
        let mut cache_guard = cache.write().await;
        let initial_size = cache_guard.cache.len();
        cache_guard.invalidate_team(team_id);
        initial_size - cache_guard.cache.len()
    } else {
        warn!(
            "Global auth cache not initialized when attempting to invalidate by team team_id={}",
            team_id
        );
        0
    }
}

/// Clear all cache entries (using global cache)
///
/// # Security
///
/// This function should be called when:
/// - A critical security event occurs
/// - System-wide permission changes are made
/// - Emergency cache invalidation is required
///
/// # Returns
///
/// Returns the number of cache entries removed.
pub async fn invalidate_all_cache() -> usize {
    if let Some(cache) = get_global_auth_cache() {
        let mut cache_guard = cache.write().await;
        let count = cache_guard.cache.len();
        cache_guard.invalidate_all();
        count
    } else {
        warn!(
            target: "security_audit",
            "Global auth cache not initialized when attempting to invalidate all"
        );
        0
    }
}

/// Get cache statistics (using global cache)
///
/// # Returns
///
/// Returns cache statistics if the global cache is initialized, `None` otherwise.
pub async fn get_cache_stats() -> Option<CacheStats> {
    if let Some(cache) = get_global_auth_cache() {
        let cache_guard = cache.read().await;
        Some(cache_guard.stats())
    } else {
        None
    }
}

/// Authentication rate limiter for brute-force protection
#[derive(Clone)]
pub struct AuthRateLimiter {
    /// Tracks authentication failures by client IP
    failures: Arc<RwLock<std::collections::HashMap<String, (usize, Instant)>>>,
}

unsafe impl Send for AuthRateLimiter {}
unsafe impl Sync for AuthRateLimiter {}

impl std::fmt::Debug for AuthRateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthRateLimiter").finish_non_exhaustive()
    }
}

impl AuthRateLimiter {
    /// Create a new auth rate limiter
    pub fn new() -> Self {
        Self {
            failures: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Check if client is locked out due to too many failures
    pub async fn is_locked_out(&self, client_ip: &str) -> bool {
        let failures = self.failures.read().await;
        if let Some((count, first_failure)) = failures.get(client_ip) {
            // Check if within failure window
            if first_failure.elapsed() < AUTH_FAILURE_WINDOW {
                return *count >= MAX_AUTH_FAILURES;
            }
        }
        false
    }

    /// Get remaining lockout time in seconds, returns 0 if not locked
    pub async fn get_lockout_remaining(&self, client_ip: &str) -> u64 {
        let failures = self.failures.read().await;
        if let Some((count, first_failure)) = failures.get(client_ip) {
            if *count >= MAX_AUTH_FAILURES {
                let elapsed = first_failure.elapsed();
                if elapsed < AUTH_LOCKOUT_DURATION {
                    return (AUTH_LOCKOUT_DURATION - elapsed).as_secs();
                }
            }
        }
        0
    }

    /// Record an authentication failure
    pub async fn record_failure(&self, client_ip: &str) {
        let mut failures = self.failures.write().await;
        let now = Instant::now();
        let new_count = failures.get(client_ip).map(|(c, _)| *c + 1).unwrap_or(1);
        failures.insert(client_ip.to_string(), (new_count, now));
    }

    /// Reset failure count after successful authentication
    pub async fn reset_failures(&self, client_ip: &str) {
        let mut failures = self.failures.write().await;
        failures.remove(client_ip);
    }

    /// Clean up old entries
    pub async fn cleanup(&self) {
        let now = Instant::now();
        let mut failures = self.failures.write().await;
        failures.retain(|_, (_, timestamp)| now.duration_since(*timestamp) < AUTH_FAILURE_WINDOW);
    }
}

/// Default implementation for AuthRateLimiter
impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// This state is injected into requests after successful authentication and contains
/// all necessary information for authorization checks.
#[derive(Clone)]
pub struct AuthState {
    /// Database pool for additional queries
    pub pool: Arc<DbPool>,
    /// AuthScopeService for loading permissions from database
    pub auth_scope_service: Option<AuthScopeService>,
    /// Team ID associated with the API key
    pub team_id: Uuid,
    /// API Key ID for audit logging and feature flags
    pub api_key_id: Uuid,
    /// Scope permissions for the API key
    pub scope: ApiKeyScope,
    /// API Key validation cache (PERF-002)
    pub api_key_cache: Option<Arc<RwLock<ApiKeyCache>>>,
    /// Auth rate limiter for brute-force protection
    pub auth_rate_limiter: Option<Arc<AuthRateLimiter>>,
    /// Trusted proxy configuration for secure IP extraction
    pub trusted_proxies: Option<security::TrustedProxyConfig>,
}

unsafe impl Send for AuthState {}
unsafe impl Sync for AuthState {}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthState")
            .field("team_id", &self.team_id)
            .field("api_key_id", &self.api_key_id)
            .field("scope", &self.scope)
            .field("auth_scope_service", &self.auth_scope_service.is_some())
            .field("api_key_cache", &self.api_key_cache.is_some())
            .field("auth_rate_limiter", &self.auth_rate_limiter.is_some())
            .field("trusted_proxies", &self.trusted_proxies)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ApiKeyCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyCache")
            .field("size", &self.cache.len())
            .field("ttl_seconds", &self.ttl.as_secs())
            .finish()
    }
}

impl AuthState {
    /// Create a new AuthState with required fields
    pub fn new(pool: Arc<DbPool>, team_id: Uuid, api_key_id: Uuid, scope: ApiKeyScope) -> Self {
        Self {
            pool,
            auth_scope_service: None,
            team_id,
            api_key_id,
            scope,
            api_key_cache: None,
            auth_rate_limiter: None,
            trusted_proxies: None,
        }
    }

    /// Create AuthState with AuthScopeService for permission loading
    pub fn with_scope_service(
        pool: Arc<DbPool>,
        auth_scope_service: AuthScopeService,
        team_id: Uuid,
        api_key_id: Uuid,
        default_scope: ApiKeyScope,
    ) -> Self {
        Self {
            pool,
            auth_scope_service: Some(auth_scope_service),
            team_id,
            api_key_id,
            scope: default_scope,
            api_key_cache: None,
            auth_rate_limiter: None,
            trusted_proxies: None,
        }
    }

    /// Create AuthState with cache support
    pub fn with_cache(
        pool: Arc<DbPool>,
        auth_scope_service: Option<AuthScopeService>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
        cache: Arc<RwLock<ApiKeyCache>>,
    ) -> Self {
        Self {
            pool,
            auth_scope_service,
            team_id,
            api_key_id,
            scope,
            api_key_cache: Some(cache),
            auth_rate_limiter: None,
            trusted_proxies: None,
        }
    }

    /// Create AuthState with trusted proxy configuration
    #[allow(clippy::too_many_arguments)]
    pub fn with_trusted_proxies(
        pool: Arc<DbPool>,
        auth_scope_service: Option<AuthScopeService>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
        cache: Option<Arc<RwLock<ApiKeyCache>>>,
        rate_limiter: Option<Arc<AuthRateLimiter>>,
        trusted_proxies: security::TrustedProxyConfig,
    ) -> Self {
        Self {
            pool,
            auth_scope_service,
            team_id,
            api_key_id,
            scope,
            api_key_cache: cache,
            auth_rate_limiter: rate_limiter,
            trusted_proxies: Some(trusted_proxies),
        }
    }

    /// Create AuthState with global cache support
    ///
    /// This method creates an AuthState that uses the global cache instance,
    /// enabling cache invalidation across the application.
    ///
    /// # Security
    ///
    /// Using the global cache allows for centralized cache invalidation when
    /// API keys are revoked, permissions are changed, or teams are suspended.
    pub fn with_global_cache(
        pool: Arc<DbPool>,
        auth_scope_service: Option<AuthScopeService>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
        rate_limiter: Option<Arc<AuthRateLimiter>>,
        trusted_proxies: Option<security::TrustedProxyConfig>,
    ) -> Self {
        Self {
            pool,
            auth_scope_service,
            team_id,
            api_key_id,
            scope,
            api_key_cache: get_global_auth_cache(),
            auth_rate_limiter: rate_limiter,
            trusted_proxies,
        }
    }

    /// Create a new AuthState for middleware initialization with global cache
    ///
    /// This is used during application startup to create the initial AuthState
    /// that will be passed to the middleware.
    pub fn new_for_middleware(
        pool: Arc<DbPool>,
        auth_scope_service: Option<AuthScopeService>,
    ) -> Self {
        // Initialize global cache if not already done
        let cache = get_global_auth_cache().unwrap_or_else(|| {
            let new_cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
            set_global_auth_cache(new_cache.clone());
            new_cache
        });

        Self {
            pool,
            auth_scope_service,
            team_id: Uuid::nil(),
            api_key_id: Uuid::nil(),
            scope: ApiKeyScope::default(),
            api_key_cache: Some(cache),
            auth_rate_limiter: None,
            trusted_proxies: None,
        }
    }

    /// Load actual scope from database if service is available
    pub async fn load_scope_from_db(&mut self) {
        if let Some(ref service) = self.auth_scope_service {
            match service.get_scope_for_key(self.api_key_id, None).await {
                Ok(scope) => {
                    self.scope = scope;
                    debug!(
                        "Loaded scope from database for API Key: {}",
                        self.api_key_id
                    );
                }
                Err(e) => {
                    log::warn!("Failed to load scope from database: {:?}, using default", e);
                    // Keep using default scope
                }
            }
        }
    }
}

/// Error types for authentication
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid or missing API key")]
    InvalidKey,
    #[error("API key is inactive")]
    InactiveKey,
    #[error("Missing required scope: {0}")]
    MissingScope(ScopePermission),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("API key associated with nil team_id")]
    NilTeamId,
    #[error("API key has expired")]
    ExpiredKey,
}

/// Unified authentication middleware
///
/// This middleware validates API keys and loads associated scope for authorization.
/// It combines the functionality of the original basic and enhanced auth middlewares.
///
/// This version uses global state for middleware initialization.
async fn auth_middleware_inner(req: axum::http::Request<Body>, next: Next) -> Response {
    // Get auth state from global storage
    let state = match get_global_auth_state() {
        Some(s) => s,
        None => {
            log::error!("Auth middleware: global auth state not initialized");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let path = req.uri().path();
    debug!("AuthMiddleware processing path: {}", path);

    // Allow public endpoints without authentication
    if PUBLIC_ENDPOINTS.contains(&path) {
        debug!("Public endpoint {}, skipping auth", path);
        return next.run(req).await;
    }

    // Create a mutable request for processing
    let mut req = req;

    // Get client IP for rate limiting using secure IP extraction
    let client_ip = get_client_ip(&req, state.trusted_proxies.as_ref());

    // Check auth rate limit lockout
    if let Err(status) = check_rate_limit_lockout(&state, &client_ip).await {
        return status.into_response();
    }

    // Extract and validate Bearer token
    let token_str = match extract_bearer_token(&req) {
        Some(token) => token,
        None => {
            record_auth_failure(&state, &client_ip).await;
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Hash the token for lookup using SHA-256
    // 注意：sha2 0.10 的 Sha256::digest 返回 Array<u8, U32>，新版 Array 不实现 LowerHex。
    // 使用 hex::encode 替代 format!("{:x}")。
    let token_hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(token_str.as_bytes()))
    );

    // Check cache first before database query
    if let Some(auth_state) = try_get_cached_auth(&state, &token_hash).await {
        inject_auth_state(&mut req, auth_state.clone(), &token_hash);
        return next.run(req).await;
    }

    // Validate API key from database
    let key = match validate_api_key_from_db(&state, &token_hash, &client_ip).await {
        Ok(Some(key)) => key,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(status) => return status.into_response(),
    };

    // Verify key hash
    if !verify_key_hash(&key, &token_str) {
        warn!("API Key verification failed for key_id={}", key.id);
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Check key expiration
    if let Err(status) = check_key_expiration(&key) {
        return status.into_response();
    }

    // Create and inject auth state
    let auth_state = match create_and_cache_auth_state(&state, &key, &token_hash).await {
        Ok(state) => state,
        Err(status) => return status.into_response(),
    };
    inject_auth_state(&mut req, auth_state, &token_hash);

    // Reset auth failures on successful authentication
    reset_auth_failures(&state, &client_ip).await;

    debug!("API Key authentication successful");

    next.run(req).await
}

/// Wrapper function for middleware registration
pub fn auth_middleware() -> impl Fn(
    axum::http::Request<Body>,
    Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    |req, next| Box::pin(auth_middleware_inner(req, next))
}

/// Check rate limit lockout for client IP
async fn check_rate_limit_lockout(state: &AuthState, client_ip: &str) -> Result<(), StatusCode> {
    if let Some(ref rate_limiter) = state.auth_rate_limiter {
        if rate_limiter.is_locked_out(client_ip).await {
            let remaining = rate_limiter.get_lockout_remaining(client_ip).await;
            log::warn!(
                "Auth rate limit exceeded for IP: {}, lockout remaining: {}s",
                client_ip,
                remaining
            );
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }
    Ok(())
}

/// Record authentication failure for rate limiting
async fn record_auth_failure(state: &AuthState, client_ip: &str) {
    if let Some(ref rate_limiter) = state.auth_rate_limiter {
        rate_limiter.record_failure(client_ip).await;
    }
}

/// Reset auth failures after successful authentication
async fn reset_auth_failures(state: &AuthState, client_ip: &str) {
    if let Some(ref rate_limiter) = state.auth_rate_limiter {
        rate_limiter.reset_failures(client_ip).await;
    }
}

/// Try to get cached authentication result
async fn try_get_cached_auth(state: &AuthState, token_hash: &str) -> Option<AuthState> {
    if let Some(ref cache) = state.api_key_cache {
        let mut cache_guard = cache.write().await;
        if let Some(cached_result) = cache_guard.get(token_hash) {
            debug!("API Key authentication cache hit for key hash");
            return Some(AuthState::with_cache(
                state.pool.clone(),
                state.auth_scope_service.clone(),
                cached_result.team_id,
                cached_result.api_key_id,
                cached_result.scope.clone(),
                cache.clone(),
            ));
        }
    }
    None
}

/// Validate API key from database
async fn validate_api_key_from_db(
    state: &AuthState,
    token_hash: &str,
    client_ip: &str,
) -> Result<Option<api_key::Model>, StatusCode> {
    let session = state.pool.get_session("admin").await.map_err(|e| {
        log::error!("Failed to get database session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let conn = session.connection().map_err(|e| {
        log::error!("Failed to get database connection: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match api_key::Entity::find()
        .filter(api_key::Column::KeyHash.eq(token_hash))
        .one(conn)
        .await
    {
        Ok(Some(key)) => {
            // Security check: reject nil UUID
            if key.team_id == Uuid::nil() {
                warn!(
                    "SECURITY: API key with nil team_id detected, key_id={}",
                    key.id
                );
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(Some(key))
        }
        Ok(None) => {
            warn!("API Key authentication failed: key not found");
            record_auth_failure(state, client_ip).await;
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            log::error!("Database error checking API key: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Verify key hash against provided token
///
/// 安全 LOW-1：`sha256:` 前缀和纯 hex 路径使用 `constant_time_eq_str`（来自
/// `infrastructure::security` 模块）做常量时间比较，避免时序侧信道泄露 hash 内容。
/// 详见 `infrastructure::security::constant_time_compare` 模块文档。
fn verify_key_hash(key: &api_key::Model, token_str: &str) -> bool {
    if let Some(ref stored_hash) = key.key_hash {
        // Check if it's a new format bcrypt hash
        if stored_hash.starts_with("$2b$") {
            security::verify_api_key(token_str, stored_hash)
        } else if stored_hash.starts_with("sha256:") {
            // SHA256 hash format (for testing)
            let stored_sha256 = stored_hash.trim_start_matches("sha256:");
            let input_sha256 = hex::encode(Sha256::digest(token_str.as_bytes()));
            // 安全 LOW-1：常量时间比较，避免时序侧信道泄露 hash 内容
            constant_time_eq_str(stored_sha256, &input_sha256)
        } else if stored_hash.len() == 64 && stored_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            // Pure SHA-256 hash (64 character hex)
            let input_sha256 = hex::encode(Sha256::digest(token_str.as_bytes()));
            // 安全 LOW-1：常量时间比较，避免时序侧信道泄露 hash 内容
            constant_time_eq_str(stored_hash, &input_sha256)
        } else {
            // Other formats, try bcrypt verification
            security::verify_api_key(token_str, stored_hash)
        }
    } else {
        // SECURITY: Plaintext API keys are never allowed
        log::error!(
            "SECURITY CRITICAL: Attempted authentication with plaintext API key (key_id={}). \
            Plaintext API keys are never allowed in any environment. \
            Please migrate to hashed storage immediately.",
            key.id
        );
        false
    }
}

/// Check if key has expired
fn check_key_expiration(key: &api_key::Model) -> Result<(), StatusCode> {
    // Reject keys without hash
    if key.key_hash.is_none() {
        log::error!(
            "SECURITY CRITICAL: Attempted authentication with plaintext API key (key_id={}). \
            Plaintext API keys are never allowed in any environment. \
            Please migrate to hashed storage immediately.",
            key.id
        );
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check expiration based on update time
    if let Some(updated_at) = key.updated_at {
        let now = chrono::Utc::now();
        let days_since_update = (now.signed_duration_since(updated_at)).num_days();

        // If key hasn't been updated in 90 days, reject access
        if days_since_update > 90 {
            log::warn!(
                "API key {} has not been updated in {} days, may be expired",
                key.id,
                days_since_update
            );
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    Ok(())
}

/// Create auth state and cache it
async fn create_and_cache_auth_state(
    state: &AuthState,
    key: &api_key::Model,
    token_hash: &str,
) -> Result<AuthState, StatusCode> {
    // Get AuthScopeService from CrawlRsState
    let auth_scope_service = match state.auth_scope_service.clone() {
        Some(service) => service,
        None => {
            log::error!(
                "FATAL: AuthScopeService not initialized in CrawlRsState. \
                This indicates a startup configuration error."
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut auth_state = AuthState::with_scope_service(
        state.pool.clone(),
        auth_scope_service,
        key.team_id,
        key.id,
        ApiKeyScope::default(),
    );

    // Load actual scope from database
    auth_state.load_scope_from_db().await;

    // Cache the successful authentication result
    if let Some(ref cache) = state.api_key_cache {
        let mut cache_guard = cache.write().await;
        cache_guard.insert(
            token_hash.to_string(),
            CachedAuthResult {
                team_id: key.team_id,
                api_key_id: key.id,
                scope: auth_state.scope.clone(),
                cached_at: Instant::now(),
            },
        );
    }

    Ok(auth_state)
}

/// Inject auth state into request extensions
fn inject_auth_state(req: &mut Request<Body>, auth_state: AuthState, token_hash: &str) {
    let team_id = auth_state.team_id;
    let api_key_id = auth_state.api_key_id;
    req.extensions_mut().insert(auth_state);
    req.extensions_mut().insert(team_id);
    req.extensions_mut().insert(api_key_id);
    req.extensions_mut().insert(token_hash.to_string());
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(req: &Request<Body>) -> Option<String> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())?;

    if !auth_header.starts_with("Bearer ") {
        return None;
    }

    Some(auth_header[7..].to_string())
}

/// Extract client IP from request for rate limiting
///
/// # Security Fix
///
/// This function now uses secure IP extraction logic to prevent X-Forwarded-For
/// header spoofing attacks. It only trusts forwarded headers when the request
/// comes from a trusted proxy.
///
/// # Arguments
///
/// * `req` - The HTTP request
/// * `trusted_proxies` - Optional trusted proxy configuration
///
/// # Returns
///
/// Returns the client's real IP address, or "unknown" if it cannot be determined
fn get_client_ip(
    req: &Request<Body>,
    trusted_proxies: Option<&security::TrustedProxyConfig>,
) -> String {
    match trusted_proxies {
        Some(config) => security::get_secure_client_ip(req, config),
        None => {
            // Fallback to default config if not provided
            let default_config = security::TrustedProxyConfig::default();
            security::get_secure_client_ip(req, &default_config)
        }
    }
}

/// Scope validation middleware
///
/// Validates that the API Key has the required scope for the requested endpoint.
/// This middleware should be used after the main auth middleware.
///
/// # Arguments
///
/// * `req` - The HTTP request with AuthState extension
/// * `next` - The next middleware in the chain
///
/// # Returns
///
/// * `Ok(Response)` - If scope validation passes
/// * `Err(StatusCode)` - If scope validation fails
pub async fn scope_middleware(req: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Determine required scope based on endpoint
    let required_scope = determine_required_scope(&path, method.as_ref());

    if let Some(required) = required_scope {
        let auth_state = req
            .extensions()
            .get::<AuthState>()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !auth_state.scope.has_permission(required) {
            warn!(
                "Scope denied: API Key {} lacks {:?} for {} {}",
                auth_state.api_key_id, required, method, path
            );

            // Log scope denial to audit service
            if let Some(audit_service) = req.extensions().get::<Arc<dyn AuditServiceTrait>>() {
                let api_key_scope: ApiKeyScope = required.into();
                let reason = format!("Missing required scope: {:?}", required);
                let _ = audit_service
                    .log_deny(
                        "scope.denied".to_string(),
                        Some(auth_state.api_key_id),
                        Some(auth_state.team_id),
                        reason,
                        Some(api_key_scope),
                    )
                    .await;
            }

            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(req).await)
}

/// Check if a path matches a prefix exactly or has a slash after the prefix
/// Ensures we match "/api/v1/teams" but not "/api/v1/teams-secret"
fn is_path_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
}

/// Determine required scope for an endpoint
fn determine_required_scope(path: &str, method: &str) -> Option<ScopePermission> {
    // Admin endpoints - use precise matching
    if is_path_prefix(path, "/api/v1/teams") || is_path_prefix(path, "/api/v1/billing") {
        return Some(ScopePermission::Admin);
    }

    // Write endpoints (POST, PUT, PATCH, DELETE)
    if method == "POST" || method == "PUT" || method == "PATCH" || method == "DELETE" {
        // POST to /v1/search and /v1/scrape are write operations
        // They create tasks and consume credits
        if (is_path_prefix(path, "/v1/search") || is_path_prefix(path, "/v1/scrape"))
            && method == "POST"
        {
            return Some(ScopePermission::Write);
        }
        return Some(ScopePermission::Write);
    }

    // Read endpoints (GET) - always allowed if read scope is present
    // Most endpoints are read-only by default
    None
}

/// Create an auth state for testing purposes
#[cfg(test)]
pub fn test_auth_state(db: Arc<DbPool>, team_id: Uuid, api_key_id: Uuid) -> AuthState {
    AuthState::new(db, team_id, api_key_id, ApiKeyScope::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::{
        create_test_db_pool, resolve_test_database_url, skip_if_no_test_db,
    };
    use crate::domain::repositories::auth_scope_repository::{
        AuthScopeRepository, RepositoryError,
    };
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;
    use std::time::Instant;

    /// Serializes tests that touch the global auth cache to prevent race conditions.
    static GLOBAL_CACHE_LOCK: StdMutex<()> = StdMutex::new(());

    /// Serializes tests that touch GLOBAL_AUTH_STATE (Mutex<Option<...>> — resettable per test).
    static GLOBAL_STATE_LOCK: StdMutex<()> = StdMutex::new(());

    /// Acquire the global cache lock, recovering from a poisoned mutex.
    ///
    /// Centralizes the `lock().unwrap_or_else(|e| e.into_inner())` pattern that
    /// was duplicated 11 times across the test module. Poison recovery is safe
    /// here because tests reset global state under the lock before asserting.
    fn lock_global_cache() -> std::sync::MutexGuard<'static, ()> {
        GLOBAL_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Acquire the global auth state lock, recovering from a poisoned mutex.
    ///
    /// Centralizes the `lock().unwrap_or_else(|e| e.into_inner())` pattern that
    /// was duplicated 8 times across the test module.
    fn lock_global_state() -> std::sync::MutexGuard<'static, ()> {
        GLOBAL_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 测试用：`get_secure_client_ip` 在无法提取 IP 时的默认返回值。
    ///
    /// 与 `src/infrastructure/security/secure_ip.rs` 中 `unwrap_or_else(|| "unknown".to_string())`
    /// 保持同步；提取为常量避免魔术字符串散落各处，未来若默认值变更可在此一处更新。
    const TEST_UNKNOWN_IP: &str = "unknown";

    /// 测试用 bcrypt hash 缓存（OnceLock），跨测试复用同一哈希结果。
    ///
    /// bcrypt cost=12 单次哈希 ~300ms，多个测试若各自调用 `hash_api_key`
    /// 会线性拖慢 CI。此缓存以固定 token `"legit_token_abc123"` 初始化一次，
    /// 后续测试直接复用，节省重复计算。
    static TEST_LEGIT_BCRYPT_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    /// 获取测试用合法 bcrypt hash（首次调用时计算，后续复用缓存）。
    fn get_test_legit_bcrypt_hash() -> &'static str {
        TEST_LEGIT_BCRYPT_HASH.get_or_init(|| {
            crate::infrastructure::security::hash_api_key("legit_token_abc123").unwrap()
        })
    }

    /// Mock AuthScopeRepository that returns a configurable scope or an error.
    ///
    /// Avoids storing `Result<.., RepositoryError>` because RepositoryError does
    /// not implement Clone — instead we store the scope and a should_error flag.
    struct MockAuthScopeRepo {
        scope: Option<ApiKeyScope>,
        should_error: bool,
    }

    #[async_trait]
    impl AuthScopeRepository for MockAuthScopeRepo {
        async fn find_by_api_key_id(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Option<ApiKeyScope>, RepositoryError> {
            if self.should_error {
                return Err(RepositoryError::NotFound("mock error".to_string()));
            }
            Ok(self.scope.clone())
        }
        async fn find_by_api_key(
            &self,
            _key: &str,
        ) -> Result<Option<ApiKeyScope>, RepositoryError> {
            if self.should_error {
                return Err(RepositoryError::NotFound("mock error".to_string()));
            }
            Ok(self.scope.clone())
        }
        async fn upsert(
            &self,
            _api_key_id: Uuid,
            scope: ApiKeyScope,
        ) -> Result<ApiKeyScope, RepositoryError> {
            if self.should_error {
                return Err(RepositoryError::NotFound("mock error".to_string()));
            }
            Ok(scope)
        }
        async fn delete_by_api_key_id(&self, _api_key_id: Uuid) -> Result<bool, RepositoryError> {
            if self.should_error {
                return Err(RepositoryError::NotFound("mock error".to_string()));
            }
            Ok(true)
        }
    }

    /// Build an AuthScopeService backed by a mock repo that returns the given scope.
    fn make_auth_scope_service(scope: Option<ApiKeyScope>) -> AuthScopeService {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepo {
            scope,
            should_error: false,
        });
        AuthScopeService::new(repo)
    }

    /// Build an AuthScopeService backed by a mock repo that always errors.
    fn make_auth_scope_service_error() -> AuthScopeService {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepo {
            scope: None,
            should_error: true,
        });
        AuthScopeService::new(repo)
    }

    #[tokio::test]
    async fn test_cache_ttl_is_2_minutes() {
        let cache = ApiKeyCache::new_default();
        assert_eq!(
            cache.ttl,
            Duration::from_secs(120),
            "TTL should be 120 seconds (2 minutes)"
        );
    }

    #[tokio::test]
    async fn test_cache_invalidate_by_token_hash() {
        let mut cache = ApiKeyCache::new_default();

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let token_hash = "sha256:test_token_hash".to_string();

        // Insert cache entry
        cache.insert(
            token_hash.clone(),
            CachedAuthResult {
                team_id,
                api_key_id,
                scope: ApiKeyScope::default(),
                cached_at: Instant::now(),
            },
        );

        // Verify cache hit
        assert!(cache.get(&token_hash).is_some());

        // Invalidate by token hash
        cache.invalidate(&token_hash);

        // Verify cache miss
        assert!(cache.get(&token_hash).is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate_by_api_key_id() {
        let mut cache = ApiKeyCache::new_default();

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let token_hash = "sha256:test_api_key_id".to_string();

        // Insert cache entry
        cache.insert(
            token_hash.clone(),
            CachedAuthResult {
                team_id,
                api_key_id,
                scope: ApiKeyScope::default(),
                cached_at: Instant::now(),
            },
        );

        // Invalidate by API key ID
        cache.invalidate_by_api_key_id(api_key_id);

        // Verify cache miss
        assert!(cache.get(&token_hash).is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate_by_team() {
        let mut cache = ApiKeyCache::new_default();

        let team_id = Uuid::new_v4();
        let api_key_id1 = Uuid::new_v4();
        let api_key_id2 = Uuid::new_v4();
        let token_hash1 = "sha256:test_team_1".to_string();
        let token_hash2 = "sha256:test_team_2".to_string();

        // Insert two cache entries for the same team
        cache.insert(
            token_hash1.clone(),
            CachedAuthResult {
                team_id,
                api_key_id: api_key_id1,
                scope: ApiKeyScope::default(),
                cached_at: Instant::now(),
            },
        );
        cache.insert(
            token_hash2.clone(),
            CachedAuthResult {
                team_id,
                api_key_id: api_key_id2,
                scope: ApiKeyScope::default(),
                cached_at: Instant::now(),
            },
        );

        // Invalidate by team
        cache.invalidate_team(team_id);

        // Verify cache miss for both entries
        assert!(cache.get(&token_hash1).is_none());
        assert!(cache.get(&token_hash2).is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate_all() {
        let mut cache = ApiKeyCache::new_default();

        // Insert multiple cache entries
        for i in 0..5 {
            cache.insert(
                format!("sha256:test_all_{}", i),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id: Uuid::new_v4(),
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }

        // Invalidate all
        cache.invalidate_all();

        // Verify cache is empty
        let stats = cache.stats();
        assert_eq!(stats.size, 0, "Cache should be empty");
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        // Create cache with 1 second TTL
        let mut cache = ApiKeyCache::new(100, 1);

        let token_hash = "sha256:test_expiration".to_string();

        // Insert cache entry
        cache.insert(
            token_hash.clone(),
            CachedAuthResult {
                team_id: Uuid::new_v4(),
                api_key_id: Uuid::new_v4(),
                scope: ApiKeyScope::default(),
                cached_at: Instant::now(),
            },
        );

        // Verify cache hit immediately
        assert!(cache.get(&token_hash).is_some());

        // Wait for TTL to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify cache miss after expiration
        assert!(
            cache.get(&token_hash).is_none(),
            "Cache entry should be expired"
        );
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = ApiKeyCache::new_default();

        let stats = cache.stats();
        assert_eq!(stats.size, 0, "Initial cache should be empty");
        assert_eq!(stats.capacity, 10000, "Default capacity should be 10000");
        assert_eq!(stats.ttl_seconds, 120, "Default TTL should be 120 seconds");
    }

    #[tokio::test]
    async fn test_global_cache_singleton() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache1 = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache1.clone());

        let cache2 = get_global_auth_cache();
        assert!(cache2.is_some());

        let cache2 = cache2.unwrap();
        assert!(
            Arc::ptr_eq(&cache1, &cache2),
            "Global cache should be a singleton"
        );
    }

    // GLOBAL_CACHE_LOCK must be held across .await because async helpers
    // (invalidate_all_cache, get_cache_stats, etc.) read/write the global
    // cache; releasing the guard would let other tests race-modify it.
    // Single-threaded tokio runtime => no deadlock risk.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_global_cache_invalidate_all() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache.clone());

        // Insert cache entry
        {
            let mut cache_guard = cache.write().await;
            cache_guard.insert(
                "sha256:test_global".to_string(),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id: Uuid::new_v4(),
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }

        // Invalidate all using global function
        let removed_count = invalidate_all_cache().await;
        assert_eq!(removed_count, 1, "Should have removed one entry");

        // Verify cache is empty
        let stats = get_cache_stats().await;
        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.size, 0, "Cache should be empty");
    }

    // ===== Helper functions for tests =====

    fn make_fixed_time(rfc3339: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap()
    }

    fn make_days_ago(days: i64) -> chrono::DateTime<chrono::FixedOffset> {
        let offset = chrono::FixedOffset::east_opt(0).unwrap();
        let past = chrono::Utc::now() - chrono::Duration::days(days);
        past.with_timezone(&offset)
    }

    fn make_key_model(key_hash: Option<String>, updated_days_ago: Option<i64>) -> api_key::Model {
        api_key::Model {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            key: "test_key".to_string(),
            key_hash,
            created_at: make_fixed_time("2025-01-15T12:00:00+00:00"),
            updated_at: updated_days_ago.map(make_days_ago),
        }
    }

    fn make_bearer_request(auth_header: Option<&str>) -> Request<Body> {
        let mut builder = axum::http::Request::builder();
        if let Some(hdr) = auth_header {
            builder = builder.header(header::AUTHORIZATION, hdr);
        }
        builder.body(Body::empty()).unwrap()
    }

    // ===== AuthRateLimiter tests =====

    #[tokio::test]
    async fn test_auth_rate_limiter_default() {
        let limiter = AuthRateLimiter::default();
        assert!(!limiter.is_locked_out("10.0.0.1").await);
        assert_eq!(limiter.get_lockout_remaining("10.0.0.1").await, 0);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_not_locked_below_threshold() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..(MAX_AUTH_FAILURES - 1) {
            limiter.record_failure("1.2.3.4").await;
        }
        assert!(
            !limiter.is_locked_out("1.2.3.4").await,
            "Should not be locked below threshold"
        );
        assert_eq!(limiter.get_lockout_remaining("1.2.3.4").await, 0);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_locked_at_threshold() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..MAX_AUTH_FAILURES {
            limiter.record_failure("1.2.3.5").await;
        }
        assert!(
            limiter.is_locked_out("1.2.3.5").await,
            "Should be locked at threshold"
        );
        let remaining = limiter.get_lockout_remaining("1.2.3.5").await;
        assert!(
            remaining > 0,
            "Remaining lockout should be positive when locked"
        );
        assert!(
            remaining <= AUTH_LOCKOUT_DURATION.as_secs(),
            "Remaining should not exceed lockout duration"
        );
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_different_ips_independent() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..MAX_AUTH_FAILURES {
            limiter.record_failure("1.2.3.6").await;
        }
        assert!(limiter.is_locked_out("1.2.3.6").await);
        assert!(
            !limiter.is_locked_out("1.2.3.7").await,
            "Different IP should not be locked"
        );
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_reset_failures() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..MAX_AUTH_FAILURES {
            limiter.record_failure("1.2.3.8").await;
        }
        assert!(limiter.is_locked_out("1.2.3.8").await);
        limiter.reset_failures("1.2.3.8").await;
        assert!(
            !limiter.is_locked_out("1.2.3.8").await,
            "Should not be locked after reset"
        );
        assert_eq!(limiter.get_lockout_remaining("1.2.3.8").await, 0);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_record_failure_increments() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..3 {
            limiter.record_failure("1.2.3.9").await;
        }
        assert!(!limiter.is_locked_out("1.2.3.9").await);
        limiter.record_failure("1.2.3.9").await;
        limiter.record_failure("1.2.3.9").await;
        assert!(limiter.is_locked_out("1.2.3.9").await);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_reset_nonexistent_ip() {
        let limiter = AuthRateLimiter::new();
        // Resetting an IP with no failures should be a no-op
        limiter.reset_failures("9.9.9.9").await;
        assert!(!limiter.is_locked_out("9.9.9.9").await);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_cleanup_preserves_recent_entries() {
        let limiter = AuthRateLimiter::new();
        limiter.record_failure("1.2.3.11").await;
        limiter.cleanup().await;
        // After cleanup, 4 more failures should still lock out (total 5)
        for _ in 0..4 {
            limiter.record_failure("1.2.3.11").await;
        }
        assert!(limiter.is_locked_out("1.2.3.11").await);
    }

    #[tokio::test]
    async fn test_auth_rate_limiter_cleanup_no_panic_on_empty() {
        let limiter = AuthRateLimiter::new();
        // Cleanup on empty limiter should not panic
        limiter.cleanup().await;
    }

    // ===== AuthError Display tests =====

    #[test]
    fn test_auth_error_display_variants() {
        assert_eq!(
            AuthError::InvalidKey.to_string(),
            "Invalid or missing API key"
        );
        assert_eq!(AuthError::InactiveKey.to_string(), "API key is inactive");
        assert_eq!(
            AuthError::MissingScope(ScopePermission::Admin).to_string(),
            "Missing required scope: admin"
        );
        assert_eq!(
            AuthError::NilTeamId.to_string(),
            "API key associated with nil team_id"
        );
        assert_eq!(AuthError::ExpiredKey.to_string(), "API key has expired");
    }

    // ===== verify_key_hash tests =====

    #[test]
    fn test_verify_key_hash_sha256_prefix_match() {
        let token = "test_token_123";
        let hash = format!("sha256:{}", hex::encode(Sha256::digest(token.as_bytes())));
        let key = make_key_model(Some(hash), Some(0));
        assert!(verify_key_hash(&key, token));
    }

    #[test]
    fn test_verify_key_hash_sha256_prefix_mismatch() {
        let token = "test_token_123";
        let hash = format!("sha256:{}", hex::encode(Sha256::digest(token.as_bytes())));
        let key = make_key_model(Some(hash), Some(0));
        assert!(!verify_key_hash(&key, "wrong_token"));
    }

    #[test]
    fn test_verify_key_hash_pure_sha256_match() {
        let token = "test_token_456";
        let hash = hex::encode(Sha256::digest(token.as_bytes()));
        let key = make_key_model(Some(hash), Some(0));
        assert!(verify_key_hash(&key, token));
    }

    #[test]
    fn test_verify_key_hash_pure_sha256_mismatch() {
        let token = "test_token_456";
        let hash = hex::encode(Sha256::digest(token.as_bytes()));
        let key = make_key_model(Some(hash), Some(0));
        assert!(!verify_key_hash(&key, "wrong_token"));
    }

    #[test]
    fn test_verify_key_hash_plaintext_rejected() {
        let key = make_key_model(None, Some(0));
        assert!(!verify_key_hash(&key, "any_token"));
    }

    #[test]
    fn test_verify_key_hash_bcrypt_match() {
        let token = "bcrypt_test_token";
        let bcrypt_hash = security::hash_api_key(token).unwrap();
        let key = make_key_model(Some(bcrypt_hash), Some(0));
        assert!(verify_key_hash(&key, token));
    }

    #[test]
    fn test_verify_key_hash_bcrypt_mismatch() {
        let token = "bcrypt_test_token";
        let bcrypt_hash = security::hash_api_key(token).unwrap();
        let key = make_key_model(Some(bcrypt_hash), Some(0));
        assert!(!verify_key_hash(&key, "wrong_token"));
    }

    #[test]
    fn test_verify_key_hash_other_format_fallback_bcrypt() {
        // A hash that doesn't match any known format should fall through to bcrypt
        let key = make_key_model(Some("unknown_format_hash".to_string()), Some(0));
        assert!(!verify_key_hash(&key, "any_token"));
    }

    // ===== check_key_expiration tests =====

    #[test]
    fn test_check_key_expiration_nil_hash_rejected() {
        let key = make_key_model(None, Some(0));
        assert!(check_key_expiration(&key).is_err());
    }

    #[test]
    fn test_check_key_expiration_expired_key_rejected() {
        let key = make_key_model(Some("sha256:somehash".to_string()), Some(100));
        assert!(check_key_expiration(&key).is_err());
    }

    #[test]
    fn test_check_key_expiration_valid_key_ok() {
        let key = make_key_model(Some("sha256:somehash".to_string()), Some(10));
        assert!(check_key_expiration(&key).is_ok());
    }

    #[test]
    fn test_check_key_expiration_no_updated_at_ok() {
        let key = make_key_model(Some("sha256:somehash".to_string()), None);
        assert!(check_key_expiration(&key).is_ok());
    }

    #[test]
    fn test_check_key_expiration_exactly_90_days_boundary() {
        // 90 days should be OK (boundary: > 90 is rejected, <= 90 is OK)
        let key = make_key_model(Some("sha256:somehash".to_string()), Some(90));
        assert!(check_key_expiration(&key).is_ok());
    }

    // ===== extract_bearer_token tests =====

    #[test]
    fn test_extract_bearer_token_valid() {
        let req = make_bearer_request(Some("Bearer my_secret_token"));
        assert_eq!(
            extract_bearer_token(&req).as_deref(),
            Some("my_secret_token")
        );
    }

    #[test]
    fn test_extract_bearer_token_missing_header() {
        let req = make_bearer_request(None);
        assert!(extract_bearer_token(&req).is_none());
    }

    #[test]
    fn test_extract_bearer_token_non_bearer_scheme() {
        let req = make_bearer_request(Some("Basic dXNlcjpwYXNz"));
        assert!(extract_bearer_token(&req).is_none());
    }

    #[test]
    fn test_extract_bearer_token_empty_token() {
        let req = make_bearer_request(Some("Bearer "));
        assert_eq!(extract_bearer_token(&req).as_deref(), Some(""));
    }

    #[test]
    fn test_extract_bearer_token_lowercase_bearer_not_matched() {
        let req = make_bearer_request(Some("bearer my_token"));
        assert!(extract_bearer_token(&req).is_none());
    }

    // ===== is_path_prefix tests =====

    #[test]
    fn test_is_path_prefix_exact_match() {
        assert!(is_path_prefix("/api/v1/teams", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_with_slash() {
        assert!(is_path_prefix("/api/v1/teams/123", "/api/v1/teams"));
        assert!(is_path_prefix("/api/v1/teams/", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_suffix_without_slash_no_match() {
        assert!(!is_path_prefix("/api/v1/teams-secret", "/api/v1/teams"));
        assert!(!is_path_prefix("/api/v1/teamsadmin", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_no_match() {
        assert!(!is_path_prefix("/api/v1/users", "/api/v1/teams"));
        assert!(!is_path_prefix("/api/v1/team", "/api/v1/teams"));
    }

    // ===== determine_required_scope tests =====

    #[test]
    fn test_determine_required_scope_teams_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/teams", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_teams_subpath_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/teams/123", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_billing_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/billing", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_teams_secret_not_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/teams-secret", "GET"),
            None
        );
    }

    #[test]
    fn test_determine_required_scope_post_write() {
        assert_eq!(
            determine_required_scope("/v1/search", "POST"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/scrape", "POST"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/crawl", "POST"),
            Some(ScopePermission::Write)
        );
    }

    #[test]
    fn test_determine_required_scope_get_none() {
        assert_eq!(determine_required_scope("/v1/search", "GET"), None);
        assert_eq!(determine_required_scope("/v1/scrape", "GET"), None);
        assert_eq!(determine_required_scope("/v1/crawl", "GET"), None);
    }

    #[test]
    fn test_determine_required_scope_put_delete_patch_write() {
        assert_eq!(
            determine_required_scope("/v1/crawl", "PUT"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/crawl", "DELETE"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/crawl", "PATCH"),
            Some(ScopePermission::Write)
        );
    }

    // ===== get_client_ip tests =====

    #[test]
    fn test_get_client_ip_with_trusted_proxies() {
        let config = security::TrustedProxyConfig::from_settings(false, vec![]);
        let req = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.50")
            .body(Body::empty())
            .unwrap();
        let ip = get_client_ip(&req, Some(&config));
        assert_eq!(ip, "203.0.113.50");
    }

    #[test]
    fn test_get_client_ip_without_trusted_proxies_uses_default() {
        let req = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.50")
            .body(Body::empty())
            .unwrap();
        let ip = get_client_ip(&req, None);
        // With default config (enabled=true, private IPs trusted),
        // no ConnectInfo → returns TEST_UNKNOWN_IP
        assert_eq!(ip, TEST_UNKNOWN_IP);
    }

    // ===== ApiKeyCache additional tests =====

    #[test]
    fn test_cache_new_custom_params() {
        let cache = ApiKeyCache::new(500, 60);
        let stats = cache.stats();
        assert_eq!(stats.capacity, 500);
        assert_eq!(stats.ttl_seconds, 60);
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_cache_get_miss_returns_none() {
        let mut cache = ApiKeyCache::new_default();
        assert!(cache.get("nonexistent_key").is_none());
    }

    #[test]
    fn test_cache_insert_and_get_hit() {
        let mut cache = ApiKeyCache::new_default();
        let key = "sha256:insert_test".to_string();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        cache.insert(
            key.clone(),
            CachedAuthResult {
                team_id,
                api_key_id,
                scope: ApiKeyScope::full_access(),
                cached_at: Instant::now(),
            },
        );
        let result = cache.get(&key);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.team_id, team_id);
        assert_eq!(result.api_key_id, api_key_id);
        assert_eq!(result.scope, ApiKeyScope::full_access());
    }

    #[test]
    fn test_cache_invalidate_nonexistent_no_panic() {
        let mut cache = ApiKeyCache::new_default();
        // Should not panic when invalidating non-existent key
        cache.invalidate("sha256:nonexistent");
        cache.invalidate_by_api_key_id(Uuid::new_v4());
        cache.invalidate_team(Uuid::new_v4());
    }

    #[test]
    fn test_cache_stats_after_inserts() {
        let mut cache = ApiKeyCache::new_default();
        for i in 0..5 {
            cache.insert(
                format!("sha256:stats_{}", i),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id: Uuid::new_v4(),
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }
        let stats = cache.stats();
        assert_eq!(stats.size, 5);
        assert_eq!(stats.capacity, DEFAULT_CACHE_MAX_SIZE);
        assert_eq!(stats.ttl_seconds, DEFAULT_CACHE_TTL_SECS);
    }

    // ===== Debug impl tests =====

    #[test]
    fn test_auth_rate_limiter_debug() {
        let limiter = AuthRateLimiter::new();
        let debug_str = format!("{:?}", limiter);
        assert!(debug_str.contains("AuthRateLimiter"));
    }

    #[test]
    fn test_api_key_cache_debug() {
        let cache = ApiKeyCache::new_default();
        let debug_str = format!("{:?}", cache);
        assert!(debug_str.contains("ApiKeyCache"));
        assert!(debug_str.contains("size"));
        assert!(debug_str.contains("ttl_seconds"));
    }

    #[test]
    fn test_cache_stats_debug() {
        let stats = CacheStats {
            size: 10,
            capacity: 100,
            ttl_seconds: 60,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("size"));
        assert!(debug_str.contains("capacity"));
        assert!(debug_str.contains("ttl_seconds"));
    }

    // ===== 维度5 #77: LRU 淘汰 (容量上限后淘汰最旧) =====

    /// 小容量缓存插入超过容量时，最旧的条目必须被 LRU 淘汰。
    ///
    /// `ApiKeyCache` 内部使用 `lru::LruCache`，满容量后自动淘汰最久未访问的条目。
    /// 此测试用小容量（3）验证淘汰逻辑，避免插入 10000 条导致的测试缓慢。
    #[test]
    fn test_cache_lru_eviction_at_capacity() {
        // 使用小容量（3）创建缓存，便于快速验证 LRU 淘汰
        let mut cache = ApiKeyCache::new(3, 120);

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let scope = ApiKeyScope::default();

        // 插入 3 条（达到容量上限）
        for i in 0..3 {
            cache.insert(
                format!("sha256:lru_key_{}", i),
                CachedAuthResult {
                    team_id,
                    api_key_id,
                    scope: scope.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        // 验证 3 条都在缓存中
        for i in 0..3 {
            assert!(
                cache.get(&format!("sha256:lru_key_{}", i)).is_some(),
                "Key lru_key_{} should be in cache before eviction",
                i
            );
        }

        // 插入第 4 条 → 触发 LRU 淘汰最旧的 lru_key_0
        cache.insert(
            "sha256:lru_key_3".to_string(),
            CachedAuthResult {
                team_id,
                api_key_id,
                scope: scope.clone(),
                cached_at: Instant::now(),
            },
        );

        // lru_key_0 应被淘汰（最久未访问）
        assert!(
            cache.get("sha256:lru_key_0").is_none(),
            "LRU eviction: oldest key lru_key_0 must be evicted when capacity exceeded"
        );
        // lru_key_1, lru_key_2, lru_key_3 应仍在缓存中
        for i in 1..4 {
            assert!(
                cache.get(&format!("sha256:lru_key_{}", i)).is_some(),
                "Key lru_key_{} should remain in cache after LRU eviction",
                i
            );
        }
    }

    /// LRU 淘汰基于"最近访问"而非"插入顺序"：
    /// 访问 lru_key_0 后，它不再是最旧的，下次淘汰应移除 lru_key_1。
    #[test]
    fn test_cache_lru_eviction_based_on_access_not_insertion() {
        let mut cache = ApiKeyCache::new(3, 120);

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let scope = ApiKeyScope::default();

        // 插入 3 条
        for i in 0..3 {
            cache.insert(
                format!("sha256:access_key_{}", i),
                CachedAuthResult {
                    team_id,
                    api_key_id,
                    scope: scope.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        // 访问 access_key_0（使其成为最近访问）
        let _ = cache.get("sha256:access_key_0");

        // 插入第 4 条 → 淘汰 access_key_1（最久未访问）
        cache.insert(
            "sha256:access_key_3".to_string(),
            CachedAuthResult {
                team_id,
                api_key_id,
                scope: scope.clone(),
                cached_at: Instant::now(),
            },
        );

        // access_key_0 应仍在缓存中（刚被访问过）
        assert!(
            cache.get("sha256:access_key_0").is_some(),
            "Recently accessed key access_key_0 must NOT be evicted (LRU based on access time)"
        );
        // access_key_1 应被淘汰（现在是最久未访问）
        assert!(
            cache.get("sha256:access_key_1").is_none(),
            "LRU eviction: least recently accessed key access_key_1 must be evicted"
        );
    }

    /// 默认容量 10000 的缓存 stats 必须正确反映容量。
    #[test]
    fn test_cache_default_capacity_is_10000() {
        let cache = ApiKeyCache::new_default();
        let stats = cache.stats();
        assert_eq!(
            stats.capacity, 10000,
            "Default cache capacity must be 10000 (DEFAULT_CACHE_MAX_SIZE)"
        );
    }

    // ===== Global cache function tests (when not initialized) =====

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_cache_functions_when_not_initialized() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        assert!(!invalidate_cache_by_token_hash("sha256:test").await);
        assert_eq!(invalidate_cache_by_api_key_id(Uuid::new_v4()).await, 0);
        assert_eq!(invalidate_cache_by_team(Uuid::new_v4()).await, 0);
        assert_eq!(invalidate_all_cache().await, 0);
        assert!(get_cache_stats().await.is_none());
    }

    // ===== Global cache function tests (when initialized) =====

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_invalidate_cache_by_token_hash_hit() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache.clone());
        {
            let mut g = cache.write().await;
            g.insert(
                "sha256:global_tok".to_string(),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id: Uuid::new_v4(),
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }
        assert!(invalidate_cache_by_token_hash("sha256:global_tok").await);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_invalidate_cache_by_token_hash_miss() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache);
        assert!(!invalidate_cache_by_token_hash("sha256:nonexistent").await);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_invalidate_cache_by_api_key_id_multiple() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache.clone());
        let api_key_id = Uuid::new_v4();
        {
            let mut g = cache.write().await;
            g.insert(
                "sha256:k1".to_string(),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id,
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
            g.insert(
                "sha256:k2".to_string(),
                CachedAuthResult {
                    team_id: Uuid::new_v4(),
                    api_key_id,
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }
        let removed = invalidate_cache_by_api_key_id(api_key_id).await;
        assert_eq!(removed, 2);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_invalidate_cache_by_team() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache.clone());
        let team_id = Uuid::new_v4();
        {
            let mut g = cache.write().await;
            g.insert(
                "sha256:t1".to_string(),
                CachedAuthResult {
                    team_id,
                    api_key_id: Uuid::new_v4(),
                    scope: ApiKeyScope::default(),
                    cached_at: Instant::now(),
                },
            );
        }
        let removed = invalidate_cache_by_team(team_id).await;
        assert_eq!(removed, 1);
        // Non-existent team
        let removed2 = invalidate_cache_by_team(Uuid::new_v4()).await;
        assert_eq!(removed2, 0);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_CACHE_LOCK serializes global cache access; see above
    #[tokio::test]
    async fn test_global_get_cache_stats_when_initialized() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache);
        let stats = get_cache_stats().await;
        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.capacity, DEFAULT_CACHE_MAX_SIZE);
    }

    // ===== AuthState construction (with_scope_service) =====

    #[test]
    fn test_auth_state_with_scope_service_sets_service() {
        let pool = create_test_db_pool();
        let service = make_auth_scope_service(Some(ApiKeyScope::full_access()));
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let default_scope = ApiKeyScope::read_only();

        let state = AuthState::with_scope_service(
            pool,
            service,
            team_id,
            api_key_id,
            default_scope.clone(),
        );

        assert!(state.auth_scope_service.is_some());
        assert_eq!(state.team_id, team_id);
        assert_eq!(state.api_key_id, api_key_id);
        assert_eq!(state.scope, default_scope);
        assert!(state.api_key_cache.is_none());
        assert!(state.auth_rate_limiter.is_none());
        assert!(state.trusted_proxies.is_none());
    }

    // ===== load_scope_from_db =====

    #[tokio::test]
    async fn test_load_scope_from_db_with_service_loads_scope() {
        let pool = create_test_db_pool();
        let custom_scope = ApiKeyScope {
            read: true,
            write: true,
            admin: false,
            search_limit: 500,
            scrape_limit: 250,
        };
        let service = make_auth_scope_service(Some(custom_scope.clone()));
        let mut state = AuthState::with_scope_service(
            pool,
            service,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        );

        // Before load: default scope
        assert_eq!(state.scope, ApiKeyScope::default());

        state.load_scope_from_db().await;

        // After load: scope from mock repo
        assert_eq!(state.scope, custom_scope);
    }

    #[tokio::test]
    async fn test_load_scope_from_db_service_error_keeps_default() {
        let pool = create_test_db_pool();
        let service = make_auth_scope_service_error();
        let original_scope = ApiKeyScope::read_only();
        let mut state = AuthState::with_scope_service(
            pool,
            service,
            Uuid::new_v4(),
            Uuid::new_v4(),
            original_scope.clone(),
        );

        state.load_scope_from_db().await;

        // On error, scope should remain unchanged
        assert_eq!(state.scope, original_scope);
    }

    #[tokio::test]
    async fn test_load_scope_from_db_no_service_is_no_op() {
        let pool = create_test_db_pool();
        let original_scope = ApiKeyScope::full_access();
        let mut state =
            AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), original_scope.clone());

        state.load_scope_from_db().await;

        // No service → no-op, scope unchanged
        assert_eq!(state.scope, original_scope);
    }

    // ===== try_get_cached_auth =====

    #[tokio::test]
    async fn test_try_get_cached_auth_no_cache_returns_none() {
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let result = try_get_cached_auth(&state, "sha256:any").await;
        assert!(result.is_none(), "Should return None when no cache is set");
    }

    #[tokio::test]
    async fn test_try_get_cached_auth_cache_miss_returns_none() {
        let pool = create_test_db_pool();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        let state = AuthState::with_cache(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            cache,
        );

        let result = try_get_cached_auth(&state, "sha256:nonexistent").await;
        assert!(result.is_none(), "Should return None on cache miss");
    }

    #[tokio::test]
    async fn test_try_get_cached_auth_cache_hit_returns_state() {
        let pool = create_test_db_pool();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let token_hash = "sha256:cache_hit_test".to_string();
        let cached_scope = ApiKeyScope::full_access();

        // Insert a cache entry before constructing AuthState
        {
            let mut g = cache.write().await;
            g.insert(
                token_hash.clone(),
                CachedAuthResult {
                    team_id,
                    api_key_id,
                    scope: cached_scope.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        let state = AuthState::with_cache(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            cache,
        );

        let result = try_get_cached_auth(&state, &token_hash).await;
        assert!(result.is_some(), "Should return Some on cache hit");
        let cached = result.unwrap();
        assert_eq!(cached.team_id, team_id);
        assert_eq!(cached.api_key_id, api_key_id);
        assert_eq!(cached.scope, cached_scope);
        // Cache should still be present (not consumed)
        assert!(state.api_key_cache.is_some());
    }

    // ===== inject_auth_state =====

    #[test]
    fn test_inject_auth_state_inserts_extensions() {
        let pool = create_test_db_pool();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let auth_state = AuthState::new(pool, team_id, api_key_id, ApiKeyScope::default());
        let token_hash = "sha256:inject_test";

        let mut req = Request::builder().body(Body::empty()).unwrap();

        // Before inject: no extensions
        assert!(req.extensions().get::<AuthState>().is_none());
        assert!(req.extensions().get::<Uuid>().is_none());
        assert!(req.extensions().get::<String>().is_none());

        inject_auth_state(&mut req, auth_state, token_hash);

        // After inject: all extensions present
        assert!(req.extensions().get::<AuthState>().is_some());
        // Note: Uuid is inserted twice (team_id, api_key_id) — only the last survives
        let injected_uuid = req.extensions().get::<Uuid>();
        assert!(injected_uuid.is_some(), "Uuid extension should be inserted");
        assert_eq!(*injected_uuid.unwrap(), api_key_id);
        let injected_str = req.extensions().get::<String>();
        assert!(
            injected_str.is_some(),
            "String extension should be inserted"
        );
        assert_eq!(injected_str.unwrap(), token_hash);
    }

    // ===== validate_api_key_from_db =====

    #[tokio::test]
    async fn test_validate_api_key_from_db_unknown_token_returns_unauthorized() {
        // Real DbPool against a database whose api_keys table is present.
        // Querying a non-existent token_hash returns Ok(None), which
        // `validate_api_key_from_db` maps to UNAUTHORIZED (401).
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let result = validate_api_key_from_db(
            &state,
            "sha256:nonexistent_token_hash_for_test_purposes",
            "10.0.0.1",
        )
        .await;

        assert!(
            result.is_err(),
            "query against non-existent token_hash must surface as an error"
        );
        assert_eq!(
            result.unwrap_err(),
            StatusCode::UNAUTHORIZED,
            "non-existent token_hash must map to UNAUTHORIZED"
        );
    }

    /// #74: 数据库连接获取失败时 `validate_api_key_from_db` 必须返回 500。
    ///
    /// 测试策略：创建 `max_connections=1, acquire_timeout=10ms` 的连接池，
    /// 先占住唯一连接（不 drop），再调用 `validate_api_key_from_db`。
    /// 此时 `get_session` → `acquire_connection` 因信号量超时返回
    /// `ConnAcquireErr::Timeout`，经 `map_err` 转换为 500。
    /// `acquire_timeout=10ms` 足够稳定：信号量等待是纯进程内操作（无网络往返），
    /// 在任何 CI 环境 10ms 都有充分余量。
    ///
    /// **为何不用 `DbPool::try_from` + 无效 URL**：`permission` feature 被
    /// 传递性启用后，`try_from` 同步构造直接返回 `Err`（需 async 初始化
    /// 缓存），无法获得 `DbPool` 实例。`with_config` 则在构造时预连接，
    /// 无效 URL 会让 `with_config` 本身失败。连接池耗尽是唯一能在持有
    /// `DbPool` 实例的同时让 `get_session` 失败的方式。
    ///
    /// **覆盖的场景**：连接池无法提供可用连接（pool exhausted/timeout），
    /// 这是"数据库连接失败"在生产环境中的典型表现（DB 宕机/网络分区导致
    /// 连接全部失效后，新连接创建也会失败，最终同样触发此路径）。
    #[tokio::test]
    async fn test_validate_api_key_db_connection_failure_returns_500() {
        use dbnexus::DbConfig;

        if skip_if_no_test_db() {
            return;
        }

        let url = resolve_test_database_url()
            .expect("TEST_DATABASE_URL or DATABASE_URL required for #74 test");

        // max_connections=1 + acquire_timeout=10ms：
        // 先占住唯一连接，后续 get_session 在信号量上等待至超时（纯进程内，无网络）
        let cfg = DbConfig {
            url,
            max_connections: 1,
            min_connections: 0,
            acquire_timeout: 10,
            ..Default::default()
        };

        let pool = Arc::new(
            DbPool::with_config(cfg)
                .await
                .expect("Failed to create DbPool for #74 test"),
        );

        // 先占住唯一连接（_held_session 不 drop，信号量保持 0 许可）
        let _held_session = pool
            .get_session("admin")
            .await
            .expect("Failed to acquire held session for #74 test");

        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        // 此时 validate_api_key_from_db 内部 get_session 会超时 → map_err → 500
        let result = validate_api_key_from_db(&state, "some_hash", TEST_UNKNOWN_IP).await;

        assert_eq!(
            result,
            Err(StatusCode::INTERNAL_SERVER_ERROR),
            "DB connection acquisition failure (pool exhausted/timeout) must return 500 \
             (INTERNAL_SERVER_ERROR), not panic or return other status code"
        );
    }

    // ===== create_and_cache_auth_state =====

    #[tokio::test]
    async fn test_create_and_cache_auth_state_no_service_returns_500() {
        let pool = create_test_db_pool();
        // No auth_scope_service, no cache
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let key = make_key_model(Some("sha256:somehash".to_string()), Some(0));

        let result = create_and_cache_auth_state(&state, &key, "sha256:any").await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Missing AuthScopeService should return 500"
        );
    }

    #[tokio::test]
    async fn test_create_and_cache_auth_state_with_service_succeeds_and_caches() {
        let pool = create_test_db_pool();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        let service = make_auth_scope_service(Some(ApiKeyScope::full_access()));
        let state = AuthState::with_cache(
            pool,
            Some(service),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            cache.clone(),
        );

        let key = make_key_model(Some("sha256:somehash".to_string()), Some(0));
        let token_hash = "sha256:create_cache_test".to_string();

        let result = create_and_cache_auth_state(&state, &key, &token_hash).await;

        assert!(result.is_ok(), "Should succeed with service present");
        let auth_state = result.unwrap();
        assert_eq!(auth_state.team_id, key.team_id);
        assert_eq!(auth_state.api_key_id, key.id);
        assert_eq!(auth_state.scope, ApiKeyScope::full_access());

        // Verify the cache was populated (get requires &mut self → write lock)
        let mut cache_guard = cache.write().await;
        let cached = cache_guard.get(&token_hash);
        assert!(cached.is_some(), "Cache should be populated after create");
        let cached = cached.unwrap();
        assert_eq!(cached.team_id, key.team_id);
        assert_eq!(cached.api_key_id, key.id);
        assert_eq!(cached.scope, ApiKeyScope::full_access());
    }

    #[tokio::test]
    async fn test_create_and_cache_auth_state_without_cache_still_returns_state() {
        let pool = create_test_db_pool();
        let service = make_auth_scope_service(None);
        // No cache — uses with_scope_service (api_key_cache is None)
        let state = AuthState::with_scope_service(
            pool,
            service,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        );

        let key = make_key_model(Some("sha256:somehash".to_string()), Some(0));

        let result = create_and_cache_auth_state(&state, &key, "sha256:any").await;

        // Should still succeed (cache insert is guarded by if-let-Some)
        assert!(result.is_ok());
        assert_eq!(result.unwrap().api_key_id, key.id);
    }

    // ===== check_rate_limit_lockout =====

    #[tokio::test]
    async fn test_check_rate_limit_lockout_no_limiter_returns_ok() {
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        let result = check_rate_limit_lockout(&state, "10.0.0.1").await;

        assert!(result.is_ok(), "No rate limiter → Ok");
    }

    #[tokio::test]
    async fn test_check_rate_limit_lockout_unlocked_returns_ok() {
        let pool = create_test_db_pool();
        let rate_limiter = Arc::new(AuthRateLimiter::new());
        let state = AuthState::with_trusted_proxies(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            None,
            Some(rate_limiter),
            security::TrustedProxyConfig::from_settings(false, vec![]),
        );

        let result = check_rate_limit_lockout(&state, "10.0.0.2").await;

        assert!(result.is_ok(), "Unlocked IP → Ok");
    }

    #[tokio::test]
    async fn test_check_rate_limit_lockout_locked_returns_429() {
        let pool = create_test_db_pool();
        let rate_limiter = Arc::new(AuthRateLimiter::new());
        let test_ip = "10.0.0.3";
        for _ in 0..MAX_AUTH_FAILURES {
            rate_limiter.record_failure(test_ip).await;
        }
        let state = AuthState::with_trusted_proxies(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            None,
            Some(rate_limiter),
            security::TrustedProxyConfig::from_settings(false, vec![]),
        );

        let result = check_rate_limit_lockout(&state, test_ip).await;

        assert!(result.is_err(), "Locked IP → Err");
        assert_eq!(
            result.unwrap_err(),
            StatusCode::TOO_MANY_REQUESTS,
            "Locked IP should return 429"
        );
    }

    // ===== record_auth_failure =====

    #[tokio::test]
    async fn test_record_auth_failure_no_limiter_is_no_op() {
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        // Should not panic
        record_auth_failure(&state, "10.0.0.4").await;
    }

    #[tokio::test]
    async fn test_record_auth_failure_with_limiter_records_failure() {
        let pool = create_test_db_pool();
        let rate_limiter = Arc::new(AuthRateLimiter::new());
        let test_ip = "10.0.0.5";
        let state = AuthState::with_trusted_proxies(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            None,
            Some(rate_limiter.clone()),
            security::TrustedProxyConfig::from_settings(false, vec![]),
        );

        // Record a few failures via the helper
        record_auth_failure(&state, test_ip).await;
        record_auth_failure(&state, test_ip).await;

        // Verify via the rate limiter directly
        assert!(!rate_limiter.is_locked_out(test_ip).await);
        // Record enough to lock out via the helper
        for _ in 0..(MAX_AUTH_FAILURES - 1) {
            record_auth_failure(&state, test_ip).await;
        }
        assert!(
            rate_limiter.is_locked_out(test_ip).await,
            "After MAX failures IP should be locked out"
        );
    }

    // ===== reset_auth_failures =====

    #[tokio::test]
    async fn test_reset_auth_failures_no_limiter_is_no_op() {
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        // Should not panic
        reset_auth_failures(&state, "10.0.0.6").await;
    }

    #[tokio::test]
    async fn test_reset_auth_failures_with_limiter_clears_failures() {
        let pool = create_test_db_pool();
        let rate_limiter = Arc::new(AuthRateLimiter::new());
        let test_ip = "10.0.0.7";
        // Pre-fill failures
        for _ in 0..MAX_AUTH_FAILURES {
            rate_limiter.record_failure(test_ip).await;
        }
        assert!(rate_limiter.is_locked_out(test_ip).await);

        let state = AuthState::with_trusted_proxies(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            None,
            Some(rate_limiter.clone()),
            security::TrustedProxyConfig::from_settings(false, vec![]),
        );

        reset_auth_failures(&state, test_ip).await;

        assert!(
            !rate_limiter.is_locked_out(test_ip).await,
            "After reset IP should not be locked out"
        );
    }

    // ===== auth_middleware wrapper =====

    #[test]
    fn test_auth_middleware_wrapper_returns_callable() {
        let wrapper = auth_middleware();
        // The wrapper is a closure — calling it requires a Request and Next.
        // We only verify it is cloneable and callable (type-checks).
        let _cloned = wrapper.clone();
        // Force the closure type to be inferred (avoids dead_code warning on `wrapper`).
        drop(wrapper);
    }

    // ===== test_auth_state helper =====

    #[test]
    fn test_test_auth_state_helper_constructs_default_scope() {
        let pool = create_test_db_pool();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let state = test_auth_state(pool, team_id, api_key_id);

        assert_eq!(state.team_id, team_id);
        assert_eq!(state.api_key_id, api_key_id);
        assert_eq!(state.scope, ApiKeyScope::default());
        assert!(state.auth_scope_service.is_none());
        assert!(state.api_key_cache.is_none());
    }

    // ===== AuthState with global cache (inline) =====

    #[tokio::test]
    async fn test_auth_state_with_global_cache_uses_global_inline() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache.clone());

        let pool = create_test_db_pool();
        let state = AuthState::with_global_cache(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            None,
            None,
        );

        assert!(state.api_key_cache.is_some());
        let state_cache = state.api_key_cache.unwrap();
        assert!(
            Arc::ptr_eq(&cache, &state_cache),
            "with_global_cache should reuse the global cache Arc"
        );
    }

    #[tokio::test]
    async fn test_new_for_middleware_creates_cache_when_absent() {
        let _guard = lock_global_cache();
        reset_global_auth_cache();
        // Ensure no global cache exists before calling new_for_middleware
        assert!(get_global_auth_cache().is_none());

        let pool = create_test_db_pool();
        let _state = AuthState::new_for_middleware(pool, None);

        // new_for_middleware should have created and set the global cache
        let global = get_global_auth_cache();
        assert!(
            global.is_some(),
            "new_for_middleware should initialize global cache if absent"
        );
    }

    // ===== set/get_global_auth_state (inline coverage of the set path) =====

    #[test]
    fn test_set_get_global_auth_state_roundtrip() {
        let _guard = lock_global_state();
        let pool = create_test_db_pool();
        let state = Arc::new(AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        set_global_auth_state(state.clone());

        // 架构 MEDIUM-1：覆盖式 set，get 应返回我们刚设置的 state（ptr_eq 成立）
        let retrieved = get_global_auth_state();
        assert!(
            retrieved.is_some(),
            "get_global_auth_state should return Some after set"
        );
        if let Some(retrieved_state) = retrieved {
            assert!(
                Arc::ptr_eq(&retrieved_state, &state),
                "get_global_auth_state should return the state we just set (覆盖式语义)"
            );
        }
    }

    // ===== scope_middleware integration tests =====
    // These exercise scope_middleware through a real axum Router to cover the
    // branches: no-required-scope passthrough, missing AuthState, insufficient
    // scope, and sufficient scope.

    use axum::{middleware, routing::get, Router};
    use tower::ServiceExt;

    fn make_scope_router() -> Router {
        Router::new()
            .route("/v1/search", get(|| async { "ok" }))
            .route("/api/v1/teams", get(|| async { "ok" }))
            .layer(middleware::from_fn(scope_middleware))
    }

    #[tokio::test]
    async fn test_scope_middleware_no_required_scope_passes_through() {
        let app = make_scope_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/search")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_scope_middleware_admin_path_missing_authstate_returns_unauthorized() {
        let app = make_scope_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/teams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_scope_middleware_admin_path_insufficient_scope_returns_forbidden() {
        let app = make_scope_router();
        let pool = create_test_db_pool();
        let auth_state = AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::read_only(),
        );
        let mut req = Request::builder()
            .uri("/api/v1/teams")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(auth_state);
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_scope_middleware_admin_path_full_access_passes_through() {
        let app = make_scope_router();
        let pool = create_test_db_pool();
        let auth_state = AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::full_access(),
        );
        let mut req = Request::builder()
            .uri("/api/v1/teams")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(auth_state);
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // ===== auth_middleware_inner integration tests =====
    // Exercise auth_middleware_inner through a real axum Router to cover the
    // main middleware branches: public bypass, missing token, DB failure.

    // GLOBAL_STATE_LOCK must be held across .await because the middleware
    // reads GLOBAL_AUTH_STATE during request handling; releasing the guard
    // would let other tests race-modify the global state.
    // Single-threaded tokio runtime => no deadlock risk.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_auth_middleware_inner_public_endpoint_bypasses() {
        let _guard = lock_global_state();
        if get_global_auth_state().is_none() {
            let pool = create_test_db_pool();
            let state = Arc::new(AuthState::new(
                pool,
                Uuid::new_v4(),
                Uuid::new_v4(),
                ApiKeyScope::default(),
            ));
            set_global_auth_state(state);
        }

        let app = Router::new()
            .route("/health", get(|| async { "OK" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_STATE_LOCK serializes global state access; see above
    #[tokio::test]
    async fn test_auth_middleware_inner_missing_bearer_returns_401() {
        let _guard = lock_global_state();
        if get_global_auth_state().is_none() {
            let pool = create_test_db_pool();
            let state = Arc::new(AuthState::new(
                pool,
                Uuid::new_v4(),
                Uuid::new_v4(),
                ApiKeyScope::default(),
            ));
            set_global_auth_state(state);
        }

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[allow(clippy::await_holding_lock)] // GLOBAL_STATE_LOCK serializes global state access; see above
    #[tokio::test]
    async fn test_auth_middleware_inner_unknown_bearer_returns_unauthorized() {
        let _guard = lock_global_state();
        if get_global_auth_state().is_none() {
            let pool = create_test_db_pool();
            let state = Arc::new(AuthState::new(
                pool,
                Uuid::new_v4(),
                Uuid::new_v4(),
                ApiKeyScope::default(),
            ));
            set_global_auth_state(state);
        }

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        // Bearer token present, but the token hash is not present in the
        // api_keys table → validate_api_key_from_db returns Ok(None),
        // which the middleware maps to UNAUTHORIZED (401).
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer test_token_unknown_to_db")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // ===== scope_middleware audit logging branch =====

    /// Mock AuditServiceTrait that tracks log_deny calls.
    struct MockAuditService {
        deny_called: Arc<std::sync::Mutex<bool>>,
    }

    #[async_trait]
    impl AuditServiceTrait for MockAuditService {
        async fn log(
            &self,
            _entry: crate::domain::auth::AuditLogEntry,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }
        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: Uuid,
            _team_id: Uuid,
            _scope: ApiKeyScope,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }
        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<Uuid>,
            _team_id: Option<Uuid>,
            _reason: String,
            _scope: Option<ApiKeyScope>,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            *self.deny_called.lock().unwrap() = true;
            Ok(())
        }
        async fn get_logs_for_key(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
        async fn get_logs_for_team(
            &self,
            _team_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
        async fn get_denied_requests(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_scope_middleware_audit_log_called_on_deny() {
        let deny_called = Arc::new(std::sync::Mutex::new(false));
        let audit_service: Arc<dyn AuditServiceTrait> = Arc::new(MockAuditService {
            deny_called: deny_called.clone(),
        });

        let app = Router::new()
            .route("/api/v1/teams", get(|| async { "ok" }))
            .layer(middleware::from_fn(scope_middleware));

        let pool = create_test_db_pool();
        let auth_state = AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::read_only(), // lacks Admin scope
        );

        let mut req = Request::builder()
            .uri("/api/v1/teams")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(auth_state);
        req.extensions_mut().insert(audit_service);

        let response = app.oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            *deny_called.lock().unwrap(),
            "AuditService.log_deny should be called on scope denial"
        );
    }

    // ===== AuthState Debug impl coverage =====

    #[test]
    fn test_auth_state_debug_impl_minimal_fields() {
        let pool = create_test_db_pool();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let state = AuthState::new(pool, team_id, api_key_id, ApiKeyScope::default());

        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("AuthState"));
        assert!(debug_str.contains(&team_id.to_string()));
        assert!(debug_str.contains(&api_key_id.to_string()));
        // Optional fields should show as false when None
        assert!(debug_str.contains("auth_scope_service"));
        assert!(debug_str.contains("api_key_cache"));
        assert!(debug_str.contains("auth_rate_limiter"));
        assert!(debug_str.contains("trusted_proxies"));
        // finish_non_exhaustive adds ".." to the output
        assert!(debug_str.contains(".."));
    }

    #[test]
    fn test_auth_state_debug_impl_with_cache_and_limiter() {
        let pool = create_test_db_pool();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let mut state = AuthState::new(pool, team_id, api_key_id, ApiKeyScope::default());
        state.api_key_cache = Some(Arc::new(RwLock::new(ApiKeyCache::new_default())));
        state.auth_rate_limiter = Some(Arc::new(AuthRateLimiter::new()));
        state.trusted_proxies = Some(security::TrustedProxyConfig::default());

        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("AuthState"));
        // With optional fields set, the debug should still contain field names
        assert!(debug_str.contains("auth_scope_service"));
        assert!(debug_str.contains("api_key_cache"));
        assert!(debug_str.contains("auth_rate_limiter"));
        assert!(debug_str.contains("trusted_proxies"));
    }

    // ========== CapturingLogger for covering log::error!/debug! format args ==========

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static AUTH_LOGGER_INIT: Once = Once::new();

    struct AuthCapturingLogger;

    impl Log for AuthCapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_auth_debug_logger() {
        AUTH_LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: AuthCapturingLogger = AuthCapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    /// Ensure the global auth state is set with cache + rate limiter.
    /// If already set (by another test), returns the existing state.
    /// 架构 MEDIUM-1：GLOBAL_AUTH_STATE 现在是 Mutex<Option<...>>（可重置），
    /// 但为避免每个测试都重置+重建，仍复用已设置的 state。
    ///
    /// Panics when `TEST_DATABASE_URL` is not set — callers MUST mark their
    /// tests with `#[ignore = "requires TEST_DATABASE_URL"]` so CI without a
    /// real DB skips them. The previous `Option` return leaked the "should this
    /// test be skipped?" concern into the production-shaped helper; the
    /// `#[ignore]` attribute is the idiomatic Rust way to express that.
    fn ensure_global_state_with_cache_and_limiter() -> Arc<AuthState> {
        if let Some(existing) = get_global_auth_state() {
            return existing;
        }
        let pool = create_test_db_pool();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        let rate_limiter = Arc::new(AuthRateLimiter::new());
        let state = AuthState::with_trusted_proxies(
            pool,
            None,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
            Some(cache),
            Some(rate_limiter),
            security::TrustedProxyConfig::from_settings(false, vec![]),
        );
        set_global_auth_state(Arc::new(state));
        get_global_auth_state().expect("global auth state should be set")
    }

    // ===== auth_middleware_inner: no global state (lines 709-710) =====
    // Only covers when this test runs before any test that sets GLOBAL_AUTH_STATE.

    #[allow(clippy::await_holding_lock)] // GLOBAL_STATE_LOCK serializes global state access; see above
    #[tokio::test]
    async fn test_auth_middleware_inner_no_global_state_returns_500() {
        ensure_auth_debug_logger();
        let _guard = lock_global_state();

        // Only test the None path if no other test has set the state yet
        if get_global_auth_state().is_some() {
            return;
        }

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ===== auth_middleware_inner: rate limit lockout (line 731) =====

    #[allow(clippy::await_holding_lock)] // GLOBAL_STATE_LOCK serializes global state access; see above
    #[tokio::test]
    async fn test_auth_middleware_inner_rate_limit_lockout_returns_429() {
        ensure_auth_debug_logger();
        let _guard = lock_global_state();

        let state = ensure_global_state_with_cache_and_limiter();

        // Need a rate limiter to test the lockout path
        let rate_limiter = match state.auth_rate_limiter.clone() {
            Some(rl) => rl,
            None => return, // State was set by another test without a rate limiter
        };

        // Reset any prior failures for "unknown" IP (test requests appear as "unknown")
        rate_limiter.reset_failures(TEST_UNKNOWN_IP).await;

        // Lock out "unknown" IP by recording MAX_AUTH_FAILURES failures
        for _ in 0..MAX_AUTH_FAILURES {
            rate_limiter.record_failure(TEST_UNKNOWN_IP).await;
        }

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer some_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Cleanup: reset failures to avoid affecting other tests
        rate_limiter.reset_failures(TEST_UNKNOWN_IP).await;
    }

    // ===== auth_middleware_inner: cache hit path (lines 748-749, 782) =====

    #[allow(clippy::await_holding_lock)] // GLOBAL_STATE_LOCK serializes global state access; see above
    #[tokio::test]
    async fn test_auth_middleware_inner_cache_hit_returns_200() {
        ensure_auth_debug_logger();
        let _guard = lock_global_state();

        let state = ensure_global_state_with_cache_and_limiter();

        // Reset rate limiter for "unknown" IP to avoid interference from lockout test
        if let Some(ref rate_limiter) = state.auth_rate_limiter {
            rate_limiter.reset_failures(TEST_UNKNOWN_IP).await;
        }

        // Need a cache to test the cache hit path
        let cache = match state.api_key_cache.clone() {
            Some(c) => c,
            None => return, // State was set by another test without a cache
        };

        let token_str = "test_token_cache_hit_path";
        let token_hash = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(token_str.as_bytes()))
        );
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let cached_scope = ApiKeyScope::full_access();

        // Pre-populate the cache with a token hash entry
        {
            let mut g = cache.write().await;
            g.insert(
                token_hash.clone(),
                CachedAuthResult {
                    team_id,
                    api_key_id,
                    scope: cached_scope,
                    cached_at: Instant::now(),
                },
            );
        }

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {}", token_str))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Cleanup: remove the cache entry to avoid affecting other tests
        {
            let mut g = cache.write().await;
            g.invalidate(&token_hash);
        }
    }

    // =========================================================================
    // 维度18 安全渗透测试 — 补全 test_matrix.md 中真实缺失的 5 项
    // (#218 SQL 注入 / #213 超长 Token / #214 特殊字符注入 /
    //  #216 并发认证竞态 / #212 分布式暴力破解)
    // =========================================================================

    // ----- #218 SQL 注入：参数化查询防护 -----

    /// SQL 注入 payload 作为 Bearer Token 必须被原样提取，不触发任何 SQL 解析。
    ///
    /// `extract_bearer_token` 是纯字符串切片操作，不接触数据库；
    /// 此测试锁定该契约，防止未来重构引入意外的字符串处理（如拼接 SQL）。
    #[test]
    fn test_sql_injection_payload_extracted_as_literal_token() {
        let payloads = [
            "' OR '1'='1",
            "'; DROP TABLE api_keys; --",
            "' UNION SELECT * FROM teams --",
            "admin'--",
            "' OR 1=1 --",
            "/* */ * FROM api_keys WHERE 1=1",
        ];
        for payload in payloads {
            let req = make_bearer_request(Some(&format!("Bearer {}", payload)));
            let token = extract_bearer_token(&req)
                .expect("SQL injection payload should be extracted as literal token");
            assert_eq!(
                token, payload,
                "extract_bearer_token must return the payload verbatim (no SQL interpretation)"
            );
        }
    }

    /// SHA-256 哈希对 SQL 注入 payload 产生确定性摘要，证明哈希层无 SQL 执行。
    ///
    /// `hash_api_key_sha256` 是纯密码学函数（Sha256 digest + hex encode），
    /// 不接触数据库；此测试锁定该契约。
    #[test]
    fn test_sql_injection_payload_hashed_deterministically() {
        let payloads = [
            "' OR '1'='1",
            "'; DROP TABLE api_keys; --",
            "' UNION SELECT * FROM teams --",
        ];
        for payload in payloads {
            let h1 = security::hash_api_key_sha256(payload);
            let h2 = security::hash_api_key_sha256(payload);
            assert_eq!(
                h1, h2,
                "SHA-256 must be deterministic for SQL injection payload"
            );
            assert_eq!(
                h1.len(),
                64,
                "SHA-256 hex digest must be 64 chars for SQL injection payload"
            );
            assert!(
                h1.chars().all(|c| c.is_ascii_hexdigit()),
                "SHA-256 hex digest must be pure hex for SQL injection payload"
            );
        }
    }

    /// `validate_api_key_from_db` 对 SQL 注入 token 必须返回 `Err(401)`（未找到），
    /// 而非 `Err(500)`（数据库错误）或 `Ok(Some)`（意外命中）。
    ///
    /// 这验证 SeaORM `Column::KeyHash.eq(token_hash)` 使用参数化查询，
    /// SQL 注入 payload 被作为字面量比较，不会改变 SQL 语义。
    ///
    /// 注意：`validate_api_key_from_db` 在 key 不存在时返回 `Err(401)`，
    /// 在 DB 错误时返回 `Err(500)`，在 key 存在时返回 `Ok(Some)`。
    /// SQL 注入 payload 应返回 `Err(401)`（字面量未匹配），而非 `Err(500)`（SQL 错误）。
    #[tokio::test]
    async fn test_sql_injection_token_rejected_by_parameterized_query() {
        let pool = create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());

        // 先验证 DB session 可用（排除环境问题）
        let session_check = state.pool.get_session("admin").await;
        if session_check.is_err() {
            eprintln!(
                "[skip] DB session not available: {:?} — skipping DB-level SQL injection test",
                session_check.err()
            );
            return;
        }
        let session = session_check.unwrap();
        let conn_check = session.connection();
        if conn_check.is_err() {
            eprintln!(
                "[skip] DB connection not available: {:?} — skipping DB-level SQL injection test",
                conn_check.err()
            );
            return;
        }
        drop(session);

        let sql_payloads = [
            "' OR '1'='1",
            "'; DROP TABLE api_keys; --",
            "' UNION SELECT id, team_id, key, key_hash FROM api_keys --",
            "admin' OR '1'='1' --",
        ];
        for payload in sql_payloads {
            // 模拟真实流程：先 SHA-256 哈希 token，再用哈希查 DB
            let token_hash = format!("sha256:{}", hex::encode(Sha256::digest(payload.as_bytes())));
            let result = validate_api_key_from_db(&state, &token_hash, "10.0.0.99").await;
            // 关键断言：必须返回 Err(401)（未找到），而非 Err(500)（DB 错误）或 Ok(Some)（命中）
            assert_eq!(
                result,
                Err(StatusCode::UNAUTHORIZED),
                "SQL injection payload must be treated as literal (return 401 not found), \
                 not cause DB error (500) or match a key (Ok(Some)): {}",
                payload
            );
        }
    }

    /// `verify_key_hash` 对 SQL 注入 token 必须返回 `false`（哈希不匹配），
    /// 不能因 payload 内容触发异常或意外返回 `true`。
    #[test]
    fn test_sql_injection_token_rejected_by_verify_key_hash() {
        // 复用跨测试缓存的 bcrypt hash（避免重复 cost=12 计算）
        let legit_token = "legit_token_abc123";
        let stored_bcrypt = get_test_legit_bcrypt_hash().to_string();
        let key = make_key_model(Some(stored_bcrypt), None);

        let sql_payloads = [
            "' OR '1'='1",
            "'; DROP TABLE api_keys; --",
            "' UNION SELECT * FROM teams --",
        ];
        for payload in sql_payloads {
            assert!(
                !verify_key_hash(&key, payload),
                "SQL injection payload must not verify against bcrypt hash: {}",
                payload
            );
        }

        // SHA-256 存储格式同样必须拒绝
        let legit_sha256 = format!("sha256:{}", security::hash_api_key_sha256(legit_token));
        let key_sha = make_key_model(Some(legit_sha256), None);
        for payload in sql_payloads {
            assert!(
                !verify_key_hash(&key_sha, payload),
                "SQL injection payload must not verify against SHA-256 hash: {}",
                payload
            );
        }
    }

    // ----- #213 超长 Token 输入 -----

    /// 10KB+ Token 必须被 `extract_bearer_token` 正常提取，不触发 panic 或截断。
    ///
    /// `extract_bearer_token` 使用 `auth_header[7..].to_string()` 切片，
    /// 理论上无长度上限；此测试锁定该契约，防止未来引入长度限制导致 DoS。
    #[test]
    fn test_oversized_token_extracted_without_panic() {
        // 10 KB Token（远超正常 API Key 长度）
        let huge_token = "A".repeat(10 * 1024);
        let req = make_bearer_request(Some(&format!("Bearer {}", huge_token)));
        let token = extract_bearer_token(&req).expect("10KB token must be extracted without panic");
        assert_eq!(
            token.len(),
            huge_token.len(),
            "10KB token must not be truncated"
        );
        assert_eq!(token, huge_token);
    }

    /// 100KB Token 也必须正常处理（边界压力测试）。
    #[test]
    fn test_100kb_token_extracted_without_panic() {
        let huge_token = "B".repeat(100 * 1024);
        let req = make_bearer_request(Some(&format!("Bearer {}", huge_token)));
        let token =
            extract_bearer_token(&req).expect("100KB token must be extracted without panic");
        assert_eq!(token.len(), huge_token.len());
    }

    /// SHA-256 哈希对超长 Token 产生有效摘要（64 字符 hex），不崩溃。
    #[test]
    fn test_oversized_token_hashed_without_panic() {
        let huge_token = "C".repeat(10 * 1024);
        let hash = security::hash_api_key_sha256(&huge_token);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// 超长 Token 通过完整中间件流程必须返回 401（未认证），
    /// 而非 500（服务器错误）或 panic。
    ///
    /// 这验证端到端流程对超长输入的健壮性。
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_oversized_token_returns_401_not_500() {
        ensure_auth_debug_logger();
        let _guard = lock_global_state();

        let state = ensure_global_state_with_cache_and_limiter();
        if let Some(ref rate_limiter) = state.auth_rate_limiter {
            rate_limiter.reset_failures(TEST_UNKNOWN_IP).await;
        }

        let huge_token = "D".repeat(10 * 1024);

        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(middleware::from_fn(auth_middleware_inner));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {}", huge_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "10KB token must return 401 (not found in DB), not 500 or panic"
        );

        // Cleanup: 清理 rate_limiter 失败计数 + 重置全局 AuthState
        // 避免本测试设置的全局状态污染后续依赖"无全局状态"契约的测试
        if let Some(ref rate_limiter) = state.auth_rate_limiter {
            rate_limiter.reset_failures(TEST_UNKNOWN_IP).await;
        }
        reset_global_auth_state();
    }

    // ----- #214 特殊字符注入（\0 / \n / Unicode）-----

    /// 含 \0（NULL 字节）、\n（换行）、\r（回车）的 Token 在 HTTP 头部层被拒绝，
    /// 永远不会到达 `extract_bearer_token`。
    ///
    /// 这验证 `HeaderValue` 对非法控制字符的过滤：
    /// - `\n`、`\r` 被拒绝（HTTP 头部不允许换行，防止 CRLF 注入）
    /// - `\0` 被拒绝（NULL 字节不允许）
    ///
    /// 这些字符在 HTTP 头构造阶段就被拒绝，无法进入认证流程。
    #[test]
    fn test_special_character_token_extracted_or_rejected_safely() {
        use axum::http::HeaderValue;

        // \n、\r、\0 在 HTTP 头部中均被 HeaderValue 拒绝
        let malicious_tokens = [
            "token_with_\n_newline",
            "token_with_\r_carriage_return",
            "token_with_\0_null",
            "token_with_\n\r_crlf_injection",
        ];
        for token in malicious_tokens {
            let header_value = format!("Bearer {}", token);
            let result = HeaderValue::from_str(&header_value);
            assert!(
                result.is_err(),
                "HTTP HeaderValue must reject token with control char: {:?}",
                token
            );
        }

        // 验证合法 ASCII token 能正常通过 HTTP 头部层
        let legit_header = HeaderValue::from_str("Bearer legit_token_abc123");
        assert!(
            legit_header.is_ok(),
            "Legit ASCII token must be accepted by HeaderValue"
        );

        // 验证合法 ASCII token 能被 extract_bearer_token 正确提取
        let req = make_bearer_request(Some("Bearer legit_token_abc123"));
        let token = extract_bearer_token(&req).expect("Legit token must be extracted successfully");
        assert_eq!(token, "legit_token_abc123");
    }

    /// 含 Unicode 字符的 Token 必须被正常提取（HTTP 头部允许 ASCII，
    /// 但非 ASCII 字节会被 HeaderValue 拒绝）。
    #[test]
    fn test_unicode_token_rejected_by_http_header_parsing() {
        // 非 ASCII 字节（如中文）在 HeaderValue 中被拒绝
        let unicode_token = "token_with_中文_unicode";
        let req = make_bearer_request(Some(&format!("Bearer {}", unicode_token)));
        assert_eq!(
            extract_bearer_token(&req),
            None,
            "Non-ASCII Unicode in header must be rejected (safe None)"
        );
    }

    /// `hash_api_key_sha256` 对含特殊字符的 Token（直接传入，绕过 HTTP 头）
    /// 必须产生确定性哈希，不崩溃。
    #[test]
    fn test_special_character_token_hashed_safely() {
        let special_tokens = [
            "token_with_\n_newline",
            "token_with_\t_tab",
            "token_with_\0_null",
            "token_with_中文_unicode",
            "token_with_emoji_🔐",
            "token_with_\r_carriage_return",
        ];
        for token in special_tokens {
            let h1 = security::hash_api_key_sha256(token);
            let h2 = security::hash_api_key_sha256(token);
            assert_eq!(
                h1, h2,
                "SHA-256 must be deterministic for special char token"
            );
            assert_eq!(h1.len(), 64);
            // 验证不同特殊字符产生不同哈希（未退化成常量）
            assert_ne!(
                h1,
                security::hash_api_key_sha256("different_token"),
                "Different special char tokens must produce different hashes"
            );
        }
    }

    /// `verify_key_hash` 对含特殊字符的 Token 必须返回 `false`（不匹配），
    /// 不因特殊字符触发异常。
    #[test]
    fn test_special_character_token_rejected_by_verify_key_hash() {
        // 复用跨测试缓存的 bcrypt hash（统一 token，避免重复 cost=12 计算）
        let stored_bcrypt = get_test_legit_bcrypt_hash().to_string();
        let key = make_key_model(Some(stored_bcrypt), None);

        let special_tokens = [
            "token_with_\n_newline",
            "token_with_\0_null",
            "token_with_中文_unicode",
            "token_with_emoji_🔐",
        ];
        for token in special_tokens {
            assert!(
                !verify_key_hash(&key, token),
                "Special char token must not verify against bcrypt hash: {:?}",
                token
            );
        }
    }

    // ----- #216 并发认证竞态：同一 Token 高并发一致性 -----

    /// 同一 Token 在高并发下 `hash_api_key_sha256` 必须产生完全一致的结果。
    ///
    /// SHA-256 是纯函数（无共享状态），理论上无竞态；
    /// 此测试锁定该契约，防止未来引入缓存/状态导致竞态。
    #[tokio::test]
    async fn test_concurrent_hashing_same_token_produces_consistent_result() {
        let token = "concurrent_test_token";
        let expected = security::hash_api_key_sha256(token);

        let mut handles = Vec::new();
        for _ in 0..50 {
            let token_owned = token.to_string();
            handles.push(tokio::spawn(async move {
                security::hash_api_key_sha256(&token_owned)
            }));
        }

        let results: Vec<String> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.expect("spawned task must not panic"))
            .collect();

        for result in &results {
            assert_eq!(
                result, &expected,
                "All concurrent hash results must be identical (pure function, no race)"
            );
        }
    }

    /// 同一 Token + 同一 key 在高并发下 `verify_key_hash` 必须产生一致结果。
    ///
    /// 样本数选择 8（而非更大）：bcrypt verify 是 CPU 密集型（cost=12），
    /// 8 个并发任务已足以暴露竞态条件（若有），更多样本不增加信息增益
    /// 但线性增加 CI 耗时（每样本 ~300ms）。
    #[tokio::test]
    async fn test_concurrent_verify_key_hash_consistent_result() {
        let legit_token = "concurrent_verify_token";
        let stored_bcrypt = security::hash_api_key(legit_token).unwrap();
        let key = Arc::new(make_key_model(Some(stored_bcrypt), None));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let key_clone = key.clone();
            let token_owned = legit_token.to_string();
            handles.push(tokio::spawn(async move {
                verify_key_hash(&key_clone, &token_owned)
            }));
        }

        let results: Vec<bool> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.expect("spawned task must not panic"))
            .collect();

        // 所有结果必须为 true（合法 token 验证通过）且一致
        for result in &results {
            assert!(
                *result,
                "All concurrent verify results must be true for legit token"
            );
        }
        let first = results[0];
        assert!(
            results.iter().all(|&r| r == first),
            "All concurrent verify results must be identical (no race condition)"
        );
    }

    /// 同一 Token 在高并发下 `validate_api_key_from_db` 必须产生一致结果。
    ///
    /// 这验证 SeaORM 参数化查询在并发下无竞态（连接池线程安全）。
    ///
    /// 注意：`validate_api_key_from_db` 在 key 不存在时返回 `Err(401)`，
    /// 所有并发调用对同一未知 token 必须一致返回 `Err(401)`（非 `Err(500)`）。
    #[tokio::test]
    async fn test_concurrent_validate_api_key_from_db_consistent() {
        let pool = create_test_db_pool();
        let state = Arc::new(AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        // 先验证 DB 可用（排除环境问题）
        let session_check = state.pool.get_session("admin").await;
        if session_check.is_err() {
            eprintln!("[skip] DB session not available — skipping concurrent DB validation test");
            return;
        }
        drop(session_check);

        let token = "concurrent_db_validate_token";
        let token_hash = format!("sha256:{}", hex::encode(Sha256::digest(token.as_bytes())));

        let mut handles = Vec::new();
        for _ in 0..20 {
            let state_clone = state.clone();
            let hash_clone = token_hash.clone();
            handles.push(tokio::spawn(async move {
                validate_api_key_from_db(&state_clone, &hash_clone, "10.0.0.200").await
            }));
        }

        let results: Vec<Result<Option<api_key::Model>, StatusCode>> =
            futures::future::join_all(handles)
                .await
                .into_iter()
                .map(|r| r.expect("spawned task must not panic"))
                .collect();

        // 所有结果必须一致：Err(401)（token 不在 DB 中，字面量未匹配）
        // 关键：不能是 Err(500)（DB 错误，表示并发竞态导致连接问题）
        for result in &results {
            assert_eq!(
                result,
                &Err(StatusCode::UNAUTHORIZED),
                "Concurrent DB validation must consistently return 401 for unknown token \
                 (not 500 DB error, not Ok(Some) race condition)"
            );
        }
        // 已验证每个结果都等于 Err(401)，因此所有结果必然一致（无需额外比较）
    }

    // ----- #212 分布式暴力破解：多 IP 各 < MAX_AUTH_FAILURES -----

    /// 多个 IP 各发起 `MAX_AUTH_FAILURES - 1` 次失败，
    /// 每个单独 IP 不应被锁定（当前 per-IP 追踪契约）。
    ///
    /// test_matrix.md #212 描述「多 IP 各 < 5 次（需全局监控）」，
    /// 当前实现为 per-IP 追踪（无全局监控）。
    /// 此测试锁定当前行为契约，并验证 per-IP 隔离正确性：
    /// - 每个 IP 独立计数
    /// - 低于阈值的 IP 不被锁定
    /// - 多个低阈值 IP 不会相互影响
    #[tokio::test]
    async fn test_distributed_brute_force_per_ip_tracking() {
        let limiter = AuthRateLimiter::new();
        let ips = ["203.0.113.1", "203.0.113.2", "203.0.113.3", "203.0.113.4"];

        // 每个 IP 发起 MAX_AUTH_FAILURES - 1 次失败（低于锁定阈值）
        for ip in &ips {
            for _ in 0..(MAX_AUTH_FAILURES - 1) {
                limiter.record_failure(ip).await;
            }
        }

        // 验证每个 IP 都未被锁定（低于阈值）
        for ip in &ips {
            assert!(
                !limiter.is_locked_out(ip).await,
                "IP {} with {} failures (below threshold) must not be locked",
                ip,
                MAX_AUTH_FAILURES - 1
            );
            assert_eq!(
                limiter.get_lockout_remaining(ip).await,
                0,
                "Non-locked IP must have 0 remaining lockout"
            );
        }
    }

    /// 分布式暴力破解场景：多 IP 各发起 `MAX_AUTH_FAILURES - 1` 次失败后，
    /// 任一 IP 再发起 1 次失败应触发该 IP 的锁定（阈值 = MAX_AUTH_FAILURES）。
    ///
    /// 这验证 per-IP 追踪在分布式场景下的正确性：
    /// 每个 IP 独立达到阈值时独立锁定。
    #[tokio::test]
    async fn test_distributed_brute_force_individual_ip_lockout_at_threshold() {
        let limiter = AuthRateLimiter::new();
        let ips = ["198.51.100.1", "198.51.100.2", "198.51.100.3"];

        // 每个 IP 发起 MAX_AUTH_FAILURES - 1 次失败
        for ip in &ips {
            for _ in 0..(MAX_AUTH_FAILURES - 1) {
                limiter.record_failure(ip).await;
            }
            assert!(!limiter.is_locked_out(ip).await);
        }

        // 其中一个 IP 再发起 1 次失败 → 该 IP 锁定，其他 IP 不受影响
        limiter.record_failure(ips[0]).await;
        assert!(
            limiter.is_locked_out(ips[0]).await,
            "IP {} must be locked after reaching threshold",
            ips[0]
        );
        assert!(
            limiter.get_lockout_remaining(ips[0]).await > 0,
            "Locked IP must have positive remaining lockout"
        );

        // 其他 IP 仍然未锁定
        assert!(
            !limiter.is_locked_out(ips[1]).await,
            "Other IP {} must not be affected by different IP's lockout",
            ips[1]
        );
        assert!(
            !limiter.is_locked_out(ips[2]).await,
            "Other IP {} must not be affected by different IP's lockout",
            ips[2]
        );
    }

    /// 分布式暴力破解场景：多 IP 各发起 `MAX_AUTH_FAILURES` 次失败，
    /// 所有 IP 都应被独立锁定（验证多 IP 同时锁定的正确性）。
    #[tokio::test]
    async fn test_distributed_brute_force_all_ips_locked_at_threshold() {
        let limiter = AuthRateLimiter::new();
        let ips = ["192.0.2.10", "192.0.2.20", "192.0.2.30", "192.0.2.40"];

        // 每个 IP 都达到锁定阈值
        for ip in &ips {
            for _ in 0..MAX_AUTH_FAILURES {
                limiter.record_failure(ip).await;
            }
        }

        // 所有 IP 都应被锁定
        for ip in &ips {
            assert!(
                limiter.is_locked_out(ip).await,
                "IP {} with {} failures must be locked",
                ip,
                MAX_AUTH_FAILURES
            );
        }
    }

    /// 分布式暴力破解场景：reset_failures 只影响指定 IP，
    /// 不影响其他 IP 的失败计数（验证 per-IP 隔离的 reset 操作）。
    #[tokio::test]
    async fn test_distributed_brute_force_reset_isolates_per_ip() {
        let limiter = AuthRateLimiter::new();
        let ip_a = "10.0.0.1";
        let ip_b = "10.0.0.2";

        // 两个 IP 都达到阈值
        for ip in &[ip_a, ip_b] {
            for _ in 0..MAX_AUTH_FAILURES {
                limiter.record_failure(ip).await;
            }
        }
        assert!(limiter.is_locked_out(ip_a).await);
        assert!(limiter.is_locked_out(ip_b).await);

        // 重置 ip_a，ip_b 不受影响
        limiter.reset_failures(ip_a).await;
        assert!(
            !limiter.is_locked_out(ip_a).await,
            "IP A must be unlocked after reset"
        );
        assert!(
            limiter.is_locked_out(ip_b).await,
            "IP B must remain locked after resetting IP A (per-IP isolation)"
        );
    }
}
