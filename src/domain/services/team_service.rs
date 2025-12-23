// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::infrastructure::geolocation::{is_ip_in_cidr, GeoLocationService};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// 团队地理限制配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamGeoRestrictions {
    /// 是否启用地理限制
    pub enable_geo_restrictions: bool,
    /// 允许的国家代码列表 (ISO 3166-1 alpha-2)
    pub allowed_countries: Option<Vec<String>>,
    /// 阻止的国家代码列表 (ISO 3166-1 alpha-2)
    pub blocked_countries: Option<Vec<String>>,
    /// IP 白名单列表 (支持 CIDR 表示法)
    pub ip_whitelist: Option<Vec<String>>,
    /// 域名黑名单列表
    pub domain_blacklist: Option<Vec<String>>,
}

/// 地理限制验证结果
#[derive(Debug, Clone, PartialEq)]
pub enum GeoRestrictionResult {
    /// 允许访问
    Allowed,
    /// 因地理限制被拒绝
    Denied(String),
}

/// 团队服务
///
/// 处理团队相关的业务逻辑，包括地理限制验证
pub struct TeamService {
    geolocation_service: GeoLocationService,
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
}

impl TeamService {
    /// 创建新的团队服务实例
    pub fn new(
        geolocation_service: GeoLocationService,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    ) -> Self {
        Self {
            geolocation_service,
            geo_restriction_repo,
        }
    }

    /// 验证 IP 地址是否符合团队的地理限制
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    /// * `ip_address` - 要验证的 IP 地址
    /// * `restrictions` - 团队的地理限制配置
    ///
    /// # 返回值
    ///
    /// * `Ok(GeoRestrictionResult)` - 验证结果
    /// * `Err(anyhow::Error)` - 验证过程中出现的错误
    pub async fn validate_geographic_restriction(
        &self,
        team_id: Uuid,
        ip_address: &str,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<GeoRestrictionResult> {
        // 如果未启用地理限制，直接允许
        if !restrictions.enable_geo_restrictions {
            return Ok(GeoRestrictionResult::Allowed);
        }

        // 解析 IP 地址
        let ip = match IpAddr::from_str(ip_address) {
            Ok(ip) => ip,
            Err(_) => {
                return Ok(GeoRestrictionResult::Denied(
                    "Invalid IP address format".to_string(),
                ))
            }
        };

        // 首先检查 IP 白名单
        if let Some(ref whitelist) = restrictions.ip_whitelist {
            for cidr in whitelist {
                if is_ip_in_cidr(&ip, cidr) {
                    tracing::info!(
                        "IP {} allowed by whitelist (CIDR: {}) for team {}",
                        ip_address,
                        cidr,
                        team_id
                    );
                    return Ok(GeoRestrictionResult::Allowed);
                }
            }
        }

        // 检查允许的国家列表
        if let Some(ref allowed) = restrictions.allowed_countries {
            // 获取 IP 的地理位置信息
            let location = match self.geolocation_service.get_location(&ip).await {
                Ok(location) => location,
                Err(e) => {
                    tracing::error!(
                        "Failed to get geolocation for IP {} for team {}: {}",
                        ip_address,
                        team_id,
                        e
                    );
                    return Ok(GeoRestrictionResult::Denied(
                        "Unable to determine geographic location".to_string(),
                    ));
                }
            };
            let country_code = location.country_code.to_uppercase();

            if !allowed
                .iter()
                .any(|code| code.to_uppercase() == country_code)
            {
                tracing::warn!(
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
            
            // 如果在允许列表中，则继续检查其他限制（如黑名单）
            // 注意：通常白名单优先于黑名单，但这里的 allowed_countries 是 "仅允许这些国家"，所以如果在列表中，还需要检查是否在黑名单中
        }

        // 获取 IP 的地理位置信息 (如果尚未获取)
        // 优化：如果已经获取过，避免重复调用
        // 但由于 Rust 的所有权机制，这里简单起见重新获取或重构代码
        // 为了简化，我们假设 geolocation_service 有缓存
        let location = match self.geolocation_service.get_location(&ip).await {
            Ok(location) => location,
            Err(e) => {
                tracing::error!(
                    "Failed to get geolocation for IP {} for team {}: {}",
                    ip_address,
                    team_id,
                    e
                );
                return Ok(GeoRestrictionResult::Denied(
                    "Unable to determine geographic location".to_string(),
                ));
            }
        };
        let country_code = location.country_code.to_uppercase();

        // 检查阻止的国家列表
        if let Some(ref blocked) = restrictions.blocked_countries {
            if blocked
                .iter()
                .any(|code| code.to_uppercase() == country_code)
            {
                tracing::warn!(
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

        tracing::info!(
            "IP {} from country {} allowed for team {}",
            ip_address,
            country_code,
            team_id
        );

        Ok(GeoRestrictionResult::Allowed)
    }

    /// 验证域名是否在黑名单中
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

    /// 从数据库获取团队的地理限制配置
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    ///
    /// # 返回值
    ///
    /// * `TeamGeoRestrictions` - 团队的地理限制配置
    pub async fn get_team_geo_restrictions(&self, team_id: Uuid) -> TeamGeoRestrictions {
        match self
            .geo_restriction_repo
            .get_team_restrictions(team_id)
            .await
        {
            Ok(restrictions) => {
                tracing::debug!(
                    "Retrieved geo restrictions for team {}: {:?}",
                    team_id,
                    restrictions
                );
                restrictions
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to get geo restrictions for team {}: {}. Using default configuration.",
                    team_id,
                    e
                );
                TeamGeoRestrictions::default()
            }
        }
    }
}

#[cfg(test)]
#[path = "team_service_test.rs"]
mod tests;
