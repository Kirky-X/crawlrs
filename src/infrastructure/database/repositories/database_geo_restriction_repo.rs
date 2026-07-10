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

#[cfg(test)]
#[cfg(feature = "dbnexus-sqlite")]
mod tests {
    use super::*;
    use crate::infrastructure::database::entities::team;
    use dbnexus::{DbConfig, DbPool};
    use sea_orm::{ActiveModelTrait, ConnectionTrait, Set, Statement};

    async fn setup_db() -> Arc<DbPool> {
        // SQLite in-memory 必须共享同一连接，否则每个连接有独立的数据库
        let config = DbConfig {
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            min_connections: 0,
            ..Default::default()
        };
        let pool = DbPool::with_config(config)
            .await
            .expect("Failed to create DbPool");
        let pool = Arc::new(pool);
        let session = pool
            .get_session("admin")
            .await
            .expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");

        let create_teams_sql = r#"
            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                allowed_countries TEXT,
                blocked_countries TEXT,
                ip_whitelist TEXT,
                domain_blacklist TEXT,
                enable_geo_restrictions INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#;
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            create_teams_sql.to_string(),
        ))
        .await
        .expect("Failed to create teams table");

        let create_logs_sql = r#"
            CREATE TABLE IF NOT EXISTS geo_restriction_logs (
                id TEXT PRIMARY KEY,
                team_id TEXT NOT NULL,
                ip_address TEXT NOT NULL,
                country_code TEXT,
                restriction_type TEXT NOT NULL,
                url TEXT,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
        "#;
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            create_logs_sql.to_string(),
        ))
        .await
        .expect("Failed to create geo_restriction_logs table");

        drop(session);
        pool
    }

    async fn create_team(db: &DbPool) -> Uuid {
        let team_id = Uuid::new_v4();
        let session = db
            .get_session("admin")
            .await
            .expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");
        let team = team::ActiveModel {
            id: Set(team_id),
            name: Set("Test Team".to_string()),
            enable_geo_restrictions: Set(false),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
            ..Default::default()
        };
        team.insert(conn).await.expect("Failed to insert team");
        team_id
    }

    #[tokio::test]
    async fn test_database_geo_restriction_repository() {
        let db = setup_db().await;
        let repo = DatabaseGeoRestrictionRepository::new(db.clone());
        let team_id = create_team(&db).await;

        // Test getting default configuration
        let restrictions = repo.get_team_restrictions(team_id).await.unwrap();
        assert!(!restrictions.enable_geo_restrictions);

        // Test updating configuration
        let mut new_restrictions = restrictions.clone();
        new_restrictions.enable_geo_restrictions = true;
        new_restrictions.allowed_countries = Some(vec!["US".to_string(), "CA".to_string()]);

        repo.update_team_restrictions(team_id, &new_restrictions)
            .await
            .unwrap();

        // Verify update
        let updated_restrictions = repo.get_team_restrictions(team_id).await.unwrap();
        assert!(updated_restrictions.enable_geo_restrictions);
        assert_eq!(updated_restrictions.allowed_countries.unwrap().len(), 2);

        // Test log_geo_restriction_action
        let result = repo
            .log_geo_restriction_action(team_id, "127.0.0.1", "US", "blocked", "Country blocked")
            .await;
        assert!(result.is_ok());
    }
}
