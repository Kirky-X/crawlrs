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
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use lru::LruCache;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// Maximum authentication failures before lockout
const MAX_AUTH_FAILURES: usize = 5;
/// Lockout duration after exceeding max failures (15 minutes)
const AUTH_LOCKOUT_DURATION: Duration = Duration::from_secs(900);
/// Auth failure tracking window (1 hour)
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(3600);

/// LRU Cache for authenticated API keys to reduce database queries
pub struct ApiKeyCache {
    /// LRU cache storing API key validation results
    cache: LruCache<String, CachedAuthResult>,
    /// TTL for cache entries (5 minutes)
    ttl: Duration,
}

#[derive(Clone)]
pub struct CachedAuthResult {
    team_id: Uuid,
    api_key_id: Uuid,
    scope: ApiKeyScope,
    cached_at: Instant,
}

impl ApiKeyCache {
    fn new(max_size: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: LruCache::new(std::num::NonZeroUsize::new(max_size).unwrap()),
            ttl: Duration::from_secs(ttl_seconds),
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

    fn clear_expired(&mut self) {
        let _now = Instant::now();
        let _ttl = self.ttl;
        // Note: Simple cleanup - in production, use a more efficient approach
    }
}

/// Authentication rate limiter for brute-force protection
#[derive(Clone)]
pub struct AuthRateLimiter {
    /// Tracks authentication failures by client IP
    failures: Arc<RwLock<std::collections::HashMap<String, (usize, Instant)>>>,
}

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
#[derive(Clone, Debug)]
pub struct AuthState {
    /// Database connection for additional queries
    pub db: Arc<DatabaseConnection>,
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
    pub fn new(
        db: Arc<DatabaseConnection>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Self {
        Self {
            db,
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
        db: Arc<DatabaseConnection>,
        auth_scope_service: AuthScopeService,
        team_id: Uuid,
        api_key_id: Uuid,
        default_scope: ApiKeyScope,
    ) -> Self {
        Self {
            db,
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
        db: Arc<DatabaseConnection>,
        auth_scope_service: Option<AuthScopeService>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
        cache: Arc<RwLock<ApiKeyCache>>,
    ) -> Self {
        Self {
            db,
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
    pub fn with_trusted_proxies(
        db: Arc<DatabaseConnection>,
        auth_scope_service: Option<AuthScopeService>,
        team_id: Uuid,
        api_key_id: Uuid,
        scope: ApiKeyScope,
        cache: Option<Arc<RwLock<ApiKeyCache>>>,
        rate_limiter: Option<Arc<AuthRateLimiter>>,
        trusted_proxies: security::TrustedProxyConfig,
    ) -> Self {
        Self {
            db,
            auth_scope_service,
            team_id,
            api_key_id,
            scope,
            api_key_cache: cache,
            auth_rate_limiter: rate_limiter,
            trusted_proxies: Some(trusted_proxies),
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
                    tracing::warn!("Failed to load scope from database: {:?}, using default", e);
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
/// # Arguments
///
/// * `state` - Authentication state containing database connection and config
/// * `req` - The HTTP request
/// * `next` - The next middleware in the chain
///
/// # Returns
///
/// * `Ok(Response)` - If authentication is successful
/// * `Err(StatusCode)` - If authentication fails
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    debug!("AuthMiddleware processing path: {}", path);

    // Allow public endpoints without authentication
    if PUBLIC_ENDPOINTS.contains(&path) {
        debug!("Public endpoint {}, skipping auth", path);
        return Ok(next.run(req).await);
    }

    // Get client IP for rate limiting using secure IP extraction
    let client_ip = get_client_ip(&req, state.trusted_proxies.as_ref());

    // Check auth rate limit lockout
    if let Some(ref rate_limiter) = state.auth_rate_limiter {
        if rate_limiter.is_locked_out(&client_ip).await {
            let remaining = rate_limiter.get_lockout_remaining(&client_ip).await;
            tracing::warn!(
                "Auth rate limit exceeded for IP: {}, lockout remaining: {}s",
                client_ip,
                remaining
            );
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    // Extract and validate Bearer token
    let token_str = match extract_bearer_token(&req) {
        Some(token) => token,
        None => {
            // Record auth failure for rate limiting
            if let Some(ref rate_limiter) = state.auth_rate_limiter {
                rate_limiter.record_failure(&client_ip).await;
            }
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Hash the token for lookup using SHA-256 (consistent hash for lookup)
    // Note: We use SHA-256 for lookup because bcrypt generates different hashes each time
    // The actual verification uses bcrypt (line 398) or SHA-256 (line 405) depending on stored format
    let token_hash = format!("sha256:{:x}", Sha256::digest(token_str.as_bytes()));

    // PERF-002: Check cache first before database query
    if let Some(ref cache) = state.api_key_cache {
        let mut cache_guard = cache.write().await;
        if let Some(cached_result) = cache_guard.get(&token_hash) {
            debug!("API Key authentication cache hit for key hash");
            // Reconstruct AuthState from cache
            let auth_state = AuthState::with_cache(
                state.db.clone(),
                state.auth_scope_service.clone(),
                cached_result.team_id,
                cached_result.api_key_id,
                cached_result.scope.clone(),
                cache.clone(),
            );
            req.extensions_mut().insert(auth_state.clone());
            req.extensions_mut().insert(cached_result.team_id);
            req.extensions_mut().insert(cached_result.api_key_id);
            debug!("API Key authentication successful (cached)");
            return Ok(next.run(req).await);
        }
    }

    // Security fix: 使用 key_hash 列进行查询，避免原始密钥在数据库查询中暴露
    match api_key::Entity::find()
        .filter(api_key::Column::KeyHash.eq(&token_hash)) // 使用哈希值查询
        .one(state.db.as_ref())
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

            // 如果 key_hash 为 None，说明是旧格式的明文存储
            // 直接使用，无需验证
            let is_valid = if let Some(ref stored_hash) = key.key_hash {
                // 检查是否是新格式的 bcrypt 哈希
                if stored_hash.starts_with("$2b$") {
                    // 使用 bcrypt 验证
                    security::verify_api_key(&token_str, stored_hash)
                } else if stored_hash.starts_with("sha256:") {
                    // SHA256 哈希格式（测试用）
                    // 提取存储的哈希值（去掉 "sha256:" 前缀）
                    let stored_sha256 = stored_hash.trim_start_matches("sha256:");
                    // 计算输入的 SHA256
                    let input_sha256 = format!("{:x}", Sha256::digest(token_str.as_bytes()));
                    stored_sha256 == input_sha256
                } else if stored_hash.len() == 64
                    && stored_hash.chars().all(|c| c.is_ascii_hexdigit())
                {
                    // 纯 SHA-256 哈希（64字符十六进制）
                    let input_sha256 = format!("{:x}", Sha256::digest(token_str.as_bytes()));
                    *stored_hash == input_sha256
                } else {
                    // 其他格式，使用 bcrypt 验证（可能失败）
                    security::verify_api_key(&token_str, stored_hash)
                }
            } else {
                // SECURITY: 明文存储的API密钥不再被接受
                // 这是一个严重的安全漏洞，任何此类密钥都应该被拒绝
                tracing::error!(
                    "SECURITY CRITICAL: Attempted authentication with plaintext API key (key_id={}). \
                    This should never happen in production. All API keys must use hashed storage.",
                    key.id
                );
                return Err(StatusCode::UNAUTHORIZED);
            };

            // 如果验证失败
            if !is_valid {
                warn!("API Key verification failed for key_id={}", key.id);
                return Err(StatusCode::UNAUTHORIZED);
            }

            // Check if key is inactive or expired
            // 使用明确的过期检查逻辑：如果是更新过的 key，检查是否过期
            if let Some(updated_at) = key.updated_at {
                let now = chrono::Utc::now();
                let days_since_update = (now.signed_duration_since(updated_at)).num_days();

                // 如果 key 在过去 90 天内更新过，认为是活跃的
                // 如果超过 90 天未更新，则拒绝访问，要求重新生成 key
                if days_since_update > 90 {
                    tracing::warn!(
                        "API key {} has not been updated in {} days, may be expired",
                        key.id,
                        days_since_update
                    );
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }

            // Security fix: 强制拒绝明文 API 密钥，不分环境
            // 明文存储的 API 密钥是严重的安全漏洞，必须被拒绝
            if key.key_hash.is_none() {
                tracing::error!(
                    "SECURITY CRITICAL: Attempted authentication with plaintext API key (key_id={}). \
                    Plaintext API keys are never allowed in any environment. \
                    Please migrate to hashed storage immediately.",
                    key.id
                );
                return Err(StatusCode::UNAUTHORIZED);
            }

            // Create AuthState with scope service from AppState
            // AuthScopeService is now initialized during app startup via init_auth_scope_service()
            let auth_scope_service = match state.auth_scope_service.clone() {
                Some(service) => service,
                None => {
                    tracing::error!(
                        "FATAL: AuthScopeService not initialized in AppState. \
                        This indicates a startup configuration error."
                    );
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };
            let mut auth_state = AuthState::with_scope_service(
                state.db.clone(),
                auth_scope_service,
                key.team_id,
                key.id,
                ApiKeyScope::default(),
            );

            // Load actual scope from database
            auth_state.load_scope_from_db().await;

            // PERF-002: Cache the successful authentication result
            // Security fix: 使用哈希值作为缓存键，防止原始密钥泄露
            if let Some(ref cache) = state.api_key_cache {
                let mut cache_guard = cache.write().await;
                cache_guard.insert(
                    token_hash.clone(), // 使用哈希值作为缓存键
                    CachedAuthResult {
                        team_id: key.team_id,
                        api_key_id: key.id,
                        scope: auth_state.scope.clone(),
                        cached_at: Instant::now(),
                    },
                );
            }

            // Inject auth state and extracted values into request extensions
            // Note: Only store token hash for security, not the raw token
            req.extensions_mut().insert(auth_state.clone());
            req.extensions_mut().insert(key.team_id);
            req.extensions_mut().insert(key.id);
            req.extensions_mut().insert(token_hash);

            // Reset auth failures on successful authentication
            if let Some(ref rate_limiter) = state.auth_rate_limiter {
                rate_limiter.reset_failures(&client_ip).await;
            }

            // 日志只记录认证成功，不记录具体密钥信息或详细 team_id
            debug!("API Key authentication successful");

            // Log successful authentication to audit service
            if let Some(audit_service) = req.extensions().get::<Arc<dyn AuditServiceTrait>>() {
                let _ = audit_service
                    .log_allow(
                        "api_key.authenticated".to_string(),
                        key.id,
                        key.team_id,
                        auth_state.scope.clone(),
                    )
                    .await;
            }

            Ok(next.run(req).await)
        }
        Ok(None) => {
            warn!("API Key authentication failed: key not found");
            // Record auth failure for rate limiting
            if let Some(ref rate_limiter) = state.auth_rate_limiter {
                rate_limiter.record_failure(&client_ip).await;
            }
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            tracing::error!("Database error checking API key: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(req: &Request) -> Option<String> {
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
fn get_client_ip(req: &Request, trusted_proxies: Option<&security::TrustedProxyConfig>) -> String {
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
pub async fn scope_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
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
pub fn test_auth_state(db: Arc<DatabaseConnection>, team_id: Uuid, api_key_id: Uuid) -> AuthState {
    AuthState::new(db, team_id, api_key_id, ApiKeyScope::default())
}
