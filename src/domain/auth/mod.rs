// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Authentication domain models for API Key scopes

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod scope;

/// API Key scope permissions
///
/// Represents the fine-grained permissions for an API Key.
/// Scopes control what endpoints an API Key can access.
///
/// # 安全提示
///
/// `search_limit` 和 `scrape_limit` 字段包含配额限制信息，
/// 外部模块应使用 `allows_search_count()` 和 `allows_scrape_count()` 方法检查限制。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyScope {
    /// Permission to access read-only endpoints (search, scrape GET)
    pub read: bool,
    /// Permission to access write endpoints (config, upload)
    pub write: bool,
    /// Permission to access administrative endpoints (team, billing)
    pub admin: bool,
    /// Maximum number of search requests per hour (敏感信息)
    pub(crate) search_limit: u32,
    /// Maximum number of scrape requests per hour (敏感信息)
    pub(crate) scrape_limit: u32,
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

/// Audit log entry for authentication and authorization decisions
///
/// # 安全提示
///
/// `ip_address`、`trace_id`、`user_agent`、`request_path`、`request_method` 字段
/// 包含敏感的用户信息，仅对 crate 可见，外部模块应使用相应的 getter 方法访问。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub requested_action: String,
    pub decision: AuditDecision,
    pub denial_reason: Option<String>,
    pub scope_used: Option<ApiKeyScope>,
    /// IP 地址 (敏感信息)
    pub(crate) ip_address: Option<std::net::IpAddr>,
    /// 追踪 ID (敏感信息)
    pub(crate) trace_id: Option<Uuid>,
    /// 用户代理 (敏感信息)
    pub(crate) user_agent: Option<String>,
    /// 请求路径 (敏感信息)
    pub(crate) request_path: Option<String>,
    /// 请求方法 (敏感信息)
    pub(crate) request_method: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl AuditLogEntry {
    /// 获取 IP 地址
    ///
    /// # 安全提示
    ///
    /// 此方法返回用户 IP 地址，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn ip_address(&self) -> Option<std::net::IpAddr> {
        self.ip_address
    }

    /// 获取追踪 ID
    pub fn trace_id(&self) -> Option<Uuid> {
        self.trace_id
    }

    /// 获取用户代理
    ///
    /// # 安全提示
    ///
    /// 此方法返回用户代理字符串，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }

    /// 获取请求路径
    ///
    /// # 安全提示
    ///
    /// 此方法返回请求路径，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn request_path(&self) -> Option<&str> {
        self.request_path.as_deref()
    }

    /// 获取请求方法
    pub fn request_method(&self) -> Option<&str> {
        self.request_method.as_deref()
    }
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

    #[test]
    fn test_scope_permission_from_read_conversion() {
        let scope = ApiKeyScope::from(ScopePermission::Read);
        assert!(scope.read);
        assert!(!scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_scope_permission_from_write_conversion() {
        let scope = ApiKeyScope::from(ScopePermission::Write);
        assert!(scope.read);
        assert!(scope.write);
        assert!(!scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }

    #[test]
    fn test_scope_permission_from_admin_conversion() {
        let scope = ApiKeyScope::from(ScopePermission::Admin);
        assert!(scope.read);
        assert!(scope.write);
        assert!(scope.admin);
        assert_eq!(scope.search_limit, 100);
        assert_eq!(scope.scrape_limit, 50);
    }
}
