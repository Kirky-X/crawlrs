// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for feature flag evaluation

use crate::domain::auth::FeatureFlag;
use crate::infrastructure::database::repositories::feature_flag_repo_impl::FeatureFlagRepository;
use sea_orm::DatabaseConnection;
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum FeatureFlagServiceError {
    #[error("Feature flag not found: {name}")]
    NotFound { name: String },
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
}

/// Service for managing and evaluating feature flags
#[derive(Clone)]
pub struct FeatureFlagService {
    flag_repo: FeatureFlagRepository,
}

impl FeatureFlagService {
    /// Create a new service
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            flag_repo: FeatureFlagRepository::new(db),
        }
    }

    /// Check if a feature is enabled for a specific API Key
    pub async fn is_feature_enabled(
        &self,
        feature_name: &str,
        api_key_id: Uuid,
    ) -> Result<bool, FeatureFlagServiceError> {
        debug!(
            "Checking feature flag: {} for API Key: {}",
            feature_name, api_key_id
        );

        // First check for per-key override
        let flag = match self.flag_repo.find_by_name(feature_name).await? {
            Some(f) => f,
            None => {
                debug!("Feature flag not found: {}", feature_name);
                return Ok(false);
            }
        };

        // Check for API Key override
        let override_ = self.flag_repo.find_override(flag.id, api_key_id).await?;

        if let Some(o) = override_ {
            debug!(
                "Found override for feature {}: enabled={}",
                feature_name, o.enabled
            );
            return Ok(o.enabled);
        }

        // Check global flag status
        let is_enabled = flag.should_enable_for_key(api_key_id);
        debug!(
            "Feature {} enabled for API Key {}: {}",
            feature_name, api_key_id, is_enabled
        );

        Ok(is_enabled)
    }

    /// Get feature flag by name
    pub async fn get_flag(
        &self,
        name: &str,
    ) -> Result<Option<FeatureFlag>, FeatureFlagServiceError> {
        self.flag_repo.find_by_name(name).await.map_err(Into::into)
    }

    /// List all feature flags
    pub async fn list_flags(&self) -> Result<Vec<FeatureFlag>, FeatureFlagServiceError> {
        self.flag_repo.list_all().await.map_err(Into::into)
    }

    /// Set override for an API Key
    pub async fn set_override(
        &self,
        feature_name: &str,
        api_key_id: Uuid,
        enabled: bool,
    ) -> Result<(), FeatureFlagServiceError> {
        let flag = self.flag_repo.find_by_name(feature_name).await?.ok_or(
            FeatureFlagServiceError::NotFound {
                name: feature_name.to_string(),
            },
        )?;

        self.flag_repo
            .set_override(flag.id, api_key_id, enabled)
            .await?;

        debug!(
            "Set feature {} override for API Key {}: {}",
            feature_name, api_key_id, enabled
        );

        Ok(())
    }

    /// Delete override for an API Key
    pub async fn delete_override(
        &self,
        feature_name: &str,
        api_key_id: Uuid,
    ) -> Result<bool, FeatureFlagServiceError> {
        let flag = self.flag_repo.find_by_name(feature_name).await?.ok_or(
            FeatureFlagServiceError::NotFound {
                name: feature_name.to_string(),
            },
        )?;

        let deleted = self.flag_repo.delete_override(flag.id, api_key_id).await?;

        debug!(
            "Deleted feature {} override for API Key {}: {}",
            feature_name, api_key_id, deleted
        );

        Ok(deleted)
    }
}
