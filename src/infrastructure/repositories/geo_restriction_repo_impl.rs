// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::{
    repositories::geo_restriction_repository::{
        GeoRestrictionRepository, GeoRestrictionRepositoryError,
    },
    services::team_service::TeamGeoRestrictions,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// 内存中的地理限制仓库实现
///
/// 这是一个临时的内存实现，用于开发和测试阶段。
/// 在生产环境中，应该使用基于数据库的实现。
pub struct InMemoryGeoRestrictionRepository {
    /// 存储团队地理限制配置的内存映射
    restrictions: Arc<RwLock<HashMap<Uuid, TeamGeoRestrictions>>>,
    /// 存储地理限制审计日志的内存向量
    audit_logs: Arc<RwLock<Vec<GeoRestrictionAuditLog>>>,
}

/// 地理限制审计日志条目
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct GeoRestrictionAuditLog {
    team_id: Uuid,
    ip_address: String,
    country_code: String,
    action: String,
    reason: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl InMemoryGeoRestrictionRepository {
    /// 创建新的内存地理限制仓库实例
    pub fn new() -> Self {
        Self {
            restrictions: Arc::new(RwLock::new(HashMap::new())),
            audit_logs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 为测试添加示例数据
    pub async fn seed_test_data(&self) {
        let mut restrictions = self.restrictions.write().await;

        // 添加一个启用地理限制的测试团队
        let test_team_id = Uuid::new_v4();
        restrictions.insert(
            test_team_id,
            TeamGeoRestrictions {
                enable_geo_restrictions: true,
                allowed_countries: Some(vec!["US".to_string(), "GB".to_string(), "CA".to_string()]),
                blocked_countries: Some(vec!["CN".to_string(), "RU".to_string()]),
                ip_whitelist: Some(vec!["192.168.1.0/24".to_string(), "10.0.0.1".to_string()]),
                domain_blacklist: Some(vec!["example.com".to_string()]),
            },
        );

        // 添加一个只启用 IP 白名单的测试团队
        let whitelist_only_team_id = Uuid::new_v4();
        restrictions.insert(
            whitelist_only_team_id,
            TeamGeoRestrictions {
                enable_geo_restrictions: true,
                allowed_countries: None,
                blocked_countries: None,
                ip_whitelist: Some(vec!["127.0.0.1".to_string(), "::1".to_string()]),
                domain_blacklist: None,
            },
        );
    }
}

impl Default for InMemoryGeoRestrictionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GeoRestrictionRepository for InMemoryGeoRestrictionRepository {
    async fn get_team_restrictions(
        &self,
        team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
        let restrictions = self.restrictions.read().await;

        // 如果团队不存在，返回默认配置（不启用地理限制）
        match restrictions.get(&team_id) {
            Some(restriction) => Ok(restriction.clone()),
            None => Ok(TeamGeoRestrictions::default()),
        }
    }

    async fn update_team_restrictions(
        &self,
        team_id: Uuid,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        let mut restrictions_map = self.restrictions.write().await;
        restrictions_map.insert(team_id, restrictions.clone());

        tracing::info!(
            "Updated geographic restrictions for team {}: {:?}",
            team_id,
            restrictions
        );

        Ok(())
    }

    async fn log_geo_restriction_action(
        &self,
        team_id: Uuid,
        ip_address: &str,
        country_code: &str,
        action: &str,
        reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError> {
        let mut audit_logs = self.audit_logs.write().await;

        let log_entry = GeoRestrictionAuditLog {
            team_id,
            ip_address: ip_address.to_string(),
            country_code: country_code.to_string(),
            action: action.to_string(),
            reason: reason.to_string(),
            timestamp: chrono::Utc::now(),
        };

        audit_logs.push(log_entry);

        tracing::info!(
            "Logged geo restriction action for team {}: IP {} from {} was {} (reason: {})",
            team_id,
            ip_address,
            country_code,
            action,
            reason
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_team_restrictions_default() {
        let repo = InMemoryGeoRestrictionRepository::new();
        let team_id = Uuid::new_v4();

        let restrictions = repo.get_team_restrictions(team_id).await.unwrap();

        assert!(!restrictions.enable_geo_restrictions);
        assert!(restrictions.allowed_countries.is_none());
        assert!(restrictions.blocked_countries.is_none());
        assert!(restrictions.ip_whitelist.is_none());
    }

    #[tokio::test]
    async fn test_update_and_get_team_restrictions() {
        let repo = InMemoryGeoRestrictionRepository::new();
        let team_id = Uuid::new_v4();

        let new_restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "GB".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
            domain_blacklist: Some(vec!["example.com".to_string()]),
        };

        repo.update_team_restrictions(team_id, &new_restrictions)
            .await
            .unwrap();

        let retrieved_restrictions = repo.get_team_restrictions(team_id).await.unwrap();

        assert_eq!(retrieved_restrictions.enable_geo_restrictions, true);
        assert_eq!(
            retrieved_restrictions.allowed_countries,
            Some(vec!["US".to_string(), "GB".to_string()])
        );
        assert_eq!(
            retrieved_restrictions.blocked_countries,
            Some(vec!["CN".to_string()])
        );
        assert_eq!(
            retrieved_restrictions.ip_whitelist,
            Some(vec!["192.168.1.0/24".to_string()])
        );
        assert_eq!(
            retrieved_restrictions.domain_blacklist,
            Some(vec!["example.com".to_string()])
        );
    }

    #[tokio::test]
    async fn test_log_geo_restriction_action() {
        let repo = InMemoryGeoRestrictionRepository::new();
        let team_id = Uuid::new_v4();

        repo.log_geo_restriction_action(
            team_id,
            "192.168.1.100",
            "US",
            "allowed",
            "IP in whitelist",
        )
        .await
        .unwrap();

        // 验证日志记录成功（可以通过后续的数据库查询来验证）
        // 目前主要是确保不抛出错误
    }
}
