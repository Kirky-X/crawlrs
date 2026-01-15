// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Authentication domain models for API Key scopes and feature flags

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// API Key scope permissions
///
/// Represents the fine-grained permissions for an API Key.
/// Scopes control what endpoints an API Key can access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyScope {
    /// Permission to access read-only endpoints (search, scrape GET)
    pub read: bool,
    /// Permission to access write endpoints (config, upload)
    pub write: bool,
    /// Permission to access administrative endpoints (team, billing)
    pub admin: bool,
    /// Maximum number of search requests per hour
    pub search_limit: u32,
    /// Maximum number of scrape requests per hour
    pub scrape_limit: u32,
}

impl Default for ApiKeyScope {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }
}

impl std::fmt::Display for ApiKeyScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ApiKeyScope(read={}, write={}, admin={}, search_limit={}, scrape_limit={})",
            self.read, self.write, self.admin, self.search_limit, self.scrape_limit
        )
    }
}

impl ApiKeyScope {
    /// Create a new scope with all permissions disabled
    pub fn denied() -> Self {
        Self {
            read: false,
            write: false,
            admin: false,
            search_limit: 0,
            scrape_limit: 0,
        }
    }

    /// Create a new scope with read-only access
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
            search_limit: 100,
            scrape_limit: 50,
        }
    }

    /// Create a new scope with full access
    pub fn full_access() -> Self {
        Self {
            read: true,
            write: true,
            admin: true,
            search_limit: u32::MAX,
            scrape_limit: u32::MAX,
        }
    }

    /// Check if the scope has permission for a required scope
    pub fn has_permission(&self, permission: ScopePermission) -> bool {
        match permission {
            ScopePermission::Read => self.read,
            ScopePermission::Write => self.write,
            ScopePermission::Admin => self.admin,
        }
    }

    /// Check if the scope allows the requested search count
    pub fn allows_search_count(&self, count: u32) -> bool {
        self.search_limit == u32::MAX || count <= self.search_limit
    }

    /// Check if the scope allows the requested scrape count
    pub fn allows_scrape_count(&self, count: u32) -> bool {
        self.scrape_limit == u32::MAX || count <= self.scrape_limit
    }
}

/// Scope permission types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScopePermission {
    /// Read-only access
    Read,
    /// Write access
    Write,
    /// Admin access
    Admin,
}

impl std::fmt::Display for ScopePermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopePermission::Read => write!(f, "read"),
            ScopePermission::Write => write!(f, "write"),
            ScopePermission::Admin => write!(f, "admin"),
        }
    }
}

impl From<ScopePermission> for ApiKeyScope {
    fn from(permission: ScopePermission) -> Self {
        match permission {
            ScopePermission::Read => Self {
                read: true,
                write: false,
                admin: false,
                search_limit: 100,
                scrape_limit: 50,
            },
            ScopePermission::Write => Self {
                read: true,
                write: true,
                admin: false,
                search_limit: 100,
                scrape_limit: 50,
            },
            ScopePermission::Admin => Self {
                read: true,
                write: true,
                admin: true,
                search_limit: 100,
                scrape_limit: 50,
            },
        }
    }
}

