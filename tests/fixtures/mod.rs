// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test fixtures for crawlrs
//!
//! This module provides reusable test fixtures and helpers for testing
//! various components of the crawlrs system.

use chrono::{Duration, Utc};
use uuid::Uuid;

/// Generate a unique API key for testing
pub fn generate_test_api_key() -> Uuid {
    Uuid::new_v4()
}

/// Generate test API key scopes
pub fn generate_test_scope(
    read: bool,
    write: bool,
    admin: bool,
) -> crate::domain::auth::ApiKeyScope {
    crate::domain::auth::ApiKeyScope {
        read,
        write,
        admin,
        search_limit: if read { 100 } else { 0 },
        scrape_limit: if read { 50 } else { 0 },
    }
}

/// Generate test feature flags
pub fn generate_test_feature_flag(
    enabled: bool,
    rollout_percentage: u8,
    active_now: bool,
) -> crate::domain::auth::FeatureFlag {
    let now = Utc::now();

    crate::domain::auth::FeatureFlag {
        id: Uuid::new_v4(),
        name: format!(
            "test_flag_{}",
            Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        ),
        description: Some("Test feature flag".to_string()),
        enabled,
        rollout_percentage,
        metadata: serde_json::json!({
            "test": true,
            "created_at": now.to_rfc3339()
        }),
        started_at: if active_now {
            Some(now - Duration::hours(1))
        } else {
            Some(now + Duration::hours(1))
        },
        stopped_at: None,
    }
}

/// Generate test audit log entry
pub fn generate_test_audit_log(
    decision: crate::domain::auth::AuditDecision,
) -> crate::domain::auth::AuditLogEntry {
    crate::domain::auth::AuditLogEntry {
        id: Uuid::new_v4(),
        api_key_id: Some(generate_test_api_key()),
        team_id: Some(Uuid::new_v4()),
        requested_action: "test:action".to_string(),
        decision,
        denial_reason: if matches!(decision, crate::domain::auth::AuditDecision::Deny) {
            Some("Test denial reason".to_string())
        } else {
            None
        },
        scope_used: Some(generate_test_scope(true, false, false)),
        ip_address: Some("127.0.0.1".parse().expect("Failed to parse IP address")),
        trace_id: Some(Uuid::new_v4()),
        user_agent: Some("Test User Agent".to_string()),
        request_path: Some("/test/path".to_string()),
        request_method: Some("GET".to_string()),
        metadata: serde_json::json!({"test": true}),
        created_at: Utc::now(),
    }
}

/// Mock test data for integration tests
pub struct MockTestData {
    pub api_key_id: Uuid,
    pub team_id: Uuid,
    pub feature_flag_id: Uuid,
    pub audit_log_id: Uuid,
}

impl Default for MockTestData {
    fn default() -> Self {
        Self {
            api_key_id: generate_test_api_key(),
            team_id: Uuid::new_v4(),
            feature_flag_id: Uuid::new_v4(),
            audit_log_id: Uuid::new_v4(),
        }
    }
}

/// Test environment configuration
pub struct TestEnvironmentConfig {
    pub database_url: String,
    pub redis_url: String,
    pub chromium_url: String,
    pub flaresolverr_url: String,
}

impl Default for TestEnvironmentConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://postgres:postgres@localhost:5432/crawlrs_test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            chromium_url: "http://localhost:9222".to_string(),
            flaresolverr_url: "http://localhost:8191".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_test_api_key() {
        let key1 = generate_test_api_key();
        let key2 = generate_test_api_key();
        assert_ne!(key1, key2);
        assert!(!key1.is_nil());
        assert!(!key2.is_nil());
    }

    #[test]
    fn test_generate_test_scope() {
        let scope = generate_test_scope(true, false, false);
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_generate_test_feature_flag() {
        let flag = generate_test_feature_flag(true, 100, true);
        assert!(flag.enabled);
        assert_eq!(flag.rollout_percentage, 100);
        assert!(flag.is_active());
    }

    #[test]
    fn test_generate_test_audit_log() {
        let allow_entry = generate_test_audit_log(crate::domain::auth::AuditDecision::Allow);
        assert!(matches!(
            allow_entry.decision,
            crate::domain::auth::AuditDecision::Allow
        ));
        assert!(allow_entry.denial_reason.is_none());

        let deny_entry = generate_test_audit_log(crate::domain::auth::AuditDecision::Deny);
        assert!(matches!(
            deny_entry.decision,
            crate::domain::auth::AuditDecision::Deny
        ));
        assert!(deny_entry.denial_reason.is_some());
    }

    #[test]
    fn test_mock_test_data() {
        let data1 = MockTestData::default();
        let data2 = MockTestData::default();
        assert_ne!(data1.api_key_id, data2.api_key_id);
        assert_ne!(data1.team_id, data2.team_id);
    }
}
