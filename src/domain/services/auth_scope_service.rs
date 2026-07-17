// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for managing API Key scopes and permissions

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::domain::repositories::auth_scope_repository::AuthScopeRepository;
use crate::presentation::middleware::auth_middleware::invalidate_cache_by_api_key_id;
use async_trait::async_trait;
use log::debug;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AuthScopeServiceError {
    #[error("API Key not found")]
    ApiKeyNotFound,
    #[error("Database error: {0}")]
    DatabaseError(#[from] crate::domain::repositories::auth_scope_repository::RepositoryError),
    #[error("Permission denied: required {required} but have {has}")]
    PermissionDenied {
        required: ScopePermission,
        has: ApiKeyScope,
    },
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),
}

/// Service trait for managing API Key scopes
#[async_trait]
pub trait AuthScopeServiceTrait: Send + Sync {
    async fn get_scope_for_key(
        &self,
        api_key_id: Uuid,
        team_default_scope: Option<ApiKeyScope>,
    ) -> Result<ApiKeyScope, AuthScopeServiceError>;

    async fn set_scope(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, AuthScopeServiceError>;

    async fn delete_scope(&self, api_key_id: Uuid) -> Result<bool, AuthScopeServiceError>;
}

/// Service for managing API Key scopes
pub struct AuthScopeService {
    scope_repo: Arc<dyn AuthScopeRepository>,
}

impl Clone for AuthScopeService {
    fn clone(&self) -> Self {
        Self {
            scope_repo: Arc::clone(&self.scope_repo),
        }
    }
}

impl std::fmt::Debug for AuthScopeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthScopeService")
            .field("scope_repo", &"AuthScopeRepository")
            .finish()
    }
}