/// Feature flag for runtime feature control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub rollout_percentage: u8,
    pub metadata: serde_json::Value,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub stopped_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl FeatureFlag {
    /// Check if the feature is currently active
    pub fn is_active(&self) -> bool {
        self.enabled
            && self.started_at.is_none_or(|t| t <= chrono::Utc::now())
            && self.stopped_at.is_none_or(|t| t > chrono::Utc::now())
    }

    /// Check if a specific API Key should have access based on rollout
    pub fn should_enable_for_key(&self, api_key_id: Uuid) -> bool {
        if !self.is_active() {
            return false;
        }

        if self.rollout_percentage == 100 {
            return true;
        }

        if self.rollout_percentage == 0 {
            return false;
        }

        // Deterministic rollout based on API Key ID
        let bytes = api_key_id.as_bytes();
        let mut hash: u64 = 0;
        for &byte in bytes {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        let bucket = hash % 100;
        bucket < self.rollout_percentage as u64
    }
}

/// Per-API-Key feature flag override
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagOverride {
    pub id: Uuid,
    pub feature_flag_id: Uuid,
    pub api_key_id: Uuid,
    pub enabled: bool,
}

/// Audit log entry for authentication and authorization decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub requested_action: String,
    pub decision: AuditDecision,
    pub denial_reason: Option<String>,
    pub scope_used: Option<ApiKeyScope>,
    pub ip_address: Option<std::net::IpAddr>,
    pub trace_id: Option<Uuid>,
    pub user_agent: Option<String>,
    pub request_path: Option<String>,
    pub request_method: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Audit decision type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditDecision {
    Allow,
    Deny,
}

impl std::fmt::Display for AuditDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditDecision::Allow => write!(f, "ALLOW"),
            AuditDecision::Deny => write!(f, "DENY"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_api_key_scope_default() {
        let scope = ApiKeyScope::default();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_api_key_scope_denied() {
        let scope = ApiKeyScope::denied();
        assert!(!scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 0);
        assert_eq!(scope.scrape_limit, 0);
    }

    #[test]
    fn test_api_key_scope_read_only() {
        let scope = ApiKeyScope::read_only();
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_api_key_scope_full_access() {
        let scope = ApiKeyScope::full_access();
        assert!(scope.read);
        assert!(scope.write);
        assert!(scope.admin);
        assert_eq!(scope.search_limit, u32::MAX);
        assert_eq!(scope.scrape_limit, u32::MAX);
    }

    #[test]
    fn test_api_key_scope_has_permission() {
        let scope = ApiKeyScope::full_access();
        assert!(scope.has_permission(ScopePermission::Read));
        assert!(scope.has_permission(ScopePermission::Write));
        assert!(scope.has_permission(ScopePermission::Admin));

        let scope = ApiKeyScope::read_only();
        assert!(scope.has_permission(ScopePermission::Read));
        assert!(!scope.has_permission(ScopePermission::Write));
        assert!(!scope.has_permission(ScopePermission::Admin));
    }

    #[test]
    fn test_api_key_scope_allows_search_count() {
        let scope = ApiKeyScope::default();
        assert!(scope.allows_search_count(50));
        assert!(scope.allows_search_count(100));
        assert!(!scope.allows_search_count(101));

        let scope = ApiKeyScope::full_access();
        assert!(scope.allows_search_count(u32::MAX));
    }

    #[test]
    fn test_api_key_scope_allows_scrape_count() {
        let scope = ApiKeyScope::default();
        assert!(scope.allows_scrape_count(25));
        assert!(scope.allows_scrape_count(50));
        assert!(!scope.allows_scrape_count(51));

        let scope = ApiKeyScope::full_access();
        assert!(scope.allows_scrape_count(u32::MAX));
    }

    #[test]
    fn test_feature_flag_is_active() {
        let flag = FeatureFlag {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            enabled: true,
            rollout_percentage: 100,
            metadata: serde_json::json!({}),
            started_at: None,
            stopped_at: None,
        };
        assert!(flag.is_active());

        let flag = FeatureFlag {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            enabled: false,
            rollout_percentage: 100,
            metadata: serde_json::json!({}),
            started_at: None,
            stopped_at: None,
        };
        assert!(!flag.is_active());
    }

    #[test]
    fn test_feature_flag_should_enable_for_key() {
        let flag = FeatureFlag {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            enabled: true,
            rollout_percentage: 100,
            metadata: serde_json::json!({}),
            started_at: None,
            stopped_at: None,
        };
        let api_key_id = Uuid::new_v4();
        assert!(flag.should_enable_for_key(api_key_id));

        let flag = FeatureFlag {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            description: None,
            enabled: true,
            rollout_percentage: 0,
            metadata: serde_json::json!({}),
            started_at: None,
            stopped_at: None,
        };
        assert!(!flag.should_enable_for_key(api_key_id));
    }

    #[test]
    fn test_audit_decision_display() {
        assert_eq!(AuditDecision::Allow.to_string(), "ALLOW");
        assert_eq!(AuditDecision::Deny.to_string(), "DENY");
    }

    #[test]
    fn test_scope_permission_display() {
        assert_eq!(ScopePermission::Read.to_string(), "read");
        assert_eq!(ScopePermission::Write.to_string(), "write");
        assert_eq!(ScopePermission::Admin.to_string(), "admin");
    }

    #[test]
    fn test_api_key_scope_display() {
        let scope = ApiKeyScope::default();
        let display = scope.to_string();
        assert!(display.contains("read=true"));
        assert!(display.contains("write=false"));
        assert!(display.contains("admin=false"));
    }
}
