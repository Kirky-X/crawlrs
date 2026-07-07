// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::geo_restriction_request::{
    TeamGeoRestrictionsResponse, UpdateTeamGeoRestrictionsRequest,
};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::team_service::TeamGeoRestrictions;
use crate::presentation::handlers::response_builder::{
    error_codes, error_response_with_code, ApiResponse,
};
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{extract::Extension, http::StatusCode, response::IntoResponse, Json};
use std::sync::Arc;
use log::error;
use uuid::Uuid;

/// 团队信息响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamInfoResponse {
    pub id: Uuid,
    pub name: String,
    pub credits_balance: i64,
    pub total_tasks: i64,
    pub completed_tasks: i64,
    pub failed_tasks: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 获取团队信息
pub async fn get_team_info(
    Extension(credits_repo): Extension<Arc<dyn CreditsRepository>>,
    Extension(task_repo): Extension<Arc<dyn TaskRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;

    // 获取积分余额
    let credits_balance = credits_repo.get_balance(team_id).await.unwrap_or(0);

    // 获取任务统计
    let total_tasks = task_repo
        .find_by_crawl_id(team_id)
        .await
        .unwrap_or_default()
        .len() as i64;

    let response = TeamInfoResponse {
        id: team_id,
        name: "Team".to_string(),
        credits_balance,
        total_tasks,
        completed_tasks: 0,
        failed_tasks: 0,
        created_at: chrono::Utc::now(),
    };

    Json(ApiResponse::success(response)).into_response()
}

/// 团队使用统计响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamUsageResponse {
    pub team_id: Uuid,
    pub period: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub credits_used: i64,
    pub avg_response_time_ms: f64,
}

/// 获取团队使用统计
pub async fn get_team_usage(
    Extension(credits_repo): Extension<Arc<dyn CreditsRepository>>,
    Extension(scrape_result_repo): Extension<Arc<dyn ScrapeResultRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;

    // 获取积分余额
    let credits_balance = credits_repo.get_balance(team_id).await.unwrap_or(0);

    // 获取团队平均响应时间
    let avg_response_time_ms: f64 = scrape_result_repo
        .get_team_avg_response_time(team_id)
        .await
        .unwrap_or(0.0);

    let response = TeamUsageResponse {
        team_id,
        period: "30d".to_string(),
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        credits_used: credits_balance.abs(), // 已使用的积分（负数表示已消耗）
        avg_response_time_ms,
    };

    Json(ApiResponse::success(response)).into_response()
}

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

            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Err(e) => {
            error!("Failed to get team geo restrictions: {:?}", e);
            error_response_with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                error_codes::INTERNAL_ERROR,
                "Failed to get team geo restrictions",
            )
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
                return error_response_with_code(
                    StatusCode::BAD_REQUEST,
                    error_codes::VALIDATION_ERROR,
                    "Country codes must be 2-letter ISO 3166-1 alpha-2 format",
                );
            }
        }
    }

    if let Some(ref countries) = request.blocked_countries {
        for country in countries {
            if country.len() != 2 {
                return error_response_with_code(
                    StatusCode::BAD_REQUEST,
                    error_codes::VALIDATION_ERROR,
                    "Country codes must be 2-letter ISO 3166-1 alpha-2 format",
                );
            }
        }
    }

    // 验证IP白名单格式
    if let Some(ref whitelist) = request.ip_whitelist {
        for ip in whitelist {
            if !is_valid_ip_or_cidr(ip) {
                return error_response_with_code(
                    StatusCode::BAD_REQUEST,
                    error_codes::VALIDATION_ERROR,
                    format!("Invalid IP address or CIDR notation: {}", ip),
                );
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

            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Err(e) => {
            error!("Failed to update team geo restrictions: {:?}", e);
            error_response_with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                error_codes::INTERNAL_ERROR,
                "Failed to update team geo restrictions",
            )
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
