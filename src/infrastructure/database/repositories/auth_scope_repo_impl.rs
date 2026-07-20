// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::common::time_utils;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::auth::ApiKeyScope;
use crate::domain::repositories::auth_scope_repository::{AuthScopeRepository, RepositoryError};
use crate::infrastructure::database::entities::api_key::{
    Column as ApiKeyColumn, Entity as ApiKeyEntity,
};
use crate::infrastructure::database::entities::auth::scope::{
    Column as ScopeColumn, Entity as ScopeEntity,
};

#[derive(Clone)]
pub struct AuthScopeRepositoryImpl {
    pool: Arc<DbPool>,
}

impl AuthScopeRepositoryImpl {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AuthScopeRepository for AuthScopeRepositoryImpl {
    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyScope>, RepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let scope = ScopeEntity::find()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .one(conn)
            .await?;

        Ok(scope.map(|s| ApiKeyScope {
            read: s.read,
            write: s.write,
            admin: s.admin,
            search_limit: s.search_limit as u32,
            scrape_limit: s.scrape_limit as u32,
        }))
    }

    async fn find_by_api_key(&self, key: &str) -> Result<Option<ApiKeyScope>, RepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let api_key = ApiKeyEntity::find()
            .filter(ApiKeyColumn::Key.eq(key))
            .one(conn)
            .await?;

