// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::{
    GeoRestrictionRepository, GeoRestrictionRepositoryError,
};
use crate::domain::services::team_service::TeamGeoRestrictions;
use crate::infrastructure::database::entities::{geo_restriction_log, team};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use uuid::Uuid;

/// 基于数据库的地理限制仓库实现
pub struct DatabaseGeoRestrictionRepository {
    db: DatabaseConnection,
}

impl DatabaseGeoRestrictionRepository {
    /// 创建新的数据库地理限制仓库实例
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl GeoRestrictionRepository for DatabaseGeoRestrictionRepository {
    /// 获取团队的地理限制配置
    async fn get_team_restrictions(
        &self,
        team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        // 查询团队记录
        let team_model = team::Entity::find_by_id(team_id)
            .one(&self.db)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?
            .ok_or_else(|| GeoRestrictionRepositoryError::TeamNotFound(team_id))?;

        // 解析 JSON 字段
        let allowed_countries = team_model
            .allowed_countries
            .map(|json| serde_json::from_value(json).ok())
            .flatten();

        let blocked_countries = team_model
            .blocked_countries
            .map(|json| serde_json::from_value(json).ok())
            .flatten();

        let ip_whitelist = team_model
            .ip_whitelist
            .map(|json| serde_json::from_value(json).ok())
            .flatten();

        Ok(TeamGeoRestrictions {
            enable_geo_restrictions: team_model.enable_geo_restrictions,
            allowed_countries,
            blocked_countries,
            ip_whitelist,
        })
    }

    /// 更新团队的地理限制配置
    async fn update_team_restrictions(
        &self,
        team_id: Uuid,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        // 查询团队记录
        let team_model = team::Entity::find_by_id(team_id)
            .one(&self.db)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?
            .ok_or_else(|| GeoRestrictionRepositoryError::TeamNotFound(team_id))?;

        // 转换为 ActiveModel 进行更新
        let mut active_model: team::ActiveModel = team_model.into();

        // 设置地理限制字段
        active_model.enable_geo_restrictions = Set(restrictions.enable_geo_restrictions);
        active_model.allowed_countries = Set(restrictions
            .allowed_countries
            .as_ref()
            .map(|countries| serde_json::to_value(countries).unwrap()));
        active_model.blocked_countries = Set(restrictions
            .blocked_countries
            .as_ref()
            .map(|countries| serde_json::to_value(countries).unwrap()));
        active_model.ip_whitelist = Set(restrictions
            .ip_whitelist
            .as_ref()
            .map(|whitelist| serde_json::to_value(whitelist).unwrap()));

        // 更新记录
        active_model
            .update(&self.db)
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
            .insert(&self.db)
            .await
            .map_err(|e| GeoRestrictionRepositoryError::Database(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::database::entities::team;
    use sea_orm::{ActiveModelTrait, Set};

    /// 创建测试团队
    async fn create_test_team(
        db: &DatabaseConnection,
        team_id: Uuid,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let team_model = team::ActiveModel {
            id: Set(team_id),
            name: Set("Test Team".to_string()),
            allowed_countries: Set(None),
            blocked_countries: Set(None),
            ip_whitelist: Set(None),
            enable_geo_restrictions: Set(false),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        team_model.insert(db).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_database_geo_restriction_repository() {
        // 这个测试需要数据库连接，这里只是展示结构
        // 实际测试应该在集成测试环境中运行

        // 创建内存数据库连接
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to database");

        // 这里应该运行数据库迁移来创建表结构
        // 由于迁移逻辑复杂，这里省略具体实现

        // 创建测试团队
        let team_id = Uuid::new_v4();
        create_test_team(&db, team_id)
            .await
            .expect("Failed to create test team");

        let repo = DatabaseGeoRestrictionRepository::new(db);

        // 测试获取默认配置
        let restrictions = repo.get_team_restrictions(team_id).await.unwrap();
        assert!(!restrictions.enable_geo_restrictions);
        assert!(restrictions.allowed_countries.is_none());
        assert!(restrictions.blocked_countries.is_none());
        assert!(restrictions.ip_whitelist.is_none());

        // 测试更新配置
        let new_restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "GB".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
        };

        repo.update_team_restrictions(team_id, &new_restrictions)
            .await
            .unwrap();

        // 验证更新结果
        let updated_restrictions = repo.get_team_restrictions(team_id).await.unwrap();
        assert!(updated_restrictions.enable_geo_restrictions);
        assert_eq!(
            updated_restrictions.allowed_countries,
            Some(vec!["US".to_string(), "GB".to_string()])
        );
        assert_eq!(
            updated_restrictions.blocked_countries,
            Some(vec!["CN".to_string()])
        );
        assert_eq!(
            updated_restrictions.ip_whitelist,
            Some(vec!["192.168.1.0/24".to_string()])
        );

        // 测试记录日志
        repo.log_geo_restriction_action(
            team_id,
            "192.168.1.100",
            "US",
            "allowed",
            "IP in whitelist",
        )
        .await
        .unwrap();
    }
}
