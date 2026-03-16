// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Feature flag repository trait for domain layer
//!
//! This module defines the repository interface for feature flag persistence.
//! The trait is defined in the domain layer, while implementations reside in the
//! infrastructure layer, following the Dependency Inversion Principle.

use async_trait::async_trait;
use shaku::Interface;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::auth::{FeatureFlag, FeatureFlagOverride};

/// Feature flag repository error types
#[derive(Debug, Error)]
pub enum FeatureFlagRepositoryError {
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    /// Feature flag not found
    #[error("Feature flag not found: {name}")]
    NotFound { name: String },
    /// Override not found
    #[error("Override not found for feature {feature_flag_id} and API key {api_key_id}")]
    OverrideNotFound {
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    },
}

/// Feature flag repository trait
///
/// Defines the interface for feature flag persistence operations.
/// Implementations should handle database interactions and error mapping.
#[async_trait]
pub trait FeatureFlagRepository: Interface + Send + Sync {
    /// Find a feature flag by its name
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the feature flag
    ///
    /// # Returns
    ///
    /// * `Ok(Some(FeatureFlag))` - Found feature flag
    /// * `Ok(None)` - Feature flag not found
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn find_by_name(&self, name: &str) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError>;

    /// Find a feature flag by its ID
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the feature flag
    ///
    /// # Returns
    ///
    /// * `Ok(Some(FeatureFlag))` - Found feature flag
    /// * `Ok(None)` - Feature flag not found
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn find_by_id(&self, id: Uuid) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError>;

    /// List all feature flags
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<FeatureFlag>)` - List of all feature flags
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn list_all(&self) -> Result<Vec<FeatureFlag>, FeatureFlagRepositoryError>;

    /// Find an override for a specific feature flag and API key
    ///
    /// # Arguments
    ///
    /// * `feature_flag_id` - The ID of the feature flag
    /// * `api_key_id` - The ID of the API key
    ///
    /// # Returns
    ///
    /// * `Ok(Some(FeatureFlagOverride))` - Found override
    /// * `Ok(None)` - Override not found
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn find_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<Option<FeatureFlagOverride>, FeatureFlagRepositoryError>;

    /// List all overrides for a feature flag
    ///
    /// # Arguments
    ///
    /// * `feature_flag_id` - The ID of the feature flag
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<FeatureFlagOverride>)` - List of overrides
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn list_overrides(
        &self,
        feature_flag_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError>;

    /// List all overrides for an API key
    ///
    /// # Arguments
    ///
    /// * `api_key_id` - The ID of the API key
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<FeatureFlagOverride>)` - List of overrides
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn list_overrides_for_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError>;

    /// Set an override for a feature flag and API key
    ///
    /// # Arguments
    ///
    /// * `feature_flag_id` - The ID of the feature flag
    /// * `api_key_id` - The ID of the API key
    /// * `enabled` - Whether the feature is enabled
    ///
    /// # Returns
    ///
    /// * `Ok(FeatureFlagOverride)` - Created or updated override
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn set_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
        enabled: bool,
    ) -> Result<FeatureFlagOverride, FeatureFlagRepositoryError>;

    /// Delete an override for a feature flag and API key
    ///
    /// # Arguments
    ///
    /// * `feature_flag_id` - The ID of the feature flag
    /// * `api_key_id` - The ID of the API key
    ///
    /// # Returns
    ///
    /// * `Ok(bool)` - True if override was deleted, false if not found
    /// * `Err(FeatureFlagRepositoryError)` - Database error
    async fn delete_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<bool, FeatureFlagRepositoryError>;
}
