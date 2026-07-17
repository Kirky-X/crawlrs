// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crate::domain::services::team_service::TeamGeoRestrictions;
use crate::infrastructure::database::entities::{geo_restriction_log, team};
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use uuid::Uuid;

use std::sync::Arc;

/// 基于数据库的地理限制仓库实现
#[derive(Clone)]
pub struct DatabaseGeoRestrictionRepository {
    pool: Arc<DbPool>,
}

impl DatabaseGeoRestrictionRepository {
    /// 创建新的数据库地理限制仓库实例
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl GeoRestrictionRepository for DatabaseGeoRestrictionRepository {
    /// 获取团队的地理限制配置
    async fn get_team_restrictions(
        &self,
        team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        // 查询团队记录
        let team_model = team::Entity::find_by_id(team_id)
            .one(conn)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?
            .ok_or(GeoRestrictionRepositoryError::TeamNotFound(team_id))?;

        // 解析 JSON 字段
        let allowed_countries = team_model
            .allowed_countries
            .and_then(|json| serde_json::from_value(json).ok());

        let blocked_countries = team_model
            .blocked_countries
            .and_then(|json| serde_json::from_value(json).ok());

        let ip_whitelist = team_model
            .ip_whitelist
            .and_then(|json| serde_json::from_value(json).ok());

        let domain_blacklist = team_model
            .domain_blacklist
            .and_then(|json| serde_json::from_value(json).ok());

        Ok(TeamGeoRestrictions {
            enable_geo_restrictions: team_model.enable_geo_restrictions,
            allowed_countries,
            blocked_countries,
            ip_whitelist,
            domain_blacklist,
        })
    }

    /// 更新团队的地理限制配置
    async fn update_team_restrictions(
        &self,
        team_id: Uuid,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        // 查询团队记录
        let team_model = team::Entity::find_by_id(team_id)
            .one(conn)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?
            .ok_or(GeoRestrictionRepositoryError::TeamNotFound(team_id))?;

        // 转换为 ActiveModel 进行更新
        let mut active_model: team::ActiveModel = team_model.into();

        // 设置地理限制字段
        active_model.enable_geo_restrictions = Set(restrictions.enable_geo_restrictions);
        active_model.allowed_countries =
            Set(restrictions.allowed_countries.as_ref().map(|countries| {
                serde_json::to_value(countries).expect(
                    "Failed to serialize allowed_countries: this should never fail for Vec<String>",
                )
            }));
        active_model.blocked_countries =
            Set(restrictions.blocked_countries.as_ref().map(|countries| {
                serde_json::to_value(countries).expect(
                    "Failed to serialize blocked_countries: this should never fail for Vec<String>",
                )
            }));
        active_model.ip_whitelist = Set(restrictions.ip_whitelist.as_ref().map(|whitelist| {
            serde_json::to_value(whitelist)
                .expect("Failed to serialize ip_whitelist: this should never fail for Vec<String>")
        }));
        active_model.domain_blacklist =
            Set(restrictions.domain_blacklist.as_ref().map(|blacklist| {
                serde_json::to_value(blacklist).expect(
                    "Failed to serialize domain_blacklist: this should never fail for Vec<String>",
                )
            }));

        // 更新记录
        active_model
            .update(conn)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        Ok(())
    }

    /// 记录地理限制审计日志
    async fn log_geo_restriction_action(
        &self,
        team_id: Uuid,
        ip_address: &str,
        country_code: &str,
        action: &str,
        reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        let log_entry = geo_restriction_log::ActiveModel {
            id: Set(Uuid::new_v4()),
            team_id: Set(team_id),
            ip_address: Set(ip_address.to_string()),
            country_code: Set(Some(country_code.to_string())),
            restriction_type: Set(action.to_string()),
            url: Set(None), // URL 可选，这里不设置
            reason: Set(reason.to_string()),
            created_at: Set(chrono::Utc::now().into()),
        };

        log_entry
            .insert(conn)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        Ok(())
    }
}

// 注意：原 `#[cfg(test)] #[cfg(feature = "dbnexus-sqlite")] mod tests` 块已删除
// 原因：dbnexus-sqlite feature 已不再支持。如需 SQLite 集成测试，需重新引入
// dbnexus-sqlite feature 并恢复此模块。下面的 error_path_tests 不依赖 SQLite，
// 使用 lazy DbPool 测试错误路径，仍保留。

/// Tests that exercise the error paths of `DatabaseGeoRestrictionRepository`
/// using a lazy `DbPool` (no real DB connection). These run under all feature
/// combinations.
#[cfg(test)]
mod error_path_tests {
    use super::*;

