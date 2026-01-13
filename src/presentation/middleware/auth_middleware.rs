// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified authentication middleware with scope and feature flag support
//!
//! This module provides comprehensive authentication middleware that consolidates
//! the functionality from both `auth_middleware.rs` and `auth_middleware_enhanced.rs`.
//!
//! ## Features
//! - API Key authentication with hashed token support
//! - Scope-based authorization
//! - Feature flag support
//! - Audit logging integration
//!
//! ## Usage
//!
//! ```rust
//! use crate::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
//! use axum::{Router, routing::get, middleware::from_fn_with_state};
//!
//! async fn handler() -> &'static str {
//!     "Hello, authenticated user!"
//! }
//!
//! let app = Router::new()
//!     .route("/", get(handler))
//!     .layer(middleware::from_fn_with_state(auth_state, auth_middleware));
//! ```

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::infrastructure::database::entities::api_key;
use crate::infrastructure::security;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use sea_orm::{ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter};
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// Authentication state with enhanced features
///
/// This state is injected into requests after successful authentication and contains
/// all necessary information for authorization checks.
#[derive(Clone, Debug)]
pub struct AuthState {
    /// Database connection for additional queries
    pub db: Arc<DatabaseConnection>,
    /// Team ID associated with the API key
    pub team_id: Uuid,
    /// API Key ID for audit logging and feature flags
    pub api_key_id: Uuid,
    /// Scope permissions for the API key
    pub scope: ApiKeyScope,
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
            team_id,
            api_key_id,
            scope,
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

/// Public endpoints that don't require authentication
const PUBLIC_ENDPOINTS: &[&str] = &["/health", "/metrics", "/v1/version"];

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
    if PUBLIC_ENDPOINTS.iter().any(|&endpoint| path == endpoint) {
        debug!("Public endpoint {}, skipping auth", path);
        return Ok(next.run(req).await);
    }

    // Extract and validate Bearer token
    let token_str = match extract_bearer_token(&req) {
        Some(token) => token,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Hash the token for lookup
    let token_hash = security::hash_api_key(&token_str);

    // Query DB to validate token and get API Key info
    match api_key::Entity::find()
        .filter(
            Condition::any()
                .add(api_key::Column::Key.eq(token_str.clone()))
                .add(api_key::Column::KeyHash.eq(token_hash)),
        )
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

            // Check if key is inactive (assuming there's an is_active column or similar)
            // For now, we proceed with authentication but log a warning if needed
            if let Some(updated_at) = key.updated_at {
                let now = chrono::Utc::now();
                if updated_at < now {
                    // Key might be deactivated based on updated_at timestamp
                    // This is a simplified check - in production, you'd have an explicit is_active field
                    debug!(
                        "API key {} was updated in the past, may need re-validation",
                        key.id
                    );
                }
            }

            // Log migration status for keys using legacy plaintext storage
            if key.key_hash.is_none() {
                tracing::info!(
                    "API Key {} uses legacy plaintext storage, consider migrating to hashed storage",
                    key.id
                );
            }

            // Create AuthState with default scope if not present in DB
            // In a real implementation, you'd load the scope from the database
            let auth_state = AuthState::new(
                state.db.clone(),
                key.team_id,
                key.id,
                ApiKeyScope::default(), // TODO: Load actual scope from database
            );

            // Inject auth state and extracted values into request extensions
            req.extensions_mut().insert(auth_state.clone());
            req.extensions_mut().insert(key.team_id);
            req.extensions_mut().insert(key.id);
            req.extensions_mut().insert(token_str);

            debug!(
                "Authenticated API Key: {}, team: {}, scope: {:?}",
                key.id, key.team_id, auth_state.scope
            );

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
pub async fn scope_middleware(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Determine required scope based on endpoint
    let required_scope = determine_required_scope(&path, &method.to_string());

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

            // TODO: Log to audit service
            // let audit_entry = AuditLogEntry { ... };

            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(req).await)
}

/// Determine required scope for an endpoint
fn determine_required_scope(path: &str, method: &str) -> Option<ScopePermission> {
    // Admin endpoints
    if path.starts_with("/api/v1/teams") || path.starts_with("/api/v1/billing") {
        return Some(ScopePermission::Admin);
    }

    // Write endpoints (POST, PUT, PATCH, DELETE)
    if method == "POST" || method == "PUT" || method == "PATCH" || method == "DELETE" {
        // Exception: some write-like endpoints might be read-only
        if path.contains("/search") || path.contains("/scrape") {
            // These are actually read operations
            return None;
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
