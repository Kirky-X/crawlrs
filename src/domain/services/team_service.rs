// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{CreditsTransactionType, Team};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::team_repository::TeamRepository;
use crate::domain::services::geo_location::{is_ip_in_cidr, GeoLocationService};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};
use shaku::{Component, Interface};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamGeoRestrictions {
    pub enable_geo_restrictions: bool,
    pub allowed_countries: Option<Vec<String>>,
    pub blocked_countries: Option<Vec<String>>,
    pub ip_whitelist: Option<Vec<String>>,
    pub domain_blacklist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeoRestrictionResult {
    Allowed,
    Denied(String),
}

/// Trait for TeamService - enables dependency injection
#[async_trait::async_trait]
pub trait TeamServiceTrait: shaku::Interface + Send + Sync {
    async fn validate_geographic_restriction(
        &self,
        team_id: Uuid,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult>;

    fn validate_domain_blacklist(
        &self,
        domain: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult>;

    async fn get_team_geo_restrictions(&self, team_id: Uuid) -> TeamGeoRestrictions;
}

pub struct TeamService {
    geolocation_service: Arc<dyn GeoLocationService>,
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamService {
    pub fn new(
        geolocation_service: Arc<dyn GeoLocationService>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    ) -> Self {
        Self {
            geolocation_service,
            geo_restriction_repo,
        }
    }

    pub async fn validate_geographic_restriction(
        &self,
        team_id: Uuid,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        if !restrictions.enable_geo_restrictions {
            return Ok(GeoRestrictionResult::Allowed);
        }

        let ip = match IpAddr::from_str(ip_address) {
            Ok(ip) => ip,
            Err(_) => {
                return Ok(GeoRestrictionResult::Denied(
                    "Invalid IP address format".to_string(),
                ))
            }
        };

        if let Some(ref whitelist) = restrictions.ip_whitelist {
            for cidr in whitelist {
                if is_ip_in_cidr(&ip, cidr) {
                    log::info!(
                        "IP {} allowed by whitelist (CIDR: {}) for team {}",
                        ip_address,
                        cidr,
                        team_id
                    );
                    return Ok(GeoRestrictionResult::Allowed);
                }
            }
        }

        let country_code = self.get_country_code(team_id, ip_address, &ip).await?;

        if let Some(ref allowed) = restrictions.allowed_countries {
            if !allowed
                .iter()
                .any(|code| code.to_uppercase() == country_code)
            {
                log::warn!(
                    "IP {} from country {} not in allowed list for team {} (allowed countries: {:?})",
                    ip_address,
                    country_code,
                    team_id,
                    allowed
                );
                return Ok(GeoRestrictionResult::Denied(format!(
                    "Access from country {} is not allowed",
                    country_code
                )));
            }
        }

        if let Some(ref blocked) = restrictions.blocked_countries {
            if blocked
                .iter()
                .any(|code| code.to_uppercase() == country_code)
            {
                log::warn!(
                    "IP {} from country {} blocked for team {} (blocked countries: {:?})",
                    ip_address,
                    country_code,
                    team_id,
                    blocked
                );
                return Ok(GeoRestrictionResult::Denied(format!(
                    "Access from country {} is not allowed",
                    country_code
                )));
            }
        }

        log::info!(
            "IP {} from country {} allowed for team {}",
            ip_address,
            country_code,
            team_id
        );

        Ok(GeoRestrictionResult::Allowed)
    }

    async fn get_country_code(
        &self,
        team_id: Uuid,
        ip_address: &str,
        ip: &IpAddr,
    ) -> Result<String> {
        let location = self
            .geolocation_service
            .get_location(ip)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to get geolocation for IP {} for team {}: {}",
                    ip_address,
                    team_id,
                    e
                );
                anyhow::anyhow!("Unable to determine geographic location")
            })?;
        Ok(location.country_code.to_uppercase())
    }

    pub fn validate_domain_blacklist(
        &self,
        domain: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        if !restrictions.enable_geo_restrictions {
            return Ok(GeoRestrictionResult::Allowed);
        }

        if let Some(ref blacklist) = restrictions.domain_blacklist {
            for blocked_domain in blacklist {
                if domain.contains(blocked_domain) {
                    return Ok(GeoRestrictionResult::Denied(format!(
                        "Domain {} is in the blacklist",
                        domain
                    )));
                }
            }
        }

        Ok(GeoRestrictionResult::Allowed)
    }

    pub async fn get_team_geo_restrictions(&self, team_id: Uuid) -> TeamGeoRestrictions {
        match self
            .geo_restriction_repo
            .get_team_restrictions(team_id)
            .await
        {
            Ok(restrictions) => {
                log::debug!(
                    "Retrieved geo restrictions for team {}: {:?}",
                    team_id,
                    restrictions
                );
                restrictions
            }
            Err(e) => {
                log::warn!(
                    "Failed to get geo restrictions for team {}: {}. Using default configuration.",
                    team_id,
                    e
                );
                TeamGeoRestrictions::default()
            }
        }
    }
}

#[async_trait::async_trait]
impl TeamServiceTrait for TeamService {
    async fn validate_geographic_restriction(
        &self,
        team_id: Uuid,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        self.validate_geographic_restriction(team_id, ip_address, restrictions)
            .await
    }

    fn validate_domain_blacklist(
        &self,
        domain: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        self.validate_domain_blacklist(domain, restrictions)
    }

    async fn get_team_geo_restrictions(&self, team_id: Uuid) -> TeamGeoRestrictions {
        self.get_team_geo_restrictions(team_id).await
    }
}

// === Section: TeamManagementService (扩展接口) ===

/// 团队管理服务接口（扩展）
///
/// 提供团队的创建、查询和积分管理功能。
/// 与 `TeamServiceTrait` 互补——后者专注于地理限制验证，
/// 本接口专注于团队生命周期与积分管理。
#[async_trait]
pub trait TeamManagementService: Interface + Send + Sync {
    /// 创建新团队
    ///
    /// # 参数
    /// * `name` - 团队名称（非空，最多 255 字符）
    ///
    /// # 返回值
    /// * `Ok(Team)` - 创建成功
    /// * `Err` - 名称无效或持久化失败
    async fn create_team(&self, name: String) -> Result<Team>;

    /// 根据 ID 获取团队
    ///
    /// # 参数
    /// * `team_id` - 团队 ID
    ///
    /// # 返回值
    /// * `Ok(Team)` - 找到团队
    /// * `Err` - 团队不存在或查询失败
    async fn get_team(&self, team_id: Uuid) -> Result<Team>;

    /// 更新团队积分
    ///
    /// 正数增加积分，负数扣减积分。扣减时若余额不足将返回错误。
    ///
    /// # 参数
    /// * `team_id` - 团队 ID
    /// * `amount` - 积分变动量（正数增加，负数扣减，零为无操作）
    /// * `description` - 变动描述
    ///
    /// # 返回值
    /// * `Ok(i64)` - 操作后的余额
    /// * `Err` - 余额不足或持久化失败
    async fn update_credits(&self, team_id: Uuid, amount: i64, description: String) -> Result<i64>;

    /// 查询团队积分余额
    ///
    /// # 参数
    /// * `team_id` - 团队 ID
    ///
    /// # 返回值
    /// * `Ok(i64)` - 当前余额
    /// * `Err` - 查询失败
    async fn check_credits(&self, team_id: Uuid) -> Result<i64>;
}

/// 团队管理服务实现
///
/// 通过注入 `TeamRepository` 和 `CreditsRepository` 实现团队与积分管理。
/// DI 注册在 Phase 11 统一处理。
#[derive(Component)]
#[shaku(interface = TeamManagementService)]
pub struct TeamManagementServiceImpl {
    /// 团队仓库
    #[shaku(inject)]
    team_repository: Arc<dyn TeamRepository>,
    /// 积分仓库
    #[shaku(inject)]
    credits_repository: Arc<dyn CreditsRepository>,
}

impl TeamManagementServiceImpl {
    /// 创建新的团队管理服务实现（测试与手动构造用）
    pub fn new(
        team_repository: Arc<dyn TeamRepository>,
        credits_repository: Arc<dyn CreditsRepository>,
    ) -> Self {
        Self {
            team_repository,
            credits_repository,
        }
    }
}

#[async_trait]
impl TeamManagementService for TeamManagementServiceImpl {
    async fn create_team(&self, name: String) -> Result<Team> {
        let team = Team::new(Uuid::new_v4(), name);
        team.validate_name()
            .map_err(|e| anyhow!("Invalid team name: {}", e))?;

        let created = self
            .team_repository
            .create(&team)
            .await
            .map_err(|e| anyhow!("Failed to create team: {}", e))?;

        info!("Created team {} ({})", created.id, created.name);
        Ok(created)
    }

    async fn get_team(&self, team_id: Uuid) -> Result<Team> {
        let team = self
            .team_repository
            .find_by_id(team_id)
            .await
            .map_err(|e| anyhow!("Failed to find team {}: {}", team_id, e))?
            .ok_or_else(|| anyhow!("Team not found: {}", team_id))?;

        Ok(team)
    }

    async fn update_credits(&self, team_id: Uuid, amount: i64, description: String) -> Result<i64> {
        if amount > 0 {
            self.credits_repository
                .add_credits(
                    team_id,
                    amount,
                    CreditsTransactionType::ManualAdjustment,
                    description,
                    None,
                )
                .await
                .map_err(|e| anyhow!("Failed to add credits: {}", e))?;
        } else if amount < 0 {
            self.credits_repository
                .deduct_credits(
                    team_id,
                    -amount,
                    CreditsTransactionType::ManualAdjustment,
                    description,
                    None,
                )
                .await
                .map_err(|e| anyhow!("Failed to deduct credits: {}", e))?;
        }

        let balance = self
            .credits_repository
            .get_balance(team_id)
            .await
            .map_err(|e| anyhow!("Failed to get balance: {}", e))?;

        info!(
            "Updated credits for team {} by {} (new balance: {})",
            team_id, amount, balance
        );
        Ok(balance)
    }

    async fn check_credits(&self, team_id: Uuid) -> Result<i64> {
        let balance = self
            .credits_repository
            .get_balance(team_id)
            .await
            .map_err(|e| anyhow!("Failed to check credits for team {}: {}", team_id, e))?;

        Ok(balance)
    }
}

#[cfg(test)]
#[path = "team_service_test.rs"]
mod tests;