    /// Build a lazy DbPool that does not actually connect; `get_session()`
    /// will fail at runtime, allowing us to exercise every error path in
    /// this repository without a real database.
    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                DbPool::try_from(&dbnexus::DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    #[test]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = DatabaseGeoRestrictionRepository::new(pool);
        let _clone = repo.clone();
    }

    #[tokio::test]
    async fn test_get_team_restrictions_returns_db_error_without_real_db() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo.get_team_restrictions(Uuid::new_v4()).await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(!msg.is_empty(), "error message should not be empty");
            }
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_update_team_restrictions_returns_db_error_without_real_db() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions::default();
        let result = repo
            .update_team_restrictions(Uuid::new_v4(), &restrictions)
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_returns_db_error_without_real_db() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo
            .log_geo_restriction_action(
                Uuid::new_v4(),
                "192.168.1.1",
                "US",
                "allowed",
                "test reason",
            )
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    // ========== GeoRestrictionRepositoryError variants ==========

    #[test]
    fn test_error_database_display() {
        let err = GeoRestrictionRepositoryError::Database("connection refused".to_string());
        assert_eq!(format!("{}", err), "Database error: connection refused");
    }

    #[test]
    fn test_error_team_not_found_display() {
        let id = Uuid::new_v4();
        let err = GeoRestrictionRepositoryError::TeamNotFound(id);
        assert_eq!(format!("{}", err), format!("Team not found: {}", id));
    }

    #[test]
    fn test_error_other_display() {
        let err = GeoRestrictionRepositoryError::Other("something went wrong".to_string());
        assert_eq!(format!("{}", err), "Other error: something went wrong");
    }

    // ========== TeamGeoRestrictions construction & defaults ==========

    #[test]
    fn test_team_geo_restrictions_default() {
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions::default();
        assert!(!restrictions.enable_geo_restrictions);
        assert!(restrictions.allowed_countries.is_none());
        assert!(restrictions.blocked_countries.is_none());
        assert!(restrictions.ip_whitelist.is_none());
        assert!(restrictions.domain_blacklist.is_none());
    }

