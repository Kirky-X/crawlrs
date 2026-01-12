// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Enhanced authentication middleware with scope and feature flag support

use crate::domain::auth::{ApiKeyScope, AuditDecision, ScopePermission};
use crate::infrastructure::database::entities::api_key;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// Authentication state with enhanced features
#[derive(Clone)]
pub struct AuthState {
    pub db: Arc<DatabaseConnection>,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub scope: ApiKeyScope,
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
}

/// Enhanced authentication middleware
///
/// Validates API Key and loads associated scope for authorization
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    debug!("EnhancedAuthMiddleware processing path: {}", path);

    // Allow public endpoints
    if path == "/health" || path == "/metrics" || path == "/v1/version" {
        return Ok(next.run(req).await);
    }

    let token_str = {
        let auth_header = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|header| header.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(StatusCode::UNAUTHORIZED);
        }

        auth_header[7..].to_string()
    };

    // Query DB to validate token and get API Key info
    match api_key::Entity::find()
        .filter(api_key::Column::Key.eq(token_str.clone()))
        .one(state.db.as_ref())
        .await
    {
        Ok(Some(key)) => {
            // Security check: reject nil UUID
            if key.team_id == Uuid::nil() {
                warn!("SECURITY: API key with nil team_id detected");
                return Err(StatusCode::UNAUTHORIZED);
            }

            if key.updated_at.is_some() && key.updated_at.unwrap() < chrono::Utc::now().naive_utc()
            {
                // Check if key is inactive (assuming there's an is_active column or similar)
                // For now, we just proceed
            }

            // Inject enhanced auth state
            req.extensions_mut().insert(state.clone());
            req.extensions_mut().insert(key.team_id);
            req.extensions_mut().insert(key.id);
            req.extensions_mut().insert(token_str);

            debug!(
                "Authenticated API Key: {}, team: {}, scope: {:?}",
                key.id, key.team_id, state.scope
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

/// Scope validation middleware
///
/// Validates that the API Key has the required scope for the requested endpoint
pub async fn scope_middleware(
    State(state): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let method = req.method().clone();

    // Determine required scope based on endpoint
    let required_scope = determine_required_scope(&path, &method);

    if let Some(required) = required_scope {
        if !state.scope.has_permission(required) {
            warn!(
                "Scope denied: API Key {} lacks {:?} for {} {}",
                state.api_key_id, required, method, path
            );

            // TODO: Log to audit service

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
/// Use this in handlers to check if a feature is enabled
pub async fn check_feature_flag(feature_name: &str, state: &AuthState) -> Result<bool, AuthError> {
    // This would integrate with the FeatureFlagService
    // For now, return true (feature enabled by default)
    Ok(true)
}
