// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::services::team_service::TeamGeoRestrictions;
use thiserror::Error;
use uuid::Uuid;

/// 地理限制仓库错误类型
#[derive(Error, Debug)]
pub enum GeoRestrictionRepositoryError {
    /// 数据库错误
    #[error("Database error: {0}")]
    Database(String),

    /// 团队未找到
    #[error("Team not found: {0}")]
    TeamNotFound(Uuid),

    /// 其他错误
    #[error("Other error: {0}")]
    Other(String),
}

/// 地理限制仓库接口
///
/// 定义了团队地理限制配置的持久化操作
#[async_trait::async_trait]
pub trait GeoRestrictionRepository: Send + Sync {
    /// 获取团队的地理限制配置
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    ///
    /// # 返回值
    ///
    /// * `Ok(TeamGeoRestrictions)` - 团队的地理限制配置
    /// * `Err(GeoRestrictionRepositoryError)` - 获取失败时返回错误
    async fn get_team_restrictions(
        &self,
        team_id: Uuid,
    ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError>;

    /// 更新团队的地理限制配置
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    /// * `restrictions` - 新的地理限制配置
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 更新成功
    /// * `Err(GeoRestrictionRepositoryError)` - 更新失败时返回错误
    async fn update_team_restrictions(
        &self,
        team_id: Uuid,
        restrictions: &TeamGeoRestrictions,
    ) -> Result<(), GeoRestrictionRepositoryError>;

    /// 记录地理限制审计日志
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    /// * `ip_address` - IP 地址
    /// * `country_code` - 国家代码
    /// * `action` - 执行的操作 ("allowed" 或 "denied")
    /// * `reason` - 操作原因
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 记录成功
    /// * `Err(GeoRestrictionRepositoryError)` - 记录失败时返回错误
    async fn log_geo_restriction_action(
        &self,
        team_id: Uuid,
        ip_address: &str,
        country_code: &str,
        action: &str,
        reason: &str,
    ) -> Result<(), GeoRestrictionRepositoryError>;
}