        match api_key {
            Some(key) => self.find_by_api_key_id(key.id).await,
            None => Ok(None),
        }
    }

    async fn upsert(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, RepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let existing = self.find_by_api_key_id(api_key_id).await?;

        match existing {
            Some(_existing_scope) => {
                // Update existing scope - need to get the ID from existing record
                let existing_model = ScopeEntity::find()
                    .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
                    .one(conn)
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::NotFound(format!(
                            "Scope not found for api_key_id: {}",
                            api_key_id
                        ))
                    })?;

                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        id: sea_orm::ActiveValue::Unchanged(existing_model.id),
                        api_key_id: sea_orm::ActiveValue::Unchanged(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        created_at: sea_orm::ActiveValue::Unchanged(existing_model.created_at),
                        updated_at: Set(Utc::now().with_timezone(&time_utils::UTC_OFFSET)),
                    };

                ScopeEntity::update(scope_active_model).exec(conn).await?;

                Ok(scope)
            }
            None => {
                let now = Utc::now().with_timezone(&time_utils::UTC_OFFSET);
                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        id: Set(Uuid::new_v4()),
                        api_key_id: Set(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        created_at: Set(now),
                        updated_at: Set(now),
                    };

                ScopeEntity::insert(scope_active_model).exec(conn).await?;
                Ok(scope)
            }
        }
    }

    async fn delete_by_api_key_id(&self, api_key_id: Uuid) -> Result<bool, RepositoryError> {
        let session = self.pool.get_session("admin").await?;

        let conn = session.connection()?;

        let result = ScopeEntity::delete_many()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .exec(conn)
            .await?;

        Ok(result.rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::auth::ApiKeyScope;

    fn sample_scope() -> ApiKeyScope {
        ApiKeyScope {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }

    // ========== construction ==========

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = AuthScopeRepositoryImpl::new(pool);
        // Repository should be constructible with a real pool
        let _clone = repo.clone();
    }

    // ========== CRUD against real DB ==========

    #[tokio::test]
    async fn test_find_by_api_key_id_returns_none_for_unknown() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key_id(Uuid::new_v4()).await;
        assert!(
            result.is_ok(),
            "find_by_api_key_id failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_none(),
            "unknown api_key_id should return None"
        );
    }

    #[tokio::test]
    async fn test_find_by_api_key_returns_none_for_unknown() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key("sk-test-key").await;
        assert!(result.is_ok(), "find_by_api_key failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "unknown key should return None");
    }

    #[tokio::test]
    async fn test_find_by_api_key_with_empty_key_returns_none() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key("").await;
        assert!(result.is_ok(), "find_by_api_key failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "empty key should return None");
    }

    #[tokio::test]
    async fn test_upsert_creates_new_scope() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let api_key_id = Uuid::new_v4();
        let expected = sample_scope();
        let result = repo.upsert(api_key_id, expected.clone()).await;
        assert!(result.is_ok(), "upsert failed: {:?}", result.err());

        // Verify DB state: find_by_api_key_id should return the created scope
        let found = repo
            .find_by_api_key_id(api_key_id)
            .await
            .expect("find_by_api_key_id failed")
            .expect("scope should exist after upsert");
        assert_eq!(found.read, expected.read);
        assert_eq!(found.write, expected.write);
        assert_eq!(found.admin, expected.admin);
        assert_eq!(found.search_limit, expected.search_limit);
        assert_eq!(found.scrape_limit, expected.scrape_limit);
    }

    #[tokio::test]
    async fn test_delete_by_api_key_id_returns_false_for_unknown() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.delete_by_api_key_id(Uuid::new_v4()).await;
        assert!(
            result.is_ok(),
            "delete_by_api_key_id failed: {:?}",
            result.err()
        );
        assert!(!result.unwrap(), "unknown api_key_id should return false");
    }

    // ========== RepositoryError variants ==========

    #[test]
    fn test_repository_error_database_display() {
        let err = RepositoryError::Database(sea_orm::DbErr::RecordNotFound(
            "scope not found".to_string(),
        ));
        assert!(format!("{}", err).contains("Database error"));
        assert!(format!("{}", err).contains("scope not found"));
    }

    #[test]
    fn test_repository_error_not_found_display() {
        let err = RepositoryError::NotFound("api key".to_string());
        assert_eq!(format!("{}", err), "Not found: api key");
    }

    #[test]
    fn test_repository_error_from_dberr() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    // ========== From<dbnexus::DbError> exhaustive coverage ==========

    #[test]
    fn test_repository_error_from_dbnexus_db_error_connection() {
        use sea_orm::ConnAcquireErr;
        let inner = sea_orm::DbErr::ConnectionAcquire(ConnAcquireErr::Timeout);
        let db_err = dbnexus::DbError::Connection(inner);
        let repo_err: RepositoryError = db_err.into();
        // Connection variant should map to Database variant preserving the inner DbErr
        match repo_err {
            RepositoryError::Database(sea_orm::DbErr::ConnectionAcquire(_)) => {}
            other => panic!("expected Database(ConnectionAcquire), got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_config() {
        let db_err = dbnexus::DbError::Config("invalid url".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Config"));
                assert!(msg.contains("invalid url"));
            }
            other => panic!("expected Database(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_permission() {
        let db_err = dbnexus::DbError::Permission("forbidden".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Permission"));
                assert!(msg.contains("forbidden"));
            }
            other => panic!("expected Database(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_transaction() {
        let db_err = dbnexus::DbError::Transaction("deadlock".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Transaction"));
                assert!(msg.contains("deadlock"));
            }
            other => panic!("expected Database(Custom), got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dbnexus_db_error_migration() {
        let db_err = dbnexus::DbError::Migration("schema mismatch".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(sea_orm::DbErr::Custom(msg)) => {
                assert!(msg.contains("Migration"));
                assert!(msg.contains("schema mismatch"));
            }
            other => panic!("expected Database(Custom), got {:?}", other),
        }
    }

    // ========== sample_scope construction & ApiKeyScope boundaries ==========

    #[test]
    fn test_sample_scope_construction() {
        let scope = sample_scope();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_api_key_scope_all_permissions_enabled() {
        let scope = ApiKeyScope {
            read: true,
            write: true,
            admin: true,
            search_limit: u32::MAX,
            scrape_limit: u32::MAX,
        };
        assert!(scope.read && scope.write && scope.admin);
        assert_eq!(scope.search_limit, u32::MAX);
        assert_eq!(scope.scrape_limit, u32::MAX);
    }

    #[test]
    fn test_api_key_scope_all_permissions_disabled() {
        let scope = ApiKeyScope {
            read: false,
            write: false,
            admin: false,
            search_limit: 0,
            scrape_limit: 0,
        };
        assert!(!scope.read && !scope.write && !scope.admin);
        assert_eq!(scope.search_limit, 0);
        assert_eq!(scope.scrape_limit, 0);
    }

    #[test]
    fn test_api_key_scope_admin_only() {
        let scope = ApiKeyScope {
            read: false,
            write: false,
            admin: true,
            search_limit: 0,
            scrape_limit: 0,
        };
        assert!(!scope.read && !scope.write && scope.admin);
    }

    // ========== additional error path variants ==========

    #[tokio::test]
    async fn test_find_by_api_key_with_unicode_key_returns_none() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key("sk-测试-キー-🔑").await;
        assert!(result.is_ok(), "find_by_api_key failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "unicode key should return None");
    }

    #[tokio::test]
    async fn test_find_by_api_key_with_long_key_returns_none() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let long_key = "sk-".to_string() + &"a".repeat(10_000);
        let result = repo.find_by_api_key(&long_key).await;
        assert!(result.is_ok(), "find_by_api_key failed: {:?}", result.err());
        assert!(result.unwrap().is_none(), "long key should return None");
    }

    #[tokio::test]
    async fn test_upsert_with_admin_scope_succeeds() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let admin_scope = ApiKeyScope {
            read: true,
            write: true,
            admin: true,
            search_limit: 1000,
            scrape_limit: 500,
        };
        let result = repo.upsert(Uuid::new_v4(), admin_scope).await;
        assert!(result.is_ok(), "upsert failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_upsert_with_zero_limits_succeeds() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let zero_scope = ApiKeyScope {
            read: false,
            write: false,
            admin: false,
            search_limit: 0,
            scrape_limit: 0,
        };
        let result = repo.upsert(Uuid::new_v4(), zero_scope).await;
        assert!(result.is_ok(), "upsert failed: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_delete_by_api_key_id_with_nil_uuid_returns_false() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.delete_by_api_key_id(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "delete_by_api_key_id failed: {:?}",
            result.err()
        );
        assert!(!result.unwrap(), "nil UUID should return false");
    }

    #[tokio::test]
    async fn test_find_by_api_key_id_with_nil_uuid_returns_none() {
        let repo = AuthScopeRepositoryImpl::new(create_test_db_pool());
        let result = repo.find_by_api_key_id(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "find_by_api_key_id failed: {:?}",
            result.err()
        );
        assert!(result.unwrap().is_none(), "nil UUID should return None");
    }

    // ========== additional From<sea_orm::DbErr> variant coverage ==========

    #[test]
    fn test_repository_error_from_dberr_record_not_found() {
        let db_err = sea_orm::DbErr::RecordNotFound("scope not found".to_string());
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_connection_acquire() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_record_not_inserted() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err: RepositoryError = db_err.into();
        match repo_err {
            RepositoryError::Database(_) => {}
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_repository_error_from_dberr_custom_display() {
        let db_err = sea_orm::DbErr::Custom("custom failure".to_string());
        let repo_err: RepositoryError = db_err.into();
        let msg = format!("{}", repo_err);
        assert!(msg.contains("Database error"));
        assert!(msg.contains("custom failure"));
    }

    // ========== RepositoryError::NotFound with various messages ==========

    #[test]
    fn test_repository_error_not_found_with_empty_message() {
        let err = RepositoryError::NotFound("".to_string());
        assert_eq!(format!("{}", err), "Not found: ");
    }

    #[test]
    fn test_repository_error_not_found_with_long_message() {
        let long_msg = "x".repeat(1000);
        let err = RepositoryError::NotFound(long_msg.clone());
        let msg = format!("{}", err);
        assert!(msg.contains(&long_msg));
    }
}
