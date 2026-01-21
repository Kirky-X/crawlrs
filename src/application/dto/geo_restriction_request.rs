// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Team geo restriction request and response DTOs

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// 更新团队地理限制配置的请求 DTO
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct UpdateTeamGeoRestrictionsRequest {
    /// 是否启用地理限制
    pub enable_geo_restrictions: bool,
    /// 允许的国家代码列表 (ISO 3166-1 alpha-2)
    #[validate(length(min = 1, message = "国家代码列表不能为空"))]
    pub allowed_countries: Option<Vec<String>>,
    /// 阻止的国家代码列表 (ISO 3166-1 alpha-2)
    #[validate(length(min = 1, message = "国家代码列表不能为空"))]
    pub blocked_countries: Option<Vec<String>>,
    /// IP 白名单列表 (支持 CIDR 表示法)
    pub ip_whitelist: Option<Vec<String>>,
    /// 域名黑名单列表
    pub domain_blacklist: Option<Vec<String>>,
}

/// 团队地理限制配置的响应 DTO
#[derive(Debug, Clone, Serialize)]
pub struct TeamGeoRestrictionsResponse {
    /// 团队 ID
    pub team_id: Uuid,
    /// 是否启用地理限制
    pub enable_geo_restrictions: bool,
    /// 允许的国家代码列表
    pub allowed_countries: Option<Vec<String>>,
    /// 阻止的国家代码列表
    pub blocked_countries: Option<Vec<String>>,
    /// IP 白名单列表
    pub ip_whitelist: Option<Vec<String>>,
    /// 域名黑名单列表
    pub domain_blacklist: Option<Vec<String>>,
}
