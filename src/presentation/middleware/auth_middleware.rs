// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified authentication middleware with scope and feature flag support
//!
//! Provides API key authentication with rate limiting for brute-force protection.

#![allow(dead_code)]

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::auth_scope_service::{AuthScopeService, AuthScopeServiceTrait};
use crate::infrastructure::database::entities::api_key;
use crate::infrastructure::security;
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
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};
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
/// 使用 Mutex<Option<Arc<...>>> 而非 OnceLock，以支持测试中重置全局状态
static GLOBAL_AUTH_CACHE: Mutex<Option<Arc<RwLock<ApiKeyCache>>>> = Mutex::new(None);

/// Global auth state for middleware
static GLOBAL_AUTH_STATE: OnceLock<Arc<AuthState>> = OnceLock::new();

/// Get the global auth cache instance
pub fn get_global_auth_cache() -> Option<Arc<RwLock<ApiKeyCache>>> {
    GLOBAL_AUTH_CACHE.lock().unwrap().clone()
}

/// Set the global auth cache instance (called during application startup)
pub fn set_global_auth_cache(cache: Arc<RwLock<ApiKeyCache>>) {
    *GLOBAL_AUTH_CACHE.lock().unwrap() = Some(cache);
}

/// Reset the global auth cache (test-only, for avoiding cross-test OnceLock pollution)
#[cfg(test)]
fn reset_global_auth_cache() {
    *GLOBAL_AUTH_CACHE.lock().unwrap() = None;
}

/// Get the global auth state for middleware
pub fn get_global_auth_state() -> Option<Arc<AuthState>> {
    GLOBAL_AUTH_STATE.get().cloned()
}

/// Set the global auth state (called during application startup)
pub fn set_global_auth_state(state: Arc<AuthState>) {
    let _ = GLOBAL_AUTH_STATE.set(state);
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
    let token_hash = format!("sha256:{:x}", Sha256::digest(token_str.as_bytes()));

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
fn verify_key_hash(key: &api_key::Model, token_str: &str) -> bool {
    if let Some(ref stored_hash) = key.key_hash {
        // Check if it's a new format bcrypt hash
        if stored_hash.starts_with("$2b$") {
            security::verify_api_key(token_str, stored_hash)
        } else if stored_hash.starts_with("sha256:") {
            // SHA256 hash format (for testing)
            let stored_sha256 = stored_hash.trim_start_matches("sha256:");
            let input_sha256 = format!("{:x}", Sha256::digest(token_str.as_bytes()));
            stored_sha256 == input_sha256
        } else if stored_hash.len() == 64 && stored_hash.chars().all(|c| c.is_ascii_hexdigit()) {
            // Pure SHA-256 hash (64 character hex)
            let input_sha256 = format!("{:x}", Sha256::digest(token_str.as_bytes()));
            *stored_hash == input_sha256
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
    // Get AuthScopeService from AppState
    let auth_scope_service = match state.auth_scope_service.clone() {
        Some(service) => service,
        None => {
            log::error!(
                "FATAL: AuthScopeService not initialized in AppState. \
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

/// Feature flag check extension
///
/// Use this in handlers to check if a feature is enabled for the current API key
pub async fn check_feature_flag(
    _feature_name: &str,
    _state: &AuthState,
) -> Result<bool, AuthError> {
    // This would integrate with the FeatureFlagService
    // For now, return true (feature enabled by default)
    Ok(true)
}

/// Create an auth state for testing purposes
#[cfg(test)]
pub fn test_auth_state(db: Arc<DbPool>, team_id: Uuid, api_key_id: Uuid) -> AuthState {
    AuthState::new(db, team_id, api_key_id, ApiKeyScope::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::Instant;

    /// Serializes tests that touch the global auth cache to prevent race conditions.
    static GLOBAL_CACHE_LOCK: StdMutex<()> = StdMutex::new(());

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
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
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

    #[tokio::test]
    async fn test_global_cache_invalidate_all() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
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
        let hash = format!("sha256:{:x}", Sha256::digest(token.as_bytes()));
        let key = make_key_model(Some(hash), Some(0));
        assert!(verify_key_hash(&key, token));
    }

    #[test]
    fn test_verify_key_hash_sha256_prefix_mismatch() {
        let token = "test_token_123";
        let hash = format!("sha256:{:x}", Sha256::digest(token.as_bytes()));
        let key = make_key_model(Some(hash), Some(0));
        assert!(!verify_key_hash(&key, "wrong_token"));
    }

    #[test]
    fn test_verify_key_hash_pure_sha256_match() {
        let token = "test_token_456";
        let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let key = make_key_model(Some(hash), Some(0));
        assert!(verify_key_hash(&key, token));
    }

    #[test]
    fn test_verify_key_hash_pure_sha256_mismatch() {
        let token = "test_token_456";
        let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
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
        // no ConnectInfo → returns "unknown"
        assert_eq!(ip, "unknown");
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

    // ===== Global cache function tests (when not initialized) =====

    #[tokio::test]
    async fn test_global_cache_functions_when_not_initialized() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
        reset_global_auth_cache();
        assert!(!invalidate_cache_by_token_hash("sha256:test").await);
        assert_eq!(invalidate_cache_by_api_key_id(Uuid::new_v4()).await, 0);
        assert_eq!(invalidate_cache_by_team(Uuid::new_v4()).await, 0);
        assert_eq!(invalidate_all_cache().await, 0);
        assert!(get_cache_stats().await.is_none());
    }

    // ===== Global cache function tests (when initialized) =====

    #[tokio::test]
    async fn test_global_invalidate_cache_by_token_hash_hit() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
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

    #[tokio::test]
    async fn test_global_invalidate_cache_by_token_hash_miss() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache);
        assert!(!invalidate_cache_by_token_hash("sha256:nonexistent").await);
    }

    #[tokio::test]
    async fn test_global_invalidate_cache_by_api_key_id_multiple() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
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

    #[tokio::test]
    async fn test_global_invalidate_cache_by_team() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
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

    #[tokio::test]
    async fn test_global_get_cache_stats_when_initialized() {
        let _guard = GLOBAL_CACHE_LOCK.lock().unwrap();
        reset_global_auth_cache();
        let cache = Arc::new(RwLock::new(ApiKeyCache::new_default()));
        set_global_auth_cache(cache);
        let stats = get_cache_stats().await;
        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.capacity, DEFAULT_CACHE_MAX_SIZE);
    }
}
