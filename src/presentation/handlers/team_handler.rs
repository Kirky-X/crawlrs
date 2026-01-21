// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::geo_restriction_request::{
    TeamGeoRestrictionsResponse, UpdateTeamGeoRestrictionsRequest,
};
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::services::team_service::TeamGeoRestrictions;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{extract::Extension, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;
use tracing::error;

/// 获取团队地理限制配置
pub async fn get_team_geo_restrictions<GR>(
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    let team_id = auth_state.team_id;
    match geo_restriction_repo.get_team_restrictions(team_id).await {
        Ok(restrictions) => {
            let response = TeamGeoRestrictionsResponse {
                team_id,
                enable_geo_restrictions: restrictions.enable_geo_restrictions,
                allowed_countries: restrictions.allowed_countries,
                blocked_countries: restrictions.blocked_countries,
                ip_whitelist: restrictions.ip_whitelist,
                domain_blacklist: restrictions.domain_blacklist,
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "data": response
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to get team geo restrictions: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to get team geo restrictions"
                })),
            )
                .into_response()
        }
    }
}

/// 更新团队地理限制配置
pub async fn update_team_geo_restrictions<GR>(
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(auth_state): Extension<AuthState>,
    Json(request): Json<UpdateTeamGeoRestrictionsRequest>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    let team_id = auth_state.team_id;
    // 验证请求数据
    if let Some(ref countries) = request.allowed_countries {
        for country in countries {
            if country.len() != 2 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": "Country codes must be 2-letter ISO 3166-1 alpha-2 format"
                    })),
                )
                    .into_response();
            }
        }
    }

    if let Some(ref countries) = request.blocked_countries {
        for country in countries {
            if country.len() != 2 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": "Country codes must be 2-letter ISO 3166-1 alpha-2 format"
                    })),
                )
                    .into_response();
            }
        }
    }

    // 验证IP白名单格式
    if let Some(ref whitelist) = request.ip_whitelist {
        for ip in whitelist {
            if !is_valid_ip_or_cidr(ip) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": format!("Invalid IP address or CIDR notation: {}", ip)
                    })),
                )
                    .into_response();
            }
        }
    }

    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: request.enable_geo_restrictions,
        allowed_countries: request.allowed_countries,
        blocked_countries: request.blocked_countries,
        ip_whitelist: request.ip_whitelist,
        domain_blacklist: request.domain_blacklist,
    };

    match geo_restriction_repo
        .update_team_restrictions(team_id, &restrictions)
        .await
    {
        Ok(_) => {
            let response = TeamGeoRestrictionsResponse {
                team_id,
                enable_geo_restrictions: restrictions.enable_geo_restrictions,
                allowed_countries: restrictions.allowed_countries,
                blocked_countries: restrictions.blocked_countries,
                ip_whitelist: restrictions.ip_whitelist,
                domain_blacklist: restrictions.domain_blacklist,
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "data": response
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to update team geo restrictions: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to update team geo restrictions"
                })),
            )
                .into_response()
        }
    }
}

/// 验证IP地址或CIDR表示法格式
fn is_valid_ip_or_cidr(input: &str) -> bool {
    // 检查是否是有效的IP地址
    if input.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }

    // 检查是否是有效的CIDR表示法
    if let Some((ip_part, prefix_part)) = input.split_once('/') {
        if let Ok(ip) = ip_part.parse::<std::net::IpAddr>() {
            if let Ok(prefix) = prefix_part.parse::<u8>() {
                let max_prefix = match ip {
                    std::net::IpAddr::V4(_) => 32,
                    std::net::IpAddr::V6(_) => 128,
                };
                return prefix <= max_prefix;
            }
        }
    }

    false
}
