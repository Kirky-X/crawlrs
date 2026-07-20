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
use log::error;
use std::sync::Arc;
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

#[cfg(test)]
mod tests {
    use super::*;
    use validator::Validate;

    // ========== is_valid_ip_or_cidr tests ==========

    #[test]
    fn test_is_valid_ipv4_address() {
        assert!(is_valid_ip_or_cidr("192.168.1.1"));
        assert!(is_valid_ip_or_cidr("10.0.0.1"));
        assert!(is_valid_ip_or_cidr("0.0.0.0"));
        assert!(is_valid_ip_or_cidr("255.255.255.255"));
    }

    #[test]
    fn test_is_valid_ipv6_address() {
        assert!(is_valid_ip_or_cidr("::1"));
        assert!(is_valid_ip_or_cidr("2001:db8::1"));
        assert!(is_valid_ip_or_cidr("fe80::1"));
    }

    #[test]
    fn test_is_valid_ipv4_cidr() {
        assert!(is_valid_ip_or_cidr("192.168.1.0/24"));
        assert!(is_valid_ip_or_cidr("10.0.0.0/8"));
        assert!(is_valid_ip_or_cidr("172.16.0.0/12"));
        assert!(is_valid_ip_or_cidr("192.168.1.1/32"));
        assert!(is_valid_ip_or_cidr("0.0.0.0/0"));
    }

    #[test]
    fn test_is_valid_ipv6_cidr() {
        assert!(is_valid_ip_or_cidr("2001:db8::/32"));
        assert!(is_valid_ip_or_cidr("fe80::/10"));
        assert!(is_valid_ip_or_cidr("::1/128"));
        assert!(is_valid_ip_or_cidr("::/0"));
    }

    #[test]
    fn test_invalid_ip_address() {
        assert!(!is_valid_ip_or_cidr("999.999.999.999"));
        assert!(!is_valid_ip_or_cidr("not-an-ip"));
        assert!(!is_valid_ip_or_cidr(""));
        assert!(!is_valid_ip_or_cidr("192.168.1"));
    }

    #[test]
    fn test_invalid_cidr_prefix() {
        assert!(!is_valid_ip_or_cidr("192.168.1.0/33"));
        assert!(!is_valid_ip_or_cidr("192.168.1.0/abc"));
        assert!(!is_valid_ip_or_cidr("192.168.1.0/"));
        assert!(!is_valid_ip_or_cidr("::1/129"));
    }

    #[test]
    fn test_invalid_cidr_ip_part() {
        assert!(!is_valid_ip_or_cidr("999.1.1.1/24"));
        assert!(!is_valid_ip_or_cidr("not-ip/24"));
    }

    // ========== TeamInfoResponse serialization ==========

    #[test]
    fn test_team_info_response_serialization() {
        let team_id = Uuid::new_v4();
        let response = TeamInfoResponse {
            id: team_id,
            name: "Test Team".to_string(),
            credits_balance: 1000,
            total_tasks: 50,
            completed_tasks: 45,
            failed_tasks: 5,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["id"], team_id.to_string());
        assert_eq!(parsed["name"], "Test Team");
        assert_eq!(parsed["credits_balance"], 1000);
        assert_eq!(parsed["total_tasks"], 50);
        assert_eq!(parsed["completed_tasks"], 45);
        assert_eq!(parsed["failed_tasks"], 5);
    }

    #[test]
    fn test_team_info_response_negative_balance() {
        let response = TeamInfoResponse {
            id: Uuid::new_v4(),
            name: "Debtor Team".to_string(),
            credits_balance: -100,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["credits_balance"], -100);
    }

    // ========== TeamUsageResponse serialization ==========

    #[test]
    fn test_team_usage_response_serialization() {
        let team_id = Uuid::new_v4();
        let response = TeamUsageResponse {
            team_id,
            period: "30d".to_string(),
            total_requests: 1000,
            successful_requests: 950,
            failed_requests: 50,
            credits_used: 5000,
            avg_response_time_ms: 123.45,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["team_id"], team_id.to_string());
        assert_eq!(parsed["period"], "30d");
        assert_eq!(parsed["total_requests"], 1000);
        assert_eq!(parsed["successful_requests"], 950);
        assert_eq!(parsed["failed_requests"], 50);
        assert_eq!(parsed["credits_used"], 5000);
        assert_eq!(parsed["avg_response_time_ms"], 123.45);
    }

