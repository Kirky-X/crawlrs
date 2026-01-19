// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified authentication middleware with scope and feature flag support

#![allow(dead_code)]

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::domain::services::audit_service::AuditService;
use crate::domain::services::auth_scope_service::AuthScopeService;
use crate::infrastructure::database::entities::api_key;
use crate::infrastructure::security;
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// LRU Cache for authenticated API keys to reduce database queries
pub struct ApiKeyCache {
    /// LRU cache storing API key validation results
    cache: HashMap<String, CachedAuthResult>,
    /// Track access order for LRU eviction
    access_order: Vec<String>,
    /// Maximum cache size
    max_size: usize,
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
            cache: HashMap::with_capacity(max_size),
            access_order: Vec::with_capacity(max_size),
            max_size,
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedAuthResult> {
        // Check if key exists and not expired
        if let Some(result) = self.cache.get(key) {
            if result.cached_at.elapsed() < self.ttl {
                // Update access order - move key to end (most recently used)
                self.access_order.retain(|k| k != key);
                self.access_order.push(key.to_string());
                return Some(result.clone());
            } else {
                // Remove expired entry
                self.cache.remove(key);
                self.access_order.retain(|k| k != key);
            }
        }
        None
    }

    fn insert(&mut self, key: String, result: CachedAuthResult) {
        // Evict oldest entry if cache is full
        if self.cache.len() >= self.max_size && !self.access_order.is_empty() {
            if let Some(oldest_key) = self.access_order.first().cloned() {
                self.cache.remove(&oldest_key);
                self.access_order.remove(0);
            }
        }
        self.cache.insert(key.clone(), result);
        self.access_order.push(key);
    }

    fn clear_expired(&mut self) {
        let now = Instant::now();
        let expired_keys: Vec<String> = self
            .access_order
            .iter()
            .filter(|key| {
                self.cache
                    .get(key.as_str())
                    .is_some_and(|r| now.duration_since(r.cached_at) >= self.ttl)
            })
            .cloned()
            .collect();

        for key in &expired_keys {
            self.cache.remove(key.as_str());
        }
        self.access_order.retain(|k| !expired_keys.contains(k));
    }
}

impl std::fmt::Debug for ApiKeyCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyCache")
            .field("size", &self.cache.len())
            .field("max_size", &self.max_size)
            .field("ttl_seconds", &self.ttl.as_secs())
            .finish()
    }
}

/// Authentication state with enhanced features
///
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

    // Extract and validate Bearer token
    let token_str = match extract_bearer_token(&req) {
        Some(token) => token,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Hash the token for lookup
    let token_hash = match security::hash_api_key(&token_str) {
        Ok(hash) => hash,
        Err(_e) => {
            tracing::error!("Failed to hash API key: hash operation failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

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

    match api_key::Entity::find()
        .filter(api_key::Column::Key.eq(&token_str)) // 先通过原始 key 查找
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
                } else {
                    // 其他格式，使用 bcrypt 验证（可能失败）
                    security::verify_api_key(&token_str, stored_hash)
                }
            } else {
                // 明文存储的 key，直接使用
                // 在测试环境允许
                true
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

            // Log migration status for keys using legacy plaintext storage
            if key.key_hash.is_none() {
                let env = std::env::var("CRAWLRS_ENV").unwrap_or_default();
                if env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod") {
                    // In production, reject legacy plaintext API keys
                    tracing::error!(
                        "SECURITY: Legacy plaintext API Key {} rejected in production environment",
                        key.id
                    );
                    return Err(StatusCode::UNAUTHORIZED);
                }
                tracing::warn!(
                    "Legacy plaintext API Key {} detected, please migrate to hashed storage",
                    key.id
                );
            }

            // Create AuthState with default scope if not present in DB
            // In a real implementation, you'd load the scope from the database
            let mut auth_state = AuthState::with_scope_service(
                state.db.clone(),
                state
                    .auth_scope_service
                    .clone()
                    .unwrap_or_else(|| AuthScopeService::new((*state.db).clone())),
                key.team_id,
                key.id,
                ApiKeyScope::default(),
            );

            // Load actual scope from database
            auth_state.load_scope_from_db().await;

            // PERF-002: Cache the successful authentication result
            if let Some(ref cache) = state.api_key_cache {
                let mut cache_guard = cache.write().await;
                cache_guard.insert(
                    token_str.clone(), // 使用原始 key 作为缓存键
                    CachedAuthResult {
                        team_id: key.team_id,
                        api_key_id: key.id,
                        scope: auth_state.scope.clone(),
                        cached_at: Instant::now(),
                    },
                );
            }

            // Inject auth state and extracted values into request extensions
            req.extensions_mut().insert(auth_state.clone());
            req.extensions_mut().insert(key.team_id);
            req.extensions_mut().insert(key.id);
            req.extensions_mut().insert(token_str);

            // 日志只记录认证成功，不记录具体密钥信息或详细 team_id
            debug!("API Key authentication successful");

            // Log successful authentication to audit service
            if let Some(audit_service) = req.extensions().get::<Arc<AuditService>>() {
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
            if let Some(audit_service) = req.extensions().get::<Arc<AuditService>>() {
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

/// Determine required scope for an endpoint
fn determine_required_scope(path: &str, method: &str) -> Option<ScopePermission> {
    // Helper function for exact path prefix matching
    // Ensures we match "/api/v1/teams" but not "/api/v1/teams-secret"
    fn is_path_prefix(path: &str, prefix: &str) -> bool {
        path == prefix || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
    }

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
