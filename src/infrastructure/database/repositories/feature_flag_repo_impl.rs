// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Feature flag repository implementation using dbnexus
//!
//! This module provides the concrete implementation of the FeatureFlagRepository trait
//! defined in the domain layer.

use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::auth::{FeatureFlag, FeatureFlagOverride};
use crate::domain::repositories::feature_flag_repository::{
    FeatureFlagRepository, FeatureFlagRepositoryError,
};
use crate::infrastructure::database::entities::auth::feature_flag::{
    Column as FfColumn, Entity as FfEntity,
};
use crate::infrastructure::database::entities::auth::feature_flag_override::{
    Column as FfoColumn, Entity as FfoEntity,
};

/// Feature flag repository implementation using dbnexus Session
#[derive(Clone)]
pub struct FeatureFlagRepositoryImpl {
    pool: Arc<DbPool>,
}

impl FeatureFlagRepositoryImpl {
    /// Create a new feature flag repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FeatureFlagRepository for FeatureFlagRepositoryImpl {
    async fn find_by_name(&self, name: &str) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let flag = FfEntity::find()
            .filter(FfColumn::Name.eq(name))
            .one(conn)
            .await?;

        Ok(flag.map(|f| FeatureFlag {
            id: f.id,
            name: f.name,
            description: f.description,
            enabled: f.enabled,
            rollout_percentage: f.rollout_percentage as u8,
            metadata: f.metadata,
            started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
            stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
        }))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let flag = FfEntity::find_by_id(id).one(conn).await?;

        Ok(flag.map(|f| FeatureFlag {
            id: f.id,
            name: f.name,
            description: f.description,
            enabled: f.enabled,
            rollout_percentage: f.rollout_percentage as u8,
            metadata: f.metadata,
            started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
            stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
        }))
    }

    async fn list_all(&self) -> Result<Vec<FeatureFlag>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let flags = FfEntity::find().all(conn).await?;

        Ok(flags
            .into_iter()
            .map(|f| FeatureFlag {
                id: f.id,
                name: f.name,
                description: f.description,
                enabled: f.enabled,
                rollout_percentage: f.rollout_percentage as u8,
                metadata: f.metadata,
                started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
                stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
            })
            .collect())
    }

    async fn find_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<Option<FeatureFlagOverride>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let override_ = FfoEntity::find()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .one(conn)
            .await?;

        Ok(override_.map(|o| FeatureFlagOverride {
            id: o.id,
            feature_flag_id: o.feature_flag_id,
            api_key_id: o.api_key_id,
            enabled: o.enabled,
        }))
    }

    async fn list_overrides(
        &self,
        feature_flag_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let overrides = FfoEntity::find()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .all(conn)
            .await?;

        Ok(overrides
            .into_iter()
            .map(|o| FeatureFlagOverride {
                id: o.id,
                feature_flag_id: o.feature_flag_id,
                api_key_id: o.api_key_id,
                enabled: o.enabled,
            })
            .collect())
    }

    async fn list_overrides_for_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let overrides = FfoEntity::find()
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .all(conn)
            .await?;

        Ok(overrides
            .into_iter()
            .map(|o| FeatureFlagOverride {
                id: o.id,
                feature_flag_id: o.feature_flag_id,
                api_key_id: o.api_key_id,
                enabled: o.enabled,
            })
            .collect())
    }

    async fn set_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
        enabled: bool,
    ) -> Result<FeatureFlagOverride, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let existing = self.find_override(feature_flag_id, api_key_id).await?;

        let override_model = match existing {
            Some(ref o) => {
                crate::infrastructure::database::entities::auth::feature_flag_override::ActiveModel {
                    id: sea_orm::ActiveValue::Unchanged(o.id),
                    feature_flag_id: sea_orm::ActiveValue::Unchanged(feature_flag_id),
                    api_key_id: sea_orm::ActiveValue::Unchanged(api_key_id),
                    enabled: Set(enabled),
                    ..Default::default()
                }
            }
            None => {
                crate::infrastructure::database::entities::auth::feature_flag_override::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    feature_flag_id: Set(feature_flag_id),
                    api_key_id: Set(api_key_id),
                    enabled: Set(enabled),
                    ..Default::default()
                }
            }
        };

        FfoEntity::update(override_model).exec(conn).await?;

        Ok(FeatureFlagOverride {
            id: existing.map(|o| o.id).unwrap_or_else(Uuid::new_v4),
            feature_flag_id,
            api_key_id,
            enabled,
        })
    }

    async fn delete_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<bool, FeatureFlagRepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session.connection().map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let result = FfoEntity::delete_many()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .exec(conn)
            .await?;

        Ok(result.rows_affected > 0)
    }
}
