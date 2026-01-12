// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::domain::auth::{FeatureFlag, FeatureFlagOverride};
use crate::infrastructure::database::entities::auth::feature_flag::{
    Column as FfColumn, Entity as FfEntity,
};
use crate::infrastructure::database::entities::auth::feature_flag_override::{
    Column as FfoColumn, Entity as FfoEntity,
};

#[derive(Clone)]
pub struct FeatureFlagRepository {
    db: DatabaseConnection,
}

impl FeatureFlagRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_name(&self, name: &str) -> Result<Option<FeatureFlag>, sea_orm::DbErr> {
        let flag = FfEntity::find()
            .filter(FfColumn::Name.eq(name))
            .one(&self.db)
            .await?;

        Ok(flag.map(|f| FeatureFlag {
            id: f.id,
            name: f.name,
            description: f.description,
            enabled: f.enabled,
            rollout_percentage: f.rollout_percentage as u8,
            metadata: serde_json::Value::from(f.metadata),
            started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
            stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
        }))
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<FeatureFlag>, sea_orm::DbErr> {
        let flag = FfEntity::find_by_id(id).one(&self.db).await?;
        Ok(flag.map(|f| FeatureFlag {
            id: f.id,
            name: f.name,
            description: f.description,
            enabled: f.enabled,
            rollout_percentage: f.rollout_percentage as u8,
            metadata: serde_json::Value::from(f.metadata),
            started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
            stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
        }))
    }

    pub async fn list_all(&self) -> Result<Vec<FeatureFlag>, sea_orm::DbErr> {
        let flags = FfEntity::find().all(&self.db).await?;
        Ok(flags
            .into_iter()
            .map(|f| FeatureFlag {
                id: f.id,
                name: f.name,
                description: f.description,
                enabled: f.enabled,
                rollout_percentage: f.rollout_percentage as u8,
                metadata: serde_json::Value::from(f.metadata),
                started_at: f.started_at.map(|t| t.with_timezone(&Utc)),
                stopped_at: f.stopped_at.map(|t| t.with_timezone(&Utc)),
            })
            .collect())
    }

    pub async fn find_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<Option<FeatureFlagOverride>, sea_orm::DbErr> {
        let override_ = FfoEntity::find()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .one(&self.db)
            .await?;

        Ok(override_.map(|o| FeatureFlagOverride {
            id: o.id,
            feature_flag_id: o.feature_flag_id,
            api_key_id: o.api_key_id,
            enabled: o.enabled,
        }))
    }

    pub async fn list_overrides(
        &self,
        feature_flag_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, sea_orm::DbErr> {
        let overrides = FfoEntity::find()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .all(&self.db)
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

    pub async fn list_overrides_for_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<FeatureFlagOverride>, sea_orm::DbErr> {
        let overrides = FfoEntity::find()
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .all(&self.db)
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

    pub async fn set_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
        enabled: bool,
    ) -> Result<FeatureFlagOverride, sea_orm::DbErr> {
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

        FfoEntity::update(override_model).exec(&self.db).await?;

        Ok(FeatureFlagOverride {
            id: existing.map(|o| o.id).unwrap_or_else(Uuid::new_v4),
            feature_flag_id,
            api_key_id,
            enabled,
        })
    }

    pub async fn delete_override(
        &self,
        feature_flag_id: Uuid,
        api_key_id: Uuid,
    ) -> Result<bool, sea_orm::DbErr> {
        let result = FfoEntity::delete_many()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .exec(&self.db)
            .await?;

        Ok(result.rows_affected > 0)
    }
}
