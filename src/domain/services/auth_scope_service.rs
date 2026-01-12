// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for managing API Key scopes and permissions

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::infrastructure::database::repositories::auth_scope_repo_impl::AuthScopeRepository;
use sea_orm::DatabaseConnection;
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AuthScopeServiceError {
    #[error("API Key not found")]
    ApiKeyNotFound,
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("Permission denied: required {required} but have {has}")]
    PermissionDenied {
        required: ScopePermission,
        has: ApiKeyScope,
    },
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),
}

/// Service for managing API Key scopes
#[derive(Clone)]
pub struct AuthScopeService {
    scope_repo: AuthScopeRepository,
}

impl AuthScopeService {
    /// Create a new service
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            scope_repo: AuthScopeRepository::new(db),
        }
    }

    /// Get scope for an API Key, with inheritance from team defaults
    pub async fn get_scope_for_key(
        &self,
        api_key_id: Uuid,
        team_default_scope: Option<ApiKeyScope>,
    ) -> Result<ApiKeyScope, AuthScopeServiceError> {
        debug!("Getting scope for API Key: {}", api_key_id);

        // Try to find custom scope
        let custom_scope = self.scope_repo.find_by_api_key_id(api_key_id).await?;

        match custom_scope {
            Some(scope) => {
                debug!("Found custom scope for API Key: {}", api_key_id);
                Ok(scope)
            }
            None => {
                // Use team default scope if available
                match team_default_scope {
                    Some(team_scope) => {
                        debug!("Using team default scope for API Key: {}", api_key_id);
                        Ok(team_scope)
                    }
                    None => {
                        // Return default scope
                        debug!("Using default scope for API Key: {}", api_key_id);
                        Ok(ApiKeyScope::default())
                    }
                }
            }
        }
    }

    /// Validate that a scope has required permission
    pub fn validate_permission(
        scope: &ApiKeyScope,
        required: ScopePermission,
    ) -> Result<(), AuthScopeServiceError> {
        if scope.has_permission(required) {
            Ok(())
        } else {
            Err(AuthScopeServiceError::PermissionDenied {
                required,
                has: scope.clone(),
            })
        }
    }

    /// Validate that a scope allows the requested search count
    pub fn validate_search_count(
        scope: &ApiKeyScope,
        count: u32,
    ) -> Result<(), AuthScopeServiceError> {
        if scope.allows_search_count(count) {
            Ok(())
        } else {
            Err(AuthScopeServiceError::QuotaExceeded(format!(
                "Search limit exceeded: requested {} but limit is {}",
                count, scope.search_limit
            )))
        }
    }

    /// Validate that a scope allows the requested scrape count
    pub fn validate_scrape_count(
        scope: &ApiKeyScope,
        count: u32,
    ) -> Result<(), AuthScopeServiceError> {
        if scope.allows_scrape_count(count) {
            Ok(())
        } else {
            Err(AuthScopeServiceError::QuotaExceeded(format!(
                "Scrape limit exceeded: requested {} but limit is {}",
                count, scope.scrape_limit
            )))
        }
    }

    /// Set scope for an API Key
    pub async fn set_scope(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, AuthScopeServiceError> {
        debug!("Setting scope for API Key: {:?}", scope);
        self.scope_repo
            .upsert(api_key_id, scope)
            .await
            .map_err(Into::into)
    }

    /// Delete custom scope for an API Key (revert to defaults)
    pub async fn delete_scope(&self, api_key_id: Uuid) -> Result<bool, AuthScopeServiceError> {
        debug!("Deleting custom scope for API Key: {}", api_key_id);
        self.scope_repo
            .delete_by_api_key_id(api_key_id)
            .await
            .map_err(Into::into)
    }

    /// Merge scopes: custom scope overrides team default
    pub fn merge_scopes(
        team_scope: Option<&ApiKeyScope>,
        custom_scope: Option<&ApiKeyScope>,
    ) -> ApiKeyScope {
        match (team_scope, custom_scope) {
            (Some(team), Some(custom)) => {
                // Custom scope takes precedence
                custom.clone()
            }
            (Some(team), None) => team.clone(),
            (None, Some(custom)) => custom.clone(),
            (None, None) => ApiKeyScope::default(),
        }
    }
}