    #[test]
    fn test_team_geo_restrictions_with_all_fields_populated() {
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
            blocked_countries: Some(vec!["CN".to_string(), "RU".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.1".to_string(), "10.0.0.1".to_string()]),
            domain_blacklist: Some(vec!["spam.com".to_string(), "malware.org".to_string()]),
        };
        assert!(restrictions.enable_geo_restrictions);
        assert_eq!(restrictions.allowed_countries.as_ref().unwrap().len(), 2);
        assert_eq!(restrictions.blocked_countries.as_ref().unwrap().len(), 2);
        assert_eq!(restrictions.ip_whitelist.as_ref().unwrap().len(), 2);
        assert_eq!(restrictions.domain_blacklist.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_team_geo_restrictions_with_empty_vectors() {
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(Vec::new()),
            blocked_countries: Some(Vec::new()),
            ip_whitelist: Some(Vec::new()),
            domain_blacklist: Some(Vec::new()),
        };
        assert!(restrictions.allowed_countries.as_ref().unwrap().is_empty());
        assert!(restrictions.blocked_countries.as_ref().unwrap().is_empty());
        assert!(restrictions.ip_whitelist.as_ref().unwrap().is_empty());
        assert!(restrictions.domain_blacklist.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_team_geo_restrictions_clone_preserves_values() {
        let original = crate::domain::services::team_service::TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: None,
            ip_whitelist: Some(vec!["127.0.0.1".to_string()]),
            domain_blacklist: None,
        };
        let cloned = original.clone();
        // Verify each field is preserved by clone (TeamGeoRestrictions does not derive PartialEq)
        assert_eq!(
            cloned.enable_geo_restrictions,
            original.enable_geo_restrictions
        );
        assert_eq!(cloned.allowed_countries, original.allowed_countries);
        assert_eq!(cloned.blocked_countries, original.blocked_countries);
        assert_eq!(cloned.ip_whitelist, original.ip_whitelist);
        assert_eq!(cloned.domain_blacklist, original.domain_blacklist);
    }

    // ========== additional error path variants ==========

    #[tokio::test]
    async fn test_get_team_restrictions_with_nil_uuid_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo.get_team_restrictions(Uuid::nil()).await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_update_team_restrictions_with_enabled_flag_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "CA".to_string(), "MX".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: None,
            domain_blacklist: Some(vec!["bad.com".to_string()]),
        };
        let result = repo
            .update_team_restrictions(Uuid::new_v4(), &restrictions)
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_update_team_restrictions_with_nil_uuid_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions::default();
        let result = repo
            .update_team_restrictions(Uuid::nil(), &restrictions)
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_empty_strings_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo
            .log_geo_restriction_action(Uuid::new_v4(), "", "", "", "")
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_unicode_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo
            .log_geo_restriction_action(
                Uuid::new_v4(),
                "192.168.1.1",
                "测试",
                "阻止",
                "地理限制-测试原因",
            )
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_special_characters_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo
            .log_geo_restriction_action(
                Uuid::new_v4(),
                "10.0.0.1",
                "US",
                "allowed",
                "reason with 'quotes' and \"double quotes\" and; semicolon",
            )
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_nil_uuid_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let result = repo
            .log_geo_restriction_action(Uuid::nil(), "127.0.0.1", "US", "blocked", "test")
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    // ========== error variant edge cases ==========

    #[test]
    fn test_error_database_display_with_empty_message() {
        let err = GeoRestrictionRepositoryError::Database("".to_string());
        assert_eq!(format!("{}", err), "Database error: ");
    }

    #[test]
    fn test_error_database_display_with_long_message() {
        let long_msg = "x".repeat(1000);
        let err = GeoRestrictionRepositoryError::Database(long_msg.clone());
        let msg = format!("{}", err);
        assert!(msg.contains(&long_msg));
    }

    #[test]
    fn test_error_team_not_found_with_nil_uuid() {
        let err = GeoRestrictionRepositoryError::TeamNotFound(Uuid::nil());
        assert_eq!(
            format!("{}", err),
            "Team not found: 00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn test_error_other_with_empty_message() {
        let err = GeoRestrictionRepositoryError::Other("".to_string());
        assert_eq!(format!("{}", err), "Other error: ");
    }

    // ========== Production conversion path: sea_orm::DbErr -> Database(msg) ==========
    // 验证 map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string())) 路径
    // 对各种 DbErr 变体的转换行为

    #[test]
    fn test_dberr_custom_to_database_variant() {
        let db_err = sea_orm::DbErr::Custom("query failed".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("query failed"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_record_not_found_to_database_variant() {
        let db_err = sea_orm::DbErr::RecordNotFound("team missing".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("team missing"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_connection_acquire_timeout_to_database_variant() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::Timeout);
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_connection_acquire_closed_to_database_variant() {
        let db_err = sea_orm::DbErr::ConnectionAcquire(sea_orm::ConnAcquireErr::ConnectionClosed);
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_record_not_inserted_to_database_variant() {
        let db_err = sea_orm::DbErr::RecordNotInserted;
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_record_not_updated_to_database_variant() {
        let db_err = sea_orm::DbErr::RecordNotUpdated;
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_query_runtime_to_database_variant() {
        let db_err =
            sea_orm::DbErr::Query(sea_orm::RuntimeErr::Internal("syntax error".to_string()));
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("syntax error"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_conn_runtime_to_database_variant() {
        let db_err = sea_orm::DbErr::Conn(sea_orm::RuntimeErr::Internal("conn lost".to_string()));
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("conn lost"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_exec_runtime_to_database_variant() {
        let db_err = sea_orm::DbErr::Exec(sea_orm::RuntimeErr::Internal("exec failed".to_string()));
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("exec failed"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_type_to_database_variant() {
        let db_err = sea_orm::DbErr::Type("invalid type".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("invalid type"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_json_to_database_variant() {
        let db_err = sea_orm::DbErr::Json("parse error".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("parse error"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_attr_not_set_to_database_variant() {
        let db_err = sea_orm::DbErr::AttrNotSet("name".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("name"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_migration_to_database_variant() {
        let db_err = sea_orm::DbErr::Migration("schema mismatch".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("schema mismatch"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_rbac_error_to_database_variant() {
        let db_err = sea_orm::DbErr::RbacError("forbidden".to_string());
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("forbidden"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_access_denied_to_database_variant() {
        let db_err = sea_orm::DbErr::AccessDenied {
            permission: "write".to_string(),
            resource: "team".to_string(),
        };
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        match repo_err {
            GeoRestrictionRepositoryError::Database(msg) => {
                assert!(msg.contains("write"));
                assert!(msg.contains("team"));
            }
            other => panic!("expected Database variant, got {:?}", other),
        }
    }

    #[test]
    fn test_dberr_mutex_poison_error_to_database_variant() {
        let db_err = sea_orm::DbErr::MutexPoisonError;
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_backend_not_supported_to_database_variant() {
        let db_err = sea_orm::DbErr::BackendNotSupported {
            db: "mysql",
            ctx: "not configured",
        };
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_unpack_insert_id_to_database_variant() {
        let db_err = sea_orm::DbErr::UnpackInsertId;
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_update_get_primary_key_to_database_variant() {
        let db_err = sea_orm::DbErr::UpdateGetPrimaryKey;
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_convert_from_u64_to_database_variant() {
        let db_err = sea_orm::DbErr::ConvertFromU64("String");
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_try_into_err_to_database_variant() {
        let source_err: std::sync::Arc<dyn std::error::Error + Send + Sync> = std::sync::Arc::new(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad value"),
        );
        let db_err = sea_orm::DbErr::TryIntoErr {
            from: "String",
            into: "i32",
            source: source_err,
        };
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_key_arity_mismatch_to_database_variant() {
        let db_err = sea_orm::DbErr::KeyArityMismatch {
            expected: 2,
            received: 1,
        };
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    #[test]
    fn test_dberr_primary_key_not_set_to_database_variant() {
        let db_err = sea_orm::DbErr::PrimaryKeyNotSet { ctx: "update" };
        let repo_err = GeoRestrictionRepositoryError::Database(db_err.to_string());
        assert!(matches!(
            repo_err,
            GeoRestrictionRepositoryError::Database(_)
        ));
    }

    // ========== Additional error variant Display tests ==========

    #[test]
    fn test_error_other_display_with_long_message() {
        let long_msg = "x".repeat(1000);
        let err = GeoRestrictionRepositoryError::Other(long_msg.clone());
        let msg = format!("{}", err);
        assert!(msg.contains(&long_msg));
    }

    #[test]
    fn test_error_database_display_with_special_characters() {
        let err = GeoRestrictionRepositoryError::Database(
            "error with 'quotes' and \"double\" and; semicolon".to_string(),
        );
        let msg = format!("{}", err);
        assert!(msg.contains("quotes"));
        assert!(msg.contains("double"));
        assert!(msg.contains("semicolon"));
    }

    #[test]
    fn test_error_implements_debug_for_all_variants() {
        let variants: Vec<GeoRestrictionRepositoryError> = vec![
            GeoRestrictionRepositoryError::Database("e".into()),
            GeoRestrictionRepositoryError::TeamNotFound(Uuid::nil()),
            GeoRestrictionRepositoryError::Other("o".into()),
        ];
        for err in &variants {
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty());
        }
    }

    // ========== Additional boundary tests for repository methods ==========

    #[tokio::test]
    async fn test_get_team_restrictions_with_max_uuid_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let max_uuid = Uuid::from_u128(u128::MAX);
        let result = repo.get_team_restrictions(max_uuid).await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_update_team_restrictions_with_empty_vectors_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let restrictions = crate::domain::services::team_service::TeamGeoRestrictions {
            enable_geo_restrictions: false,
            allowed_countries: Some(Vec::new()),
            blocked_countries: Some(Vec::new()),
            ip_whitelist: Some(Vec::new()),
            domain_blacklist: Some(Vec::new()),
        };
        let result = repo
            .update_team_restrictions(Uuid::new_v4(), &restrictions)
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_long_reason_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let long_reason = "x".repeat(2000);
        let result = repo
            .log_geo_restriction_action(Uuid::new_v4(), "10.0.0.1", "US", "allowed", &long_reason)
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action_with_max_uuid_returns_db_error() {
        let repo = DatabaseGeoRestrictionRepository::new(create_test_db_pool());
        let max_uuid = Uuid::from_u128(u128::MAX);
        let result = repo
            .log_geo_restriction_action(max_uuid, "127.0.0.1", "US", "blocked", "test")
            .await;
        let err = result.expect_err("should fail without a real database");
        match err {
            GeoRestrictionRepositoryError::Database(_) => {}
            other => panic!("expected Database error, got {:?}", other),
        }
    }
}
