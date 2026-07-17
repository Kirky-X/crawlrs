// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
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
            restrictions: Arc::new(RwLock::new(HashMap::with_capacity(64))),
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

        log::info!(
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

        log::info!(
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

        assert!(retrieved_restrictions.enable_geo_restrictions);
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
        let logs = repo.audit_logs.read().await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].team_id, team_id);
        assert_eq!(logs[0].ip_address, "192.168.1.100");
        assert_eq!(logs[0].country_code, "US");
        assert_eq!(logs[0].action, "allowed");
        assert_eq!(logs[0].reason, "IP in whitelist");
    }

    // ========== Default trait ==========

    #[tokio::test]
    async fn test_default_creates_empty_repository() {
        let repo = InMemoryGeoRestrictionRepository::default();
        let restrictions = repo.restrictions.read().await;
        assert!(
            restrictions.is_empty(),
            "default repo should have no seed data"
        );
        let logs = repo.audit_logs.read().await;
        assert!(logs.is_empty(), "default repo should have no audit logs");
    }

    #[test]
    fn test_new_and_default_are_equivalent() {
        let new_repo = InMemoryGeoRestrictionRepository::new();
        let default_repo = InMemoryGeoRestrictionRepository::default();
        // Both should start empty (synchronous check via try_read would block;
        // use blocking_read since these are fresh RwLocks with no writers)
        let new_restrictions = new_repo.restrictions.blocking_read();
        let default_restrictions = default_repo.restrictions.blocking_read();
        assert_eq!(new_restrictions.len(), default_restrictions.len());
    }

    // ========== seed_test_data ==========

    #[tokio::test]
    async fn test_seed_test_data_populates_two_teams() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;
        let restrictions = repo.restrictions.read().await;
        assert_eq!(
            restrictions.len(),
            2,
            "seed_test_data should insert exactly 2 teams"
        );
    }

    #[tokio::test]
    async fn test_seed_test_data_teams_have_geo_restrictions_enabled() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;
        let restrictions = repo.restrictions.read().await;
        // Both seeded teams should have geo restrictions enabled
        for (_, team_geo) in restrictions.iter() {
            assert!(
                team_geo.enable_geo_restrictions,
                "seeded team should have geo restrictions enabled"
            );
        }
    }

    #[tokio::test]
    async fn test_seed_test_data_first_team_has_full_config() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;
        let restrictions = repo.restrictions.read().await;
        // Exactly one team should have both allowed and blocked countries
        let teams_with_both = restrictions
            .iter()
            .filter(|(_, g)| g.allowed_countries.is_some() && g.blocked_countries.is_some())
            .count();
        assert_eq!(
            teams_with_both, 1,
            "one seeded team should have full config"
        );

        // That team should also have ip_whitelist and domain_blacklist
        let full_team = restrictions
            .iter()
            .find(|(_, g)| g.allowed_countries.is_some() && g.blocked_countries.is_some())
            .map(|(_, g)| g)
            .expect("full config team should exist");
        assert!(full_team.ip_whitelist.is_some());
        assert!(full_team.domain_blacklist.is_some());
    }

    #[tokio::test]
    async fn test_seed_test_data_second_team_is_whitelist_only() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;
        let restrictions = repo.restrictions.read().await;
        // Exactly one team should have ip_whitelist but no country lists
        let whitelist_only = restrictions
            .iter()
            .filter(|(_, g)| {
                g.ip_whitelist.is_some()
                    && g.allowed_countries.is_none()
                    && g.blocked_countries.is_none()
                    && g.domain_blacklist.is_none()
            })
            .count();
        assert_eq!(
            whitelist_only, 1,
            "one seeded team should be whitelist-only"
        );
    }

    #[tokio::test]
    async fn test_seed_test_data_does_not_clear_existing_data() {
        let repo = InMemoryGeoRestrictionRepository::new();
        // Insert a team before seeding
        let pre_team_id = Uuid::new_v4();
        repo.update_team_restrictions(
            pre_team_id,
            &TeamGeoRestrictions {
                enable_geo_restrictions: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        repo.seed_test_data().await;

        let restrictions = repo.restrictions.read().await;
        // Pre-existing team should still be there + 2 seeded = 3 total
        assert_eq!(
            restrictions.len(),
            3,
            "seed should add to, not replace, existing data"
        );
        assert!(restrictions.contains_key(&pre_team_id));
    }

    #[tokio::test]
    async fn test_seed_test_data_can_be_called_multiple_times() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;
        repo.seed_test_data().await;
        let restrictions = repo.restrictions.read().await;
        // Each call adds 2 unique teams (random Uuids), so 2 calls = 4 teams
        assert_eq!(
            restrictions.len(),
            4,
            "seed_test_data should be idempotent-safe (additive)"
        );
    }

    // ========== audit log accumulation ==========

    #[tokio::test]
    async fn test_multiple_log_actions_accumulate() {
        let repo = InMemoryGeoRestrictionRepository::new();
        let team_id = Uuid::new_v4();

        repo.log_geo_restriction_action(team_id, "1.1.1.1", "US", "allowed", "reason 1")
            .await
            .unwrap();
        repo.log_geo_restriction_action(team_id, "2.2.2.2", "CN", "denied", "reason 2")
            .await
            .unwrap();
        repo.log_geo_restriction_action(Uuid::new_v4(), "3.3.3.3", "RU", "denied", "reason 3")
            .await
            .unwrap();

        let logs = repo.audit_logs.read().await;
        assert_eq!(logs.len(), 3, "all log actions should accumulate");
        assert_eq!(logs[0].action, "allowed");
        assert_eq!(logs[1].action, "denied");
        assert_eq!(logs[2].action, "denied");
    }

    #[tokio::test]
    async fn test_get_team_restrictions_for_seeded_team_returns_seeded_data() {
        let repo = InMemoryGeoRestrictionRepository::new();
        repo.seed_test_data().await;

        // Pick any seeded team and verify get_team_restrictions returns the
        // seeded config (not the default).
        let restrictions = repo.restrictions.read().await;
        let seeded_id = *restrictions
            .keys()
            .next()
            .expect("should have seeded teams");
        drop(restrictions);

        let fetched = repo.get_team_restrictions(seeded_id).await.unwrap();
        assert!(
            fetched.enable_geo_restrictions,
            "seeded team should return enabled geo restrictions"
        );
    }
}