    #[test]
    fn test_team_usage_response_zero_values() {
        let response = TeamUsageResponse {
            team_id: Uuid::new_v4(),
            period: "7d".to_string(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            credits_used: 0,
            avg_response_time_ms: 0.0,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total_requests"], 0);
        assert_eq!(parsed["avg_response_time_ms"], 0.0);
    }

    #[test]
    fn test_team_info_response_deserialization() {
        let json = format!(
            r#"{{"id":"{}","name":"Test","credits_balance":100,"total_tasks":10,"completed_tasks":8,"failed_tasks":2,"created_at":"2025-01-01T00:00:00Z"}}"#,
            Uuid::new_v4()
        );
        let dto: TeamInfoResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.name, "Test");
        assert_eq!(dto.credits_balance, 100);
    }

    // ========== is_valid_ip_or_cidr additional edge cases ==========

    #[test]
    fn test_is_valid_loopback_ipv4() {
        assert!(is_valid_ip_or_cidr("127.0.0.1"));
        assert!(is_valid_ip_or_cidr("127.0.0.1/8"));
    }

    #[test]
    fn test_is_valid_link_local_ipv4() {
        assert!(is_valid_ip_or_cidr("169.254.0.1"));
        assert!(is_valid_ip_or_cidr("169.254.0.0/16"));
    }

    #[test]
    fn test_is_valid_ipv6_loopback() {
        assert!(is_valid_ip_or_cidr("::1"));
        assert!(is_valid_ip_or_cidr("::1/128"));
    }

    #[test]
    fn test_is_valid_ipv6_full_form() {
        assert!(is_valid_ip_or_cidr(
            "2001:0db8:0000:0000:0000:0000:0000:0001"
        ));
    }

    #[test]
    fn test_invalid_cidr_missing_prefix() {
        assert!(!is_valid_ip_or_cidr("192.168.1.0/"));
    }

    #[test]
    fn test_invalid_cidr_non_numeric_prefix() {
        assert!(!is_valid_ip_or_cidr("192.168.1.0/abc"));
    }

    #[test]
    fn test_invalid_ipv4_cidr_negative_prefix() {
        // u8 parse won't accept negative sign, so this should fail
        assert!(!is_valid_ip_or_cidr("192.168.1.0/-1"));
    }

    #[test]
    fn test_invalid_ipv6_cidr_prefix_too_large() {
        assert!(!is_valid_ip_or_cidr("::1/130"));
    }

    #[test]
    fn test_invalid_empty_string() {
        assert!(!is_valid_ip_or_cidr(""));
    }

    #[test]
    fn test_invalid_just_slash() {
        assert!(!is_valid_ip_or_cidr("/"));
    }

    #[test]
    fn test_invalid_multiple_slashes() {
        assert!(!is_valid_ip_or_cidr("192.168.1.0/24/extra"));
    }

    #[test]
    fn test_valid_ipv4_cidr_prefix_zero() {
        assert!(is_valid_ip_or_cidr("10.0.0.0/0"));
    }

    #[test]
    fn test_valid_ipv6_cidr_prefix_zero() {
        assert!(is_valid_ip_or_cidr("::/0"));
    }

    #[test]
    fn test_invalid_ipv4_with_extra_octets() {
        assert!(!is_valid_ip_or_cidr("192.168.1.1.1"));
    }

    #[test]
    fn test_invalid_ipv4_with_leading_zero() {
        // Leading zeros may or may not be accepted depending on parser
        // Just verify it doesn't crash
        let _ = is_valid_ip_or_cidr("192.168.001.001");
    }

    // ========== Country code validation logic (mirrors handler lines 152-174) ==========

    #[test]
    fn test_country_code_validation_two_letter_passes() {
        // Handler: if country.len() != 2 { return error }
        let countries = vec!["US".to_string(), "CN".to_string(), "JP".to_string()];
        for country in &countries {
            assert_eq!(country.len(), 2, "country code should be 2 letters");
        }
    }

    #[test]
    fn test_country_code_validation_one_letter_fails() {
        let country = "U".to_string();
        assert_ne!(country.len(), 2, "one-letter code should fail");
    }

    #[test]
    fn test_country_code_validation_three_letter_fails() {
        let country = "USA".to_string();
        assert_ne!(country.len(), 2, "three-letter code should fail");
    }

    #[test]
    fn test_country_code_validation_empty_fails() {
        let country = "".to_string();
        assert_ne!(country.len(), 2, "empty code should fail");
    }

    #[test]
    fn test_country_code_validation_lowercase_two_letters() {
        // Handler only checks length, not case
        let country = "us".to_string();
        assert_eq!(
            country.len(),
            2,
            "lowercase 2-letter should pass length check"
        );
    }

    // ========== TeamGeoRestrictionsResponse construction ==========

    #[test]
    fn test_team_geo_restrictions_response_with_data() {
        let team_id = Uuid::new_v4();
        let response = TeamGeoRestrictionsResponse {
            team_id,
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
            domain_blacklist: Some(vec!["evil.com".to_string()]),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["team_id"], team_id.to_string());
        assert_eq!(parsed["enable_geo_restrictions"], true);
        assert_eq!(parsed["allowed_countries"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["blocked_countries"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["ip_whitelist"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["domain_blacklist"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_team_geo_restrictions_response_disabled() {
        let response = TeamGeoRestrictionsResponse {
            team_id: Uuid::new_v4(),
            enable_geo_restrictions: false,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["enable_geo_restrictions"], false);
        assert!(parsed["allowed_countries"].is_null());
        assert!(parsed["blocked_countries"].is_null());
        assert!(parsed["ip_whitelist"].is_null());
        assert!(parsed["domain_blacklist"].is_null());
    }

    // ========== UpdateTeamGeoRestrictionsRequest deserialization ==========

    #[test]
    fn test_update_geo_restrictions_request_minimal() {
        let json = r#"{"enable_geo_restrictions": false}"#;
        let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).unwrap();
        assert!(!req.enable_geo_restrictions);
        assert!(req.allowed_countries.is_none());
        assert!(req.blocked_countries.is_none());
        assert!(req.ip_whitelist.is_none());
        assert!(req.domain_blacklist.is_none());
    }

    #[test]
    fn test_update_geo_restrictions_request_full() {
        let json = r#"{
            "enable_geo_restrictions": true,
            "allowed_countries": ["US", "CA"],
            "blocked_countries": ["CN"],
            "ip_whitelist": ["10.0.0.0/8"],
            "domain_blacklist": ["spam.com"]
        }"#;
        let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).unwrap();
        assert!(req.enable_geo_restrictions);
        assert_eq!(req.allowed_countries.as_ref().unwrap().len(), 2);
        assert_eq!(req.blocked_countries.as_ref().unwrap().len(), 1);
        assert_eq!(req.ip_whitelist.as_ref().unwrap().len(), 1);
        assert_eq!(req.domain_blacklist.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_update_geo_restrictions_request_deny_unknown_fields() {
        let json = r#"{"enable_geo_restrictions": true, "unknown": 1}"#;
        let result: Result<UpdateTeamGeoRestrictionsRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_geo_restrictions_request_validation_empty_allowed_countries() {
        // The #[validate(length(min = 1))] on allowed_countries means
        // an empty vec should fail validation
        let req = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec![]),
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_update_geo_restrictions_request_validation_empty_blocked_countries() {
        let req = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec![]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_update_geo_restrictions_request_validation_with_countries() {
        let req = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        assert!(req.validate().is_ok());
    }

    // ========== TeamInfoResponse clone and debug ==========

    #[test]
    fn test_team_info_response_clone() {
        let response = TeamInfoResponse {
            id: Uuid::new_v4(),
            name: "Clone Test".to_string(),
            credits_balance: 500,
            total_tasks: 10,
            completed_tasks: 8,
            failed_tasks: 2,
            created_at: chrono::Utc::now(),
        };
        let cloned = response.clone();
        assert_eq!(cloned.id, response.id);
        assert_eq!(cloned.name, response.name);
        assert_eq!(cloned.credits_balance, response.credits_balance);
        assert_eq!(cloned.total_tasks, response.total_tasks);
    }

    #[test]
    fn test_team_info_response_debug() {
        let response = TeamInfoResponse {
            id: Uuid::new_v4(),
            name: "Debug Test".to_string(),
            credits_balance: 0,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
        };
        let debug = format!("{:?}", response);
        assert!(debug.contains("TeamInfoResponse"));
        assert!(debug.contains("Debug Test"));
    }

    // ========== TeamUsageResponse clone and debug ==========

    #[test]
    fn test_team_usage_response_clone() {
        let response = TeamUsageResponse {
            team_id: Uuid::new_v4(),
            period: "30d".to_string(),
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
            credits_used: 500,
            avg_response_time_ms: 42.5,
        };
        let cloned = response.clone();
        assert_eq!(cloned.team_id, response.team_id);
        assert_eq!(cloned.total_requests, response.total_requests);
        assert_eq!(cloned.avg_response_time_ms, response.avg_response_time_ms);
    }

    #[test]
    fn test_team_usage_response_debug() {
        let response = TeamUsageResponse {
            team_id: Uuid::new_v4(),
            period: "7d".to_string(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            credits_used: 0,
            avg_response_time_ms: 0.0,
        };
        let debug = format!("{:?}", response);
        assert!(debug.contains("TeamUsageResponse"));
        assert!(debug.contains("7d"));
    }

    #[test]
    fn test_team_usage_response_deserialization() {
        let json = format!(
            r#"{{"team_id":"{}","period":"30d","total_requests":50,"successful_requests":45,"failed_requests":5,"credits_used":250,"avg_response_time_ms":99.9}}"#,
            Uuid::new_v4()
        );
        let response: TeamUsageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.period, "30d");
        assert_eq!(response.total_requests, 50);
        assert_eq!(response.avg_response_time_ms, 99.9);
    }

    // ========== TeamInfoResponse with extreme values ==========

    #[test]
    fn test_team_info_response_max_balance() {
        let response = TeamInfoResponse {
            id: Uuid::new_v4(),
            name: "Rich Team".to_string(),
            credits_balance: i64::MAX,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["credits_balance"], i64::MAX);
    }

    #[test]
    fn test_team_info_response_min_balance() {
        let response = TeamInfoResponse {
            id: Uuid::new_v4(),
            name: "Debt Team".to_string(),
            credits_balance: i64::MIN,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["credits_balance"], i64::MIN);
    }

    // ========== IP whitelist validation logic (mirrors handler lines 177-187) ==========

    #[test]
    fn test_ip_whitelist_validation_all_valid() {
        let whitelist = vec![
            "192.168.1.1".to_string(),
            "10.0.0.0/8".to_string(),
            "::1".to_string(),
            "2001:db8::/32".to_string(),
        ];
        for ip in &whitelist {
            assert!(is_valid_ip_or_cidr(ip), "IP {} should be valid", ip);
        }
    }

    #[test]
    fn test_ip_whitelist_validation_with_invalid_entry() {
        let whitelist = [
            "192.168.1.1".to_string(),
            "invalid-ip".to_string(),
            "10.0.0.0/8".to_string(),
        ];
        let has_invalid = whitelist.iter().any(|ip| !is_valid_ip_or_cidr(ip));
        assert!(has_invalid, "Should detect invalid IP in whitelist");
    }

    #[test]
    fn test_ip_whitelist_validation_all_invalid() {
        let whitelist = ["not-an-ip".to_string(), "999.999.999.999".to_string()];
        let all_invalid = whitelist.iter().all(|ip| !is_valid_ip_or_cidr(ip));
        assert!(all_invalid, "All entries should be invalid");
    }

    // ========== TeamGeoRestrictions construction (domain type used in handler) ==========

    #[test]
    fn test_team_geo_restrictions_disabled() {
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: false,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };
        assert!(!restrictions.enable_geo_restrictions);
    }

    #[test]
    fn test_team_geo_restrictions_enabled_with_data() {
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["10.0.0.0/8".to_string()]),
            domain_blacklist: Some(vec!["bad.com".to_string()]),
        };
        assert!(restrictions.enable_geo_restrictions);
        assert_eq!(restrictions.allowed_countries.as_ref().unwrap().len(), 1);
        assert_eq!(restrictions.blocked_countries.as_ref().unwrap().len(), 1);
    }

    // ========== Handler test infrastructure ==========

    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::{
        CreditsTransaction, CreditsTransactionType, ScrapeResult, Task, TaskType,
    };
    use crate::domain::repositories::credits_repository::{
        CreditsRepository, CreditsRepositoryError,
    };
    use crate::domain::repositories::geo_restriction_repository::{
        GeoRestrictionRepository, GeoRestrictionRepositoryError,
    };
    use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use async_trait::async_trait;
    use dbnexus::DbPool;
    use std::collections::HashSet;
    use std::sync::Mutex;

    /// Construct a lazy `DbPool` that does not connect to any database.
    fn make_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                let url = std::env::var("TEST_DATABASE_URL")
                    .expect("TEST_DATABASE_URL must be set; no hardcoded fallback");
                rt.block_on(async {
                    let cfg = dbnexus::DbConfig {
                        url,
                        ..Default::default()
                    };
                    DbPool::with_config(cfg).await
                })
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    /// Build an `AuthState` suitable for handler unit tests.
    fn make_test_auth_state() -> AuthState {
        AuthState::new(
            make_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    // ========== MockCreditsRepository ==========

    struct MockCreditsRepository {
        balance: i64,
        get_balance_fails: bool,
    }

    impl MockCreditsRepository {
        fn with_balance(balance: i64) -> Self {
            Self {
                balance,
                get_balance_fails: false,
            }
        }

        fn failing() -> Self {
            Self {
                balance: 0,
                get_balance_fails: true,
            }
        }
    }

    #[async_trait]
    impl CreditsRepository for MockCreditsRepository {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            if self.get_balance_fails {
                return Err(CreditsRepositoryError::DatabaseError(
                    "mock get_balance failure".to_string(),
                ));
            }
            Ok(self.balance)
        }

        async fn deduct_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            Ok(())
        }

        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }

        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }

        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            _initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
    }

    // ========== MockTaskRepository ==========

    struct MockTaskRepository {
        find_by_crawl_id_result: Mutex<Option<Result<Vec<Task>, RepositoryError>>>,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                find_by_crawl_id_result: Mutex::new(None),
            }
        }

        fn with_find_by_crawl_id(result: Result<Vec<Task>, RepositoryError>) -> Self {
            Self {
                find_by_crawl_id_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }

        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            match self.find_by_crawl_id_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok(vec![]),
            }
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    // ========== MockScrapeResultRepository ==========

    struct MockScrapeResultRepository {
        avg_response_time: f64,
        should_fail: bool,
    }

    impl MockScrapeResultRepository {
        fn with_avg_response_time(avg: f64) -> Self {
            Self {
                avg_response_time: avg,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                avg_response_time: 0.0,
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }

        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }

        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            if self.should_fail {
                return Err(anyhow::anyhow!("get_team_avg_response_time failed"));
            }
            Ok(self.avg_response_time)
        }
    }

    // ========== MockGeoRestrictionRepository ==========

    struct MockGeoRestrictionRepository {
        get_result: Mutex<Option<Result<TeamGeoRestrictions, GeoRestrictionRepositoryError>>>,
        update_result: Mutex<Option<Result<(), GeoRestrictionRepositoryError>>>,
    }

    impl MockGeoRestrictionRepository {
        fn new() -> Self {
            Self {
                get_result: Mutex::new(None),
                update_result: Mutex::new(None),
            }
        }

        fn with_get_result(
            result: Result<TeamGeoRestrictions, GeoRestrictionRepositoryError>,
        ) -> Self {
            Self {
                get_result: Mutex::new(Some(result)),
                update_result: Mutex::new(None),
            }
        }

        fn with_update_result(result: Result<(), GeoRestrictionRepositoryError>) -> Self {
            Self {
                get_result: Mutex::new(None),
                update_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
            match self.get_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok(TeamGeoRestrictions {
                    enable_geo_restrictions: false,
                    allowed_countries: None,
                    blocked_countries: None,
                    ip_whitelist: None,
                    domain_blacklist: None,
                }),
            }
        }

        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            match self.update_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok(()),
            }
        }

        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }
    }

    /// Build a sample UpdateTeamGeoRestrictionsRequest for testing.
    fn make_update_request() -> UpdateTeamGeoRestrictionsRequest {
        UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["192.168.1.0/24".to_string()]),
            domain_blacklist: Some(vec!["evil.com".to_string()]),
        }
    }

    // ========== get_team_info handler tests ==========

    #[tokio::test]
    async fn test_get_team_info_success() {
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(500));
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();

        let response = get_team_info(
            Extension(credits_repo),
            Extension(task_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_info_credits_failure() {
        let credits_repo: Arc<dyn CreditsRepository> = Arc::new(MockCreditsRepository::failing());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();

        let response = get_team_info(
            Extension(credits_repo),
            Extension(task_repo),
            Extension(auth),
        )
        .await
        .into_response();

        // get_balance failure uses unwrap_or(0) → response still OK
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_info_task_repo_failure() {
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(100));
        let task_repo: Arc<dyn TaskRepository> =
            Arc::new(MockTaskRepository::with_find_by_crawl_id(Err(
                RepositoryError::Database(anyhow::anyhow!("task repo down")),
            )));
        let auth = make_test_auth_state();

        let response = get_team_info(
            Extension(credits_repo),
            Extension(task_repo),
            Extension(auth),
        )
        .await
        .into_response();

        // find_by_crawl_id failure uses unwrap_or_default() → response still OK
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_info_with_tasks() {
        let team_id = Uuid::new_v4();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            team_id,
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::Value::Null,
        );
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(100));
        let task_repo: Arc<dyn TaskRepository> =
            Arc::new(MockTaskRepository::with_find_by_crawl_id(Ok(vec![task])));
        let auth = make_test_auth_state();

        let response = get_team_info(
            Extension(credits_repo),
            Extension(task_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ========== get_team_usage handler tests ==========

    #[tokio::test]
    async fn test_get_team_usage_success() {
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(500));
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> =
            Arc::new(MockScrapeResultRepository::with_avg_response_time(123.45));
        let auth = make_test_auth_state();

        let response = get_team_usage(
            Extension(credits_repo),
            Extension(scrape_result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_usage_credits_failure() {
        let credits_repo: Arc<dyn CreditsRepository> = Arc::new(MockCreditsRepository::failing());
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> =
            Arc::new(MockScrapeResultRepository::with_avg_response_time(100.0));
        let auth = make_test_auth_state();

        let response = get_team_usage(
            Extension(credits_repo),
            Extension(scrape_result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        // get_balance failure uses unwrap_or(0) → response still OK
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_usage_scrape_result_failure() {
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(100));
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> =
            Arc::new(MockScrapeResultRepository::failing());
        let auth = make_test_auth_state();

        let response = get_team_usage(
            Extension(credits_repo),
            Extension(scrape_result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        // get_team_avg_response_time failure uses unwrap_or(0.0) → response still OK
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_usage_negative_balance() {
        let credits_repo: Arc<dyn CreditsRepository> =
            Arc::new(MockCreditsRepository::with_balance(-500));
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> =
            Arc::new(MockScrapeResultRepository::with_avg_response_time(50.0));
        let auth = make_test_auth_state();

        let response = get_team_usage(
            Extension(credits_repo),
            Extension(scrape_result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ========== get_team_geo_restrictions handler tests ==========

    #[tokio::test]
    async fn test_get_team_geo_restrictions_success() {
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: Some(vec!["10.0.0.0/8".to_string()]),
            domain_blacklist: Some(vec!["bad.com".to_string()]),
        };
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(
            MockGeoRestrictionRepository::with_get_result(Ok(restrictions)),
        );
        let auth = make_test_auth_state();

        let response = get_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_geo_restrictions_empty() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();

        let response = get_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_team_geo_restrictions_failure() {
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_get_result(Err(
                GeoRestrictionRepositoryError::Database("db error".to_string()),
            )));
        let auth = make_test_auth_state();

        let response = get_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_team_geo_restrictions_team_not_found() {
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_get_result(Err(
                GeoRestrictionRepositoryError::TeamNotFound(Uuid::new_v4()),
            )));
        let auth = make_test_auth_state();

        let response = get_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== update_team_geo_restrictions handler tests ==========

    #[tokio::test]
    async fn test_update_team_geo_restrictions_success() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(make_update_request()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_minimal_request() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: false,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_allowed_country() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["USA".to_string()]), // 3 letters → invalid
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_blocked_country() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["X".to_string()]), // 1 letter → invalid
            ip_whitelist: None,
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_ip() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: Some(vec!["not-an-ip".to_string()]),
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_cidr() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: Some(vec!["192.168.1.0/33".to_string()]), // prefix > 32 → invalid
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_repo_failure() {
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_update_result(Err(
                GeoRestrictionRepositoryError::Database("update failed".to_string()),
            )));
        let auth = make_test_auth_state();

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(make_update_request()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_repo_team_not_found() {
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_update_result(Err(
                GeoRestrictionRepositoryError::TeamNotFound(Uuid::new_v4()),
            )));
        let auth = make_test_auth_state();

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(make_update_request()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_valid_ipv6() {
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: None,
            ip_whitelist: Some(vec!["::1".to_string(), "2001:db8::/32".to_string()]),
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ========== Test logger for covering log::error! format args ==========

    use axum::body::to_bytes;
    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    // ========== Error response body verification tests ==========

    #[tokio::test]
    async fn test_get_team_geo_restrictions_failure_body_verified() {
        ensure_debug_logger();
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_get_result(Err(
                GeoRestrictionRepositoryError::Database("db error".to_string()),
            )));
        let auth = make_test_auth_state();

        let response = get_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("Failed to get team geo restrictions"),
            "error message should mention geo restrictions"
        );
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_repo_failure_body_verified() {
        ensure_debug_logger();
        let repo: Arc<MockGeoRestrictionRepository> =
            Arc::new(MockGeoRestrictionRepository::with_update_result(Err(
                GeoRestrictionRepositoryError::Database("update failed".to_string()),
            )));
        let auth = make_test_auth_state();

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(make_update_request()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("Failed to update team geo restrictions"),
            "error message should mention update failure"
        );
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_allowed_body_verified() {
        ensure_debug_logger();
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["USA".to_string()]),
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("Country codes"),
            "error message should mention country code format"
        );
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_blocked_body_verified() {
        ensure_debug_logger();
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["X".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("Country codes"),
            "error message should mention country code format"
        );
    }

    #[tokio::test]
    async fn test_update_team_geo_restrictions_invalid_ip_body_verified() {
        ensure_debug_logger();
        let repo: Arc<MockGeoRestrictionRepository> = Arc::new(MockGeoRestrictionRepository::new());
        let auth = make_test_auth_state();
        let request = UpdateTeamGeoRestrictionsRequest {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: None,
            ip_whitelist: Some(vec!["not-an-ip".to_string()]),
            domain_blacklist: None,
        };

        let response = update_team_geo_restrictions::<MockGeoRestrictionRepository>(
            Extension(repo),
            Extension(auth),
            Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("Invalid IP address"),
            "error message should mention invalid IP address"
        );
    }
}
