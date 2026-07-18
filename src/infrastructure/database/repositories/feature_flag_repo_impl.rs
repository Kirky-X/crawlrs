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
    async fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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

    async fn find_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let existing = self.find_override(feature_flag_id, api_key_id).await?;

        let override_id = match existing {
            Some(ref o) => {
                // 已有记录，用 update
                let active_model = crate::infrastructure::database::entities::auth::feature_flag_override::ActiveModel {
                    id: sea_orm::ActiveValue::Unchanged(o.id),
                    feature_flag_id: sea_orm::ActiveValue::Unchanged(feature_flag_id),
                    api_key_id: sea_orm::ActiveValue::Unchanged(api_key_id),
                    enabled: Set(enabled),
                    ..Default::default()
                };
                FfoEntity::update(active_model).exec(conn).await?;
                o.id
            }
            None => {
                // 新记录，用 insert
                let new_id = Uuid::new_v4();
                let active_model = crate::infrastructure::database::entities::auth::feature_flag_override::ActiveModel {
                    id: Set(new_id),
                    feature_flag_id: Set(feature_flag_id),
                    api_key_id: Set(api_key_id),
                    enabled: Set(enabled),
                    ..Default::default()
                };
                FfoEntity::insert(active_model).exec(conn).await?;
                new_id
            }
        };

        Ok(FeatureFlagOverride {
            id: override_id,
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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| FeatureFlagRepositoryError::DatabaseError(e.to_string()))?;

        let result = FfoEntity::delete_many()
            .filter(FfoColumn::FeatureFlagId.eq(feature_flag_id))
            .filter(FfoColumn::ApiKeyId.eq(api_key_id))
            .exec(conn)
            .await?;

        Ok(result.rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a lazy DbPool that does not establish a real database connection.
    /// `get_session()` calls will fail, allowing us to test error paths without
    /// requiring a running PostgreSQL instance.
    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                rt.block_on(dbnexus::DbPool::with_config({
                    let mut cfg = dbnexus::DbConfig::default();
                    cfg.url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
                        "postgres://crawlrs:password@localhost:5443/crawlrs_test".to_string()
                    });
                    cfg
                }))
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    // ============================================================
    // Construction tests
    // ============================================================

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        // Repository should be constructible without connecting to DB
        // (pool is lazy, no connection until get_session is called)
        let _cloned = repo.clone();
    }

    // ============================================================
    // Error path tests — all methods should fail gracefully when
    // the lazy pool cannot provide a real session.
    // ============================================================

    #[tokio::test]
    async fn test_find_by_name_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_by_name("test_flag").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, FeatureFlagRepositoryError::DatabaseError(_)),
            "Expected DatabaseError, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_find_by_id_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_by_id(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_list_all_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.list_all().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_find_override_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_override(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_list_overrides_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.list_overrides(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_list_overrides_for_key_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.list_overrides_for_key(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_set_override_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo
            .set_override(Uuid::new_v4(), Uuid::new_v4(), true)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_delete_override_returns_db_error_without_real_db() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.delete_override(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    // ============================================================
    // FeatureFlagRepositoryError variant tests
    // ============================================================

    #[test]
    fn test_error_database_error_display() {
        let err = FeatureFlagRepositoryError::DatabaseError("conn refused".to_string());
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("conn refused"));
    }

    #[test]
    fn test_error_not_found_display() {
        let err = FeatureFlagRepositoryError::NotFound {
            name: "my_flag".to_string(),
        };
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("my_flag"));
    }

    #[test]
    fn test_error_override_not_found_display() {
        let ff_id = Uuid::new_v4();
        let ak_id = Uuid::new_v4();
        let err = FeatureFlagRepositoryError::OverrideNotFound {
            feature_flag_id: ff_id,
            api_key_id: ak_id,
        };
        assert!(err.to_string().contains("Override not found"));
        assert!(err.to_string().contains(&ff_id.to_string()));
        assert!(err.to_string().contains(&ak_id.to_string()));
    }

    #[test]
    fn test_from_dberr_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Custom("query failed".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("query failed"));
    }

    // ============================================================
    // Additional From<DbErr> variant coverage
    // 覆盖 sea_orm::DbErr 各变体到 FeatureFlagRepositoryError::DatabaseError 的转换
    // ============================================================

    #[test]
    fn test_from_dberr_connection_acquire_timeout_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_connection_acquire_closed_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed);
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_record_not_inserted_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_record_not_updated_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::RecordNotUpdated;
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_query_runtime_internal_to_feature_flag_error() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("syntax error"));
    }

    #[test]
    fn test_from_dberr_query_sqlx_error_to_feature_flag_error() {
        let inner = sea_orm::sqlx::Error::RowNotFound;
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(std::sync::Arc::new(inner)));
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_conn_runtime_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal("conn lost".to_string()));
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("conn lost"));
    }

    #[test]
    fn test_from_dberr_exec_runtime_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Exec(sea_orm::RuntimeErr::Internal("exec failed".to_string()));
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("exec failed"));
    }

    #[test]
    fn test_from_dberr_type_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Type("invalid type".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("invalid type"));
    }

    #[test]
    fn test_from_dberr_json_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Json("parse error".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("parse error"));
    }

    #[test]
    fn test_from_dberr_attr_not_set_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::AttrNotSet("name".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("name"));
    }

    #[test]
    fn test_from_dberr_convert_from_u64_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::ConvertFromU64("String");
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_unpack_insert_id_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::UnpackInsertId;
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_update_get_primary_key_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::UpdateGetPrimaryKey;
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_migration_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::Migration("schema mismatch".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("schema mismatch"));
    }

    #[test]
    fn test_from_dberr_mutex_poison_error_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::MutexPoisonError;
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[test]
    fn test_from_dberr_rbac_error_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::RbacError("forbidden".to_string());
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("forbidden"));
    }

    #[test]
    fn test_from_dberr_access_denied_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::AccessDenied {
            permission: "write".to_string(),
            resource: "feature_flag".to_string(),
        };
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
        assert!(repo_err.to_string().contains("write"));
        assert!(repo_err.to_string().contains("feature_flag"));
    }

    #[test]
    fn test_from_dberr_backend_not_supported_to_feature_flag_error() {
        let db_err = sea_orm::DbErr::BackendNotSupported {
            db: "mysql",
            ctx: "not configured",
        };
        let repo_err: FeatureFlagRepositoryError = db_err.into();
        assert!(matches!(
            repo_err,
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    // ============================================================
    // Repository method error paths — additional boundary cases
    // 即使所有方法在 lazy pool 下都返回 DatabaseError，我们仍然要验证
    // 边界输入不会导致 panic 或非 DatabaseError 变体
    // ============================================================

    #[tokio::test]
    async fn test_find_by_name_with_empty_string_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_by_name("").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_find_by_name_with_unicode_name_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_by_name("功能开关").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_find_by_name_with_long_name_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let long_name = "flag_".repeat(1000);
        let result = repo.find_by_name(&long_name).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_find_by_id_with_nil_uuid_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.find_by_id(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_set_override_with_nil_uuids_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.set_override(Uuid::nil(), Uuid::nil(), true).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_set_override_with_disabled_flag_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo
            .set_override(Uuid::new_v4(), Uuid::new_v4(), false)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    async fn test_delete_override_with_nil_uuids_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool);
        let result = repo.delete_override(Uuid::nil(), Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FeatureFlagRepositoryError::DatabaseError(_)
        ));
    }

    // ============================================================
    // Error variant Display — 精确消息内容验证
    // ============================================================

    #[test]
    fn test_error_database_error_display_exact_prefix() {
        let err = FeatureFlagRepositoryError::DatabaseError("conn refused".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Database error: conn refused");
    }

    #[test]
    fn test_error_database_error_display_empty_message_boundary() {
        let err = FeatureFlagRepositoryError::DatabaseError(String::new());
        assert_eq!(err.to_string(), "Database error: ");
    }

    #[test]
    fn test_error_database_error_display_long_message_boundary() {
        let long_msg = "x".repeat(2000);
        let err = FeatureFlagRepositoryError::DatabaseError(long_msg.clone());
        let msg = err.to_string();
        assert!(msg.contains(&long_msg));
    }

    #[test]
    fn test_error_not_found_display_exact() {
        let err = FeatureFlagRepositoryError::NotFound {
            name: "my_flag".to_string(),
        };
        assert_eq!(err.to_string(), "Feature flag not found: my_flag");
    }

    #[test]
    fn test_error_not_found_with_empty_name_boundary() {
        let err = FeatureFlagRepositoryError::NotFound {
            name: String::new(),
        };
        let msg = err.to_string();
        assert_eq!(msg, "Feature flag not found: ");
    }

    #[test]
    fn test_error_override_not_found_with_nil_uuids_boundary() {
        let err = FeatureFlagRepositoryError::OverrideNotFound {
            feature_flag_id: Uuid::nil(),
            api_key_id: Uuid::nil(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Override not found"));
        let nil_str = "00000000-0000-0000-0000-000000000000";
        assert_eq!(msg.matches(nil_str).count(), 2);
    }

    #[test]
    fn test_error_implements_debug_for_all_variants() {
        // 验证 #[derive(Debug)] 对所有变体生效
        let variants: Vec<FeatureFlagRepositoryError> = vec![
            FeatureFlagRepositoryError::DatabaseError("e".into()),
            FeatureFlagRepositoryError::NotFound { name: "n".into() },
            FeatureFlagRepositoryError::OverrideNotFound {
                feature_flag_id: Uuid::nil(),
                api_key_id: Uuid::nil(),
            },
        ];
        for err in &variants {
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty());
        }
    }

    // ============================================================
    // RepositoryImpl accessor / Clone tests
    // ============================================================

    #[test]
    fn test_repository_clone_preserves_pool_identity() {
        let pool = create_test_db_pool();
        let repo = FeatureFlagRepositoryImpl::new(pool.clone());
        let cloned = repo.clone();
        // clone 后 inner pool 应该是同一个 Arc
        assert!(Arc::ptr_eq(&cloned.pool, &pool));
    }

    #[test]
    fn test_new_with_distinct_pools_do_not_share_identity() {
        // 两个独立的 lazy pool 不应共享 Arc 标识
        let pool1 = create_test_db_pool();
        let pool2 = create_test_db_pool();
        let repo1 = FeatureFlagRepositoryImpl::new(pool1);
        let repo2 = FeatureFlagRepositoryImpl::new(pool2);
        assert!(!Arc::ptr_eq(&repo1.pool, &repo2.pool));
    }
}