impl AuthScopeService {
    /// Create a new service
    pub fn new(scope_repo: Arc<dyn AuthScopeRepository>) -> Self {
        Self { scope_repo }
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

    /// Merge scopes: custom scope overrides team default
    pub fn merge_scopes(
        team_scope: Option<&ApiKeyScope>,
        custom_scope: Option<&ApiKeyScope>,
    ) -> ApiKeyScope {
        match (team_scope, custom_scope) {
            (Some(_team), Some(custom)) => {
                // Custom scope takes precedence
                custom.clone()
            }
            (Some(team), None) => team.clone(),
            (None, Some(custom)) => custom.clone(),
            (None, None) => ApiKeyScope::default(),
        }
    }
}

#[async_trait]
impl AuthScopeServiceTrait for AuthScopeService {
    /// Get scope for an API Key, with inheritance from team defaults
    async fn get_scope_for_key(
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

    /// Set scope for an API Key
    async fn set_scope(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, AuthScopeServiceError> {
        debug!("Setting scope for API Key: {:?}", scope);
        let result = self
            .scope_repo
            .upsert(api_key_id, scope)
            .await
            .map_err(Into::into);

        // Invalidate cache after scope change for security
        if result.is_ok() {
            let removed = invalidate_cache_by_api_key_id(api_key_id).await;
            debug!(
                "Invalidated {} cache entries for API key {} after scope update",
                removed, api_key_id
            );
        }

        result
    }

    /// Delete custom scope for an API Key (revert to defaults)
    async fn delete_scope(&self, api_key_id: Uuid) -> Result<bool, AuthScopeServiceError> {
        debug!("Deleting custom scope for API Key: {}", api_key_id);
        let result = self
            .scope_repo
            .delete_by_api_key_id(api_key_id)
            .await
            .map_err(Into::into);

        // Invalidate cache after scope deletion for security
        if result.is_ok() {
            let removed = invalidate_cache_by_api_key_id(api_key_id).await;
            debug!(
                "Invalidated {} cache entries for API key {} after scope deletion",
                removed, api_key_id
            );
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_permission_allows_read_for_read_scope() {
        let scope = ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            ..Default::default()
        };
        let result = AuthScopeService::validate_permission(&scope, ScopePermission::Read);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_permission_allows_write_for_write_scope() {
        let scope = ApiKeyScope {
            read: true,
            write: true,
            admin: false,
            ..Default::default()
        };
        let result = AuthScopeService::validate_permission(&scope, ScopePermission::Write);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_permission_denies_write_for_read_only() {
        let scope = ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            ..Default::default()
        };
        let result = AuthScopeService::validate_permission(&scope, ScopePermission::Write);
        assert!(result.is_err());
        if let Err(AuthScopeServiceError::PermissionDenied { required, has }) = result {
            assert_eq!(required, ScopePermission::Write);
            assert!(!has.write);
            assert!(has.read);
        } else {
            panic!("Expected PermissionDenied error");
        }
    }

    #[test]
    fn test_validate_permission_denies_admin_for_write_scope() {
        let scope = ApiKeyScope {
            read: true,
            write: true,
            admin: false,
            ..Default::default()
        };
        let result = AuthScopeService::validate_permission(&scope, ScopePermission::Admin);
        assert!(result.is_err());
        if let Err(AuthScopeServiceError::PermissionDenied { required, has }) = result {
            assert_eq!(required, ScopePermission::Admin);
            assert!(!has.admin);
        } else {
            panic!("Expected PermissionDenied error");
        }
    }

    #[test]
    fn test_validate_search_count_within_limit() {
        let scope = ApiKeyScope {
            search_limit: 100,
            ..Default::default()
        };
        let result = AuthScopeService::validate_search_count(&scope, 50);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_search_count_exceeds_limit() {
        let scope = ApiKeyScope {
            search_limit: 100,
            ..Default::default()
        };
        let result = AuthScopeService::validate_search_count(&scope, 150);
        assert!(result.is_err());
        if let Err(AuthScopeServiceError::QuotaExceeded(msg)) = result {
            assert!(msg.contains("Search limit exceeded"));
        } else {
            panic!("Expected QuotaExceeded error");
        }
    }

    #[test]
    fn test_validate_scrape_count_exceeds_limit() {
        let scope = ApiKeyScope {
            scrape_limit: 50,
            ..Default::default()
        };
        let result = AuthScopeService::validate_scrape_count(&scope, 100);
        assert!(result.is_err());
        if let Err(AuthScopeServiceError::QuotaExceeded(msg)) = result {
            assert!(msg.contains("Scrape limit exceeded"));
        } else {
            panic!("Expected QuotaExceeded error");
        }
    }

    #[test]
    fn test_validate_search_count_unlimited() {
        let scope = ApiKeyScope {
            search_limit: u32::MAX,
            ..Default::default()
        };
        let result = AuthScopeService::validate_search_count(&scope, 1_000_000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_scrape_count_within_limit() {
        let scope = ApiKeyScope {
            scrape_limit: 50,
            ..Default::default()
        };
        let result = AuthScopeService::validate_scrape_count(&scope, 30);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_scrape_count_unlimited() {
        let scope = ApiKeyScope {
            scrape_limit: u32::MAX,
            ..Default::default()
        };
        let result = AuthScopeService::validate_scrape_count(&scope, 500_000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_merge_scopes_custom_takes_precedence() {
        let team_scope = ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        };
        let custom_scope = ApiKeyScope {
            read: true,
            write: true,
            admin: false,
            search_limit: 200,
            scrape_limit: 100,
        };

        let result = AuthScopeService::merge_scopes(Some(&team_scope), Some(&custom_scope));
        assert!(result.write); // Custom takes precedence
        assert_eq!(result.search_limit, 200);
        assert_eq!(result.scrape_limit, 100);
    }

    #[test]
    fn test_merge_scopes_returns_team_default_when_no_custom() {
        let team_scope = ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        };

        let result = AuthScopeService::merge_scopes(Some(&team_scope), None);
        assert!(!result.write);
        assert_eq!(result.search_limit, 100);
        assert_eq!(result.scrape_limit, 50);
    }

    #[test]
    fn test_merge_scopes_returns_custom_when_no_team() {
        let custom_scope = ApiKeyScope {
            read: true,
            write: true,
            admin: true,
            search_limit: 300,
            scrape_limit: 150,
        };

        let result = AuthScopeService::merge_scopes(None, Some(&custom_scope));
        assert!(result.write);
        assert!(result.admin);
        assert_eq!(result.search_limit, 300);
        assert_eq!(result.scrape_limit, 150);
    }

    #[test]
    fn test_merge_scopes_returns_default_when_neither() {
        let result = AuthScopeService::merge_scopes(None, None);
        assert_eq!(result, ApiKeyScope::default());
    }

    use crate::domain::repositories::auth_scope_repository::RepositoryError;

    /// Minimal mock AuthScopeRepository for AuthScopeService construction tests.
    struct MockAuthScopeRepo;

    #[async_trait]
    impl AuthScopeRepository for MockAuthScopeRepo {
        async fn find_by_api_key_id(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Option<ApiKeyScope>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_api_key(
            &self,
            _key: &str,
        ) -> Result<Option<ApiKeyScope>, RepositoryError> {
            Ok(None)
        }
        async fn upsert(
            &self,
            _api_key_id: Uuid,
            scope: ApiKeyScope,
        ) -> Result<ApiKeyScope, RepositoryError> {
            Ok(scope)
        }
        async fn delete_by_api_key_id(&self, _api_key_id: Uuid) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[test]
    fn test_auth_scope_service_new_clone_and_debug() {
        let repo: Arc<dyn AuthScopeRepository> = Arc::new(MockAuthScopeRepo);
        let service = AuthScopeService::new(repo);
        // Clone impl must produce a working copy sharing the same repo Arc
        let cloned = service.clone();
        // Debug impl must surface the type name and repo field placeholder
        let debug_str = format!("{:?}", service);
        assert!(debug_str.contains("AuthScopeService"));
        assert!(debug_str.contains("AuthScopeRepository"));
        // Cloned instance must also be debuggable (exercises Clone path)
        let cloned_debug = format!("{:?}", cloned);
        assert!(cloned_debug.contains("AuthScopeService"));
    }
}
