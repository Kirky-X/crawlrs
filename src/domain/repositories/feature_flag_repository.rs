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
use thiserror::Error;
use uuid::Uuid;

use crate::domain::auth::{FeatureFlag, FeatureFlagOverride};

/// Feature flag repository error types
#[derive(Debug, Error)]
pub enum FeatureFlagRepositoryError {
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),
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

impl From<sea_orm::DbErr> for FeatureFlagRepositoryError {
    fn from(err: sea_orm::DbErr) -> Self {
        FeatureFlagRepositoryError::DatabaseError(err.to_string())
    }
}

/// Feature flag repository trait
///
/// Defines the interface for feature flag persistence operations.
/// Implementations should handle database interactions and error mapping.
#[async_trait]
pub trait FeatureFlagRepository: Send + Sync {
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
    async fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError>;

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
    async fn find_by_id(&self, id: Uuid)
        -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError>;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::{FeatureFlag, FeatureFlagOverride};
    use std::error::Error as _;

    fn make_flag(name: &str, enabled: bool, rollout: u8) -> FeatureFlag {
        FeatureFlag {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: None,
            enabled,
            rollout_percentage: rollout,
            metadata: serde_json::json!({}),
            started_at: None,
            stopped_at: None,
        }
    }

    fn make_override(
        feature_flag_id: Uuid,
        api_key_id: Uuid,
        enabled: bool,
    ) -> FeatureFlagOverride {
        FeatureFlagOverride {
            id: Uuid::new_v4(),
            feature_flag_id,
            api_key_id,
            enabled,
        }
    }

    // ==================== Error Display 测试 ====================

