// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for team_handler public API.
//!
//! Tests DTO serialization (TeamInfoResponse, TeamUsageResponse) and
//! the GeoRestriction DTOs used by get/update team geo restriction handlers.

use chrono::Utc;
use uuid::Uuid;

use crawlrs::application::dto::geo_restriction_request::{
    TeamGeoRestrictionsResponse, UpdateTeamGeoRestrictionsRequest,
};
use crawlrs::domain::services::team_service::TeamGeoRestrictions;
use crawlrs::presentation::handlers::team_handler::{TeamInfoResponse, TeamUsageResponse};

// =============================================================================
// TeamInfoResponse serialization
// =============================================================================

#[test]
fn tc_team_info_response_serialization_round_trip() {
    let response = TeamInfoResponse {
        id: Uuid::new_v4(),
        name: "Test Team".to_string(),
        credits_balance: 1000,
        total_tasks: 50,
        completed_tasks: 40,
        failed_tasks: 10,
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: TeamInfoResponse = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.id, response.id);
    assert_eq!(parsed.name, "Test Team");
    assert_eq!(parsed.credits_balance, 1000);
    assert_eq!(parsed.total_tasks, 50);
    assert_eq!(parsed.completed_tasks, 40);
    assert_eq!(parsed.failed_tasks, 10);
}

#[test]
fn tc_team_info_response_json_field_names() {
    let response = TeamInfoResponse {
        id: Uuid::new_v4(),
        name: "Fields Team".to_string(),
        credits_balance: -500,
        total_tasks: 0,
        completed_tasks: 0,
        failed_tasks: 0,
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["name"], "Fields Team");
    assert_eq!(parsed["credits_balance"], -500);
    assert_eq!(parsed["total_tasks"], 0);
}

#[test]
fn tc_team_info_response_negative_balance() {
    let response = TeamInfoResponse {
        id: Uuid::new_v4(),
        name: "Negative".to_string(),
        credits_balance: -100,
        total_tasks: 5,
        completed_tasks: 3,
        failed_tasks: 2,
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: TeamInfoResponse = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.credits_balance, -100);
}

#[test]
fn tc_team_info_response_clone_preserves_fields() {
    let response = TeamInfoResponse {
        id: Uuid::new_v4(),
        name: "Clone Team".to_string(),
        credits_balance: 250,
        total_tasks: 10,
        completed_tasks: 8,
        failed_tasks: 2,
        created_at: Utc::now(),
    };
    let cloned = response.clone();
    assert_eq!(response.id, cloned.id);
    assert_eq!(response.name, cloned.name);
    assert_eq!(response.credits_balance, cloned.credits_balance);
}

// =============================================================================
// TeamUsageResponse serialization
// =============================================================================

#[test]
fn tc_team_usage_response_serialization_round_trip() {
    let response = TeamUsageResponse {
        team_id: Uuid::new_v4(),
        period: "30d".to_string(),
        total_requests: 1000,
        successful_requests: 950,
        failed_requests: 50,
        credits_used: 5000,
        avg_response_time_ms: 250.5,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: TeamUsageResponse = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.team_id, response.team_id);
    assert_eq!(parsed.period, "30d");
    assert_eq!(parsed.total_requests, 1000);
    assert_eq!(parsed.successful_requests, 950);
    assert_eq!(parsed.failed_requests, 50);
    assert_eq!(parsed.credits_used, 5000);
    assert_eq!(parsed.avg_response_time_ms, 250.5);
}

#[test]
fn tc_team_usage_response_json_fields() {
    let response = TeamUsageResponse {
        team_id: Uuid::new_v4(),
        period: "7d".to_string(),
        total_requests: 100,
        successful_requests: 90,
        failed_requests: 10,
        credits_used: 500,
        avg_response_time_ms: 0.0,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["period"], "7d");
    assert_eq!(parsed["total_requests"], 100);
    assert_eq!(parsed["avg_response_time_ms"], 0.0);
}

#[test]
fn tc_team_usage_response_zero_values() {
    let response = TeamUsageResponse {
        team_id: Uuid::nil(),
        period: String::new(),
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        credits_used: 0,
        avg_response_time_ms: 0.0,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: TeamUsageResponse = serde_json::from_str(&json).expect("must deserialize");
    assert_eq!(parsed.team_id, Uuid::nil());
    assert_eq!(parsed.total_requests, 0);
}

#[test]
fn tc_team_usage_response_clone_preserves_fields() {
    let response = TeamUsageResponse {
        team_id: Uuid::new_v4(),
        period: "1d".to_string(),
        total_requests: 5,
        successful_requests: 4,
        failed_requests: 1,
        credits_used: 25,
        avg_response_time_ms: 100.0,
    };
    let cloned = response.clone();
    assert_eq!(response.team_id, cloned.team_id);
    assert_eq!(response.period, cloned.period);
    assert_eq!(response.credits_used, cloned.credits_used);
}

// =============================================================================
// TeamGeoRestrictionsResponse serialization
// =============================================================================

#[test]
fn tc_team_geo_restrictions_response_with_defaults() {
    let response = TeamGeoRestrictionsResponse {
        team_id: Uuid::new_v4(),
        enable_geo_restrictions: false,
        allowed_countries: None,
        blocked_countries: None,
        ip_whitelist: None,
        domain_blacklist: None,
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    // None should serialize to null.
    assert!(parsed["allowed_countries"].is_null());
    assert!(parsed["blocked_countries"].is_null());
}

#[test]
fn tc_team_geo_restrictions_response_serialization() {
    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string(), "CA".to_string()]),
        blocked_countries: Some(vec!["CN".to_string()]),
        ip_whitelist: None,
        domain_blacklist: None,
    };
    let response = TeamGeoRestrictionsResponse {
        team_id: Uuid::new_v4(),
        enable_geo_restrictions: restrictions.enable_geo_restrictions,
        allowed_countries: restrictions.allowed_countries.clone(),
        blocked_countries: restrictions.blocked_countries.clone(),
        ip_whitelist: restrictions.ip_whitelist.clone(),
        domain_blacklist: restrictions.domain_blacklist.clone(),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(parsed["allowed_countries"][0], "US");
    assert_eq!(parsed["allowed_countries"][1], "CA");
    assert_eq!(parsed["blocked_countries"][0], "CN");
    assert_eq!(parsed["enable_geo_restrictions"], true);
}

#[test]
fn tc_team_geo_restrictions_response_empty_lists() {
    let response = TeamGeoRestrictionsResponse {
        team_id: Uuid::new_v4(),
        enable_geo_restrictions: true,
        allowed_countries: Some(vec![]),
        blocked_countries: Some(vec![]),
        ip_whitelist: Some(vec![]),
        domain_blacklist: Some(vec![]),
    };
    let json = serde_json::to_string(&response).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must parse JSON");
    assert_eq!(
        parsed["allowed_countries"],
        serde_json::Value::Array(vec![])
    );
    assert_eq!(
        parsed["blocked_countries"],
        serde_json::Value::Array(vec![])
    );
}

// =============================================================================
// UpdateTeamGeoRestrictionsRequest deserialization
// =============================================================================

#[test]
fn tc_update_team_geo_restrictions_request_valid() {
    let json =
        r#"{"enable_geo_restrictions":true,"allowed_countries":["US"],"blocked_countries":["CN"]}"#;
    let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.allowed_countries, Some(vec!["US".to_string()]));
    assert_eq!(req.blocked_countries, Some(vec!["CN".to_string()]));
    assert!(req.enable_geo_restrictions);
}

#[test]
fn tc_update_team_geo_restrictions_request_empty_optional_fields() {
    let json = r#"{"enable_geo_restrictions":false}"#;
    let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).expect("must parse");
    assert!(req.allowed_countries.is_none());
    assert!(req.blocked_countries.is_none());
    assert!(!req.enable_geo_restrictions);
}

#[test]
fn tc_update_team_geo_restrictions_request_only_allowed() {
    let json = r#"{"enable_geo_restrictions":true,"allowed_countries":["US","CA","MX"]}"#;
    let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.allowed_countries.unwrap().len(), 3);
    assert!(req.blocked_countries.is_none());
}

#[test]
fn tc_update_team_geo_restrictions_request_only_blocked() {
    let json = r#"{"enable_geo_restrictions":true,"blocked_countries":["RU"]}"#;
    let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).expect("must parse");
    assert!(req.allowed_countries.is_none());
    assert_eq!(req.blocked_countries.unwrap(), vec!["RU".to_string()]);
}

#[test]
fn tc_update_team_geo_restrictions_request_empty_arrays() {
    let json = r#"{"enable_geo_restrictions":true,"allowed_countries":[],"blocked_countries":[]}"#;
    let req: UpdateTeamGeoRestrictionsRequest = serde_json::from_str(json).expect("must parse");
    assert_eq!(req.allowed_countries, Some(vec![]));
    assert_eq!(req.blocked_countries, Some(vec![]));
}

// =============================================================================
// TeamGeoRestrictions default
// =============================================================================

#[test]
fn tc_team_geo_restrictions_default_is_empty() {
    let restrictions = TeamGeoRestrictions::default();
    assert!(restrictions.allowed_countries.is_none());
    assert!(restrictions.blocked_countries.is_none());
    assert!(!restrictions.enable_geo_restrictions);
}

#[test]
fn tc_team_geo_restrictictions_clone_preserves_lists() {
    let restrictions = TeamGeoRestrictions {
        enable_geo_restrictions: true,
        allowed_countries: Some(vec!["US".to_string()]),
        blocked_countries: Some(vec!["CN".to_string()]),
        ip_whitelist: None,
        domain_blacklist: None,
    };
    let cloned = restrictions.clone();
    assert!(cloned.allowed_countries.is_some());
    assert!(cloned.blocked_countries.is_some());
    assert_eq!(
        cloned.allowed_countries.as_ref().unwrap(),
        restrictions.allowed_countries.as_ref().unwrap()
    );
    assert_eq!(
        cloned.blocked_countries.as_ref().unwrap(),
        restrictions.blocked_countries.as_ref().unwrap()
    );
}