    #[test]
    fn test_database_error_display() {
        let error = FeatureFlagRepositoryError::DatabaseError("connection refused".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Database error"));
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_database_error_empty_message_boundary() {
        let error = FeatureFlagRepositoryError::DatabaseError(String::new());
        let msg = error.to_string();
        assert!(msg.contains("Database error"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_not_found_display() {
        let error = FeatureFlagRepositoryError::NotFound {
            name: "experimental_feature".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Feature flag not found"));
        assert!(msg.contains("experimental_feature"));
    }

    #[test]
    fn test_not_found_empty_name_boundary() {
        let error = FeatureFlagRepositoryError::NotFound {
            name: String::new(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Feature flag not found"));
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_override_not_found_display() {
        let feature_flag_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let error = FeatureFlagRepositoryError::OverrideNotFound {
            feature_flag_id,
            api_key_id,
        };
        let msg = error.to_string();
        assert!(msg.contains("Override not found"));
        assert!(msg.contains(&feature_flag_id.to_string()));
        assert!(msg.contains(&api_key_id.to_string()));
    }

    #[test]
    fn test_override_not_found_nil_uuids_boundary() {
        let error = FeatureFlagRepositoryError::OverrideNotFound {
            feature_flag_id: Uuid::nil(),
            api_key_id: Uuid::nil(),
        };
        let msg = error.to_string();
        assert!(msg.contains("Override not found"));
        // 两个 nil UUID 都应出现在消息中
        let nil_str = "00000000-0000-0000-0000-000000000000";
        assert_eq!(msg.matches(nil_str).count(), 2);
    }

    // ==================== From<sea_orm::DbErr> 转换测试 ====================

    #[test]
    fn test_from_db_err_custom_variant() {
        let db_err = sea_orm::DbErr::Custom("query failed".to_string());
        let error: FeatureFlagRepositoryError = db_err.into();
        match error {
            FeatureFlagRepositoryError::DatabaseError(msg) => {
                assert!(msg.contains("query failed"));
            }
            _ => panic!("expected DatabaseError variant"),
        }
    }

    #[test]
    fn test_from_db_err_record_not_found_variant() {
        let db_err = sea_orm::DbErr::RecordNotFound("task 42".to_string());
        let error: FeatureFlagRepositoryError = db_err.into();
        match error {
            FeatureFlagRepositoryError::DatabaseError(msg) => {
                assert!(msg.contains("task 42"));
            }
            _ => panic!("expected DatabaseError variant"),
        }
    }

    // ==================== std::error::Error trait 测试 ====================

    #[test]
    fn test_all_variants_implement_std_error() {
        fn assert_error<T: std::error::Error>(_: &T) {}
        let errors: Vec<FeatureFlagRepositoryError> = vec![
            FeatureFlagRepositoryError::DatabaseError("e".into()),
            FeatureFlagRepositoryError::NotFound { name: "n".into() },
            FeatureFlagRepositoryError::OverrideNotFound {
                feature_flag_id: Uuid::nil(),
                api_key_id: Uuid::nil(),
            },
        ];
        for err in &errors {
            assert_error(err);
        }
    }

    #[test]
    fn test_source_returns_none_for_all_variants() {
        // FeatureFlagRepositoryError 没有声明 #[source] 字段
        let db_err = FeatureFlagRepositoryError::DatabaseError("e".to_string());
        assert!(db_err.source().is_none());

        let not_found = FeatureFlagRepositoryError::NotFound {
            name: "n".to_string(),
        };
        assert!(not_found.source().is_none());

        let override_not_found = FeatureFlagRepositoryError::OverrideNotFound {
            feature_flag_id: Uuid::nil(),
            api_key_id: Uuid::nil(),
        };
        assert!(override_not_found.source().is_none());
    }

    // ==================== Mock 仓库实现：trait 契约测试 ====================

    /// 内存 mock 仓库用于验证 trait 接口可用性
    struct MockFeatureFlagRepository {
        flags: std::sync::Mutex<std::collections::HashMap<String, FeatureFlag>>,
        overrides: std::sync::Mutex<Vec<FeatureFlagOverride>>,
    }

    impl MockFeatureFlagRepository {
        fn new() -> Self {
            Self {
                flags: std::sync::Mutex::new(std::collections::HashMap::new()),
                overrides: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn insert_flag(&self, flag: FeatureFlag) {
            self.flags.lock().unwrap().insert(flag.name.clone(), flag);
        }
    }

    #[async_trait::async_trait]
    impl FeatureFlagRepository for MockFeatureFlagRepository {
        async fn find_by_name(
            &self,
            name: &str,
        ) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
            Ok(self.flags.lock().unwrap().get(name).cloned())
        }

        async fn find_by_id(
            &self,
            id: Uuid,
        ) -> Result<Option<FeatureFlag>, FeatureFlagRepositoryError> {
            Ok(self
                .flags
                .lock()
                .unwrap()
                .values()
                .find(|f| f.id == id)
                .cloned())
        }

        async fn list_all(&self) -> Result<Vec<FeatureFlag>, FeatureFlagRepositoryError> {
            Ok(self.flags.lock().unwrap().values().cloned().collect())
        }

        async fn find_override(
            &self,
            feature_flag_id: Uuid,
            api_key_id: Uuid,
        ) -> Result<Option<FeatureFlagOverride>, FeatureFlagRepositoryError> {
            Ok(self
                .overrides
                .lock()
                .unwrap()
                .iter()
                .find(|o| o.feature_flag_id == feature_flag_id && o.api_key_id == api_key_id)
                .cloned())
        }

        async fn list_overrides(
            &self,
            feature_flag_id: Uuid,
        ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError> {
            Ok(self
                .overrides
                .lock()
                .unwrap()
                .iter()
                .filter(|o| o.feature_flag_id == feature_flag_id)
                .cloned()
                .collect())
        }

        async fn list_overrides_for_key(
            &self,
            api_key_id: Uuid,
        ) -> Result<Vec<FeatureFlagOverride>, FeatureFlagRepositoryError> {
            Ok(self
                .overrides
                .lock()
                .unwrap()
                .iter()
                .filter(|o| o.api_key_id == api_key_id)
                .cloned()
                .collect())
        }

        async fn set_override(
            &self,
            feature_flag_id: Uuid,
            api_key_id: Uuid,
            enabled: bool,
        ) -> Result<FeatureFlagOverride, FeatureFlagRepositoryError> {
            let mut overrides = self.overrides.lock().unwrap();
            if let Some(existing) = overrides
                .iter_mut()
                .find(|o| o.feature_flag_id == feature_flag_id && o.api_key_id == api_key_id)
            {
                existing.enabled = enabled;
                return Ok(existing.clone());
            }
            let new_override = make_override(feature_flag_id, api_key_id, enabled);
            overrides.push(new_override.clone());
            Ok(new_override)
        }

        async fn delete_override(
            &self,
            feature_flag_id: Uuid,
            api_key_id: Uuid,
        ) -> Result<bool, FeatureFlagRepositoryError> {
            let mut overrides = self.overrides.lock().unwrap();
            let before = overrides.len();
            overrides
                .retain(|o| !(o.feature_flag_id == feature_flag_id && o.api_key_id == api_key_id));
            Ok(overrides.len() < before)
        }
    }

    #[tokio::test]
    async fn test_mock_find_by_name_returns_some_when_present() {
        let repo = MockFeatureFlagRepository::new();
        let flag = make_flag("beta", true, 100);
        repo.insert_flag(flag.clone());

        let found = repo.find_by_name("beta").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, flag.id);
    }

    #[tokio::test]
    async fn test_mock_find_by_name_returns_none_when_absent() {
        let repo = MockFeatureFlagRepository::new();
        let found = repo.find_by_name("missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_mock_find_by_id_returns_some_when_present() {
        let repo = MockFeatureFlagRepository::new();
        let flag = make_flag("beta", true, 100);
        let id = flag.id;
        repo.insert_flag(flag);

        let found = repo.find_by_id(id).await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_mock_list_all_returns_all_inserted_flags() {
        let repo = MockFeatureFlagRepository::new();
        repo.insert_flag(make_flag("a", true, 100));
        repo.insert_flag(make_flag("b", false, 0));
        let list = repo.list_all().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_set_override_creates_new_override() {
        let repo = MockFeatureFlagRepository::new();
        let feature_flag_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();

        let created = repo
            .set_override(feature_flag_id, api_key_id, true)
            .await
            .unwrap();
        assert!(created.enabled);
        assert_eq!(created.feature_flag_id, feature_flag_id);
        assert_eq!(created.api_key_id, api_key_id);
    }

    #[tokio::test]
    async fn test_mock_set_override_updates_existing_override() {
        let repo = MockFeatureFlagRepository::new();
        let feature_flag_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();

        repo.set_override(feature_flag_id, api_key_id, false)
            .await
            .unwrap();
        let updated = repo
            .set_override(feature_flag_id, api_key_id, true)
            .await
            .unwrap();
        assert!(updated.enabled);

        // 验证仍只有一条 override 记录
        let all = repo.list_overrides(feature_flag_id).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_find_override_returns_some_when_present() {
        let repo = MockFeatureFlagRepository::new();
        let feature_flag_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        repo.set_override(feature_flag_id, api_key_id, true)
            .await
            .unwrap();

        let found = repo
            .find_override(feature_flag_id, api_key_id)
            .await
            .unwrap();
        assert!(found.is_some());
        assert!(found.unwrap().enabled);
    }

    #[tokio::test]
    async fn test_mock_find_override_returns_none_when_absent() {
        let repo = MockFeatureFlagRepository::new();
        let found = repo
            .find_override(Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_mock_list_overrides_filters_by_feature_flag_id() {
        let repo = MockFeatureFlagRepository::new();
        let ff1 = Uuid::new_v4();
        let ff2 = Uuid::new_v4();
        let key1 = Uuid::new_v4();
        let key2 = Uuid::new_v4();

        repo.set_override(ff1, key1, true).await.unwrap();
        repo.set_override(ff1, key2, true).await.unwrap();
        repo.set_override(ff2, key1, true).await.unwrap();

        let list_ff1 = repo.list_overrides(ff1).await.unwrap();
        assert_eq!(list_ff1.len(), 2);

        let list_ff2 = repo.list_overrides(ff2).await.unwrap();
        assert_eq!(list_ff2.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_list_overrides_for_key_filters_by_api_key_id() {
        let repo = MockFeatureFlagRepository::new();
        let ff1 = Uuid::new_v4();
        let ff2 = Uuid::new_v4();
        let key1 = Uuid::new_v4();
        let key2 = Uuid::new_v4();

        repo.set_override(ff1, key1, true).await.unwrap();
        repo.set_override(ff2, key1, true).await.unwrap();
        repo.set_override(ff1, key2, true).await.unwrap();

        let list_key1 = repo.list_overrides_for_key(key1).await.unwrap();
        assert_eq!(list_key1.len(), 2);

        let list_key2 = repo.list_overrides_for_key(key2).await.unwrap();
        assert_eq!(list_key2.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_delete_override_returns_true_when_present() {
        let repo = MockFeatureFlagRepository::new();
        let feature_flag_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        repo.set_override(feature_flag_id, api_key_id, true)
            .await
            .unwrap();

        let deleted = repo
            .delete_override(feature_flag_id, api_key_id)
            .await
            .unwrap();
        assert!(deleted);

        // 二次删除应返回 false
        let deleted_again = repo
            .delete_override(feature_flag_id, api_key_id)
            .await
            .unwrap();
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn test_mock_delete_override_returns_false_when_absent() {
        let repo = MockFeatureFlagRepository::new();
        let deleted = repo
            .delete_override(Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap();
        assert!(!deleted);
    }
}
