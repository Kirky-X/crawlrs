// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! AuditLogBuilder — 用于构建 [`AuditLogEntry`] 的 builder 模式实现。
//!
//! 从 `audit_service.rs` 拆出（架构 MEDIUM 1：单文件混合 Service + Builder + Error
//! 三个独立 concern，违反单一职责原则）。本文件只包含 builder 本身，
//! Service / Error 仍保留在 `audit_service.rs` 中。

use crate::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};
use uuid::Uuid;

/// Builder for creating audit log entries
#[derive(Clone, Debug)]
pub struct AuditLogBuilder {
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
}

impl AuditLogBuilder {
    /// Create a new builder
    pub fn new(action: impl Into<String>, decision: AuditDecision) -> Self {
        Self {
            api_key_id: None,
            team_id: None,
            requested_action: action.into(),
            decision,
            denial_reason: None,
            scope_used: None,
            ip_address: None,
            trace_id: None,
            user_agent: None,
            request_path: None,
            request_method: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Set API Key ID
    pub fn with_api_key_id(mut self, id: Uuid) -> Self {
        self.api_key_id = Some(id);
        self
    }

    /// Set team ID
    pub fn with_team_id(mut self, id: Uuid) -> Self {
        self.team_id = Some(id);
        self
    }

    /// Set denial reason
    pub fn with_denial_reason(mut self, reason: impl Into<String>) -> Self {
        self.denial_reason = Some(reason.into());
        self
    }

    /// Set scope used
    pub fn with_scope(mut self, scope: ApiKeyScope) -> Self {
        self.scope_used = Some(scope);
        self
    }

    /// Conditionally set API Key ID.
    ///
    /// 当 `id` 为 `None`（如未认证请求被拒绝）时保持字段为 `None`，
    /// 避免 `unwrap_or_default()` 写入 nil UUID (`00000000-...`)。
    /// nil UUID 与 NULL 在数据库中语义不同：
    /// - NULL 表示"未知/不适用"
    /// - nil UUID 会被 `find_by_api_key_id(nil_uuid)` 误匹配，混淆真实 API key 的审计日志
    pub fn maybe_with_api_key_id(mut self, id: Option<Uuid>) -> Self {
        self.api_key_id = id;
        self
    }

    /// Conditionally set team ID（语义同 `maybe_with_api_key_id`）。
    pub fn maybe_with_team_id(mut self, id: Option<Uuid>) -> Self {
        self.team_id = id;
        self
    }

    /// Conditionally set scope used（语义同 `maybe_with_api_key_id`）。
    pub fn maybe_with_scope(mut self, scope: Option<ApiKeyScope>) -> Self {
        self.scope_used = scope;
        self
    }

    /// Set IP address
    pub fn with_ip_address(mut self, ip: impl Into<std::net::IpAddr>) -> Self {
        self.ip_address = Some(ip.into());
        self
    }

    /// Set trace ID
    pub fn with_trace_id(mut self, id: Uuid) -> Self {
        self.trace_id = Some(id);
        self
    }

    /// Set user agent
    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Set request path
    pub fn with_request_path(mut self, path: impl Into<String>) -> Self {
        self.request_path = Some(path.into());
        self
    }

    /// Set request method
    pub fn with_request_method(mut self, method: impl Into<String>) -> Self {
        self.request_method = Some(method.into());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if let serde_json::Value::Object(map) = &mut self.metadata {
            map.insert(key.into(), value);
        }
        self
    }

    /// Build the audit log entry
    pub fn build(self) -> AuditLogEntry {
        AuditLogEntry {
            id: Uuid::new_v4(),
            api_key_id: self.api_key_id,
            team_id: self.team_id,
            requested_action: self.requested_action,
            decision: self.decision,
            denial_reason: self.denial_reason,
            scope_used: self.scope_used,
            ip_address: self.ip_address,
            trace_id: self.trace_id,
            user_agent: self.user_agent,
            request_path: self.request_path,
            request_method: self.request_method,
            metadata: self.metadata,
            created_at: chrono::Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::Ipv4Addr;

    #[test]
    fn test_audit_log_builder_new() {
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow);
        assert_eq!(builder.requested_action, "test_action");
        assert_eq!(builder.decision, AuditDecision::Allow);
    }

    #[test]
    fn test_audit_log_builder_with_api_key_id() {
        let api_key_id = Uuid::new_v4();
        let builder =
            AuditLogBuilder::new("test_action", AuditDecision::Allow).with_api_key_id(api_key_id);
        assert_eq!(builder.api_key_id, Some(api_key_id));
    }

    #[test]
    fn test_audit_log_builder_with_team_id() {
        let team_id = Uuid::new_v4();
        let builder =
            AuditLogBuilder::new("test_action", AuditDecision::Allow).with_team_id(team_id);
        assert_eq!(builder.team_id, Some(team_id));
    }

    #[test]
    fn test_audit_log_builder_with_denial_reason() {
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Deny)
            .with_denial_reason("insufficient permissions");
        assert_eq!(
            builder.denial_reason,
            Some("insufficient permissions".to_string())
        );
    }

    #[test]
    fn test_audit_log_builder_with_ip_address() {
        let ip: std::net::IpAddr = Ipv4Addr::new(192, 168, 1, 1).into();
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow).with_ip_address(ip);
        assert!(builder.ip_address.is_some());
    }

    #[test]
    fn test_audit_log_builder_with_trace_id() {
        let trace_id = Uuid::new_v4();
        let builder =
            AuditLogBuilder::new("test_action", AuditDecision::Allow).with_trace_id(trace_id);
        assert_eq!(builder.trace_id, Some(trace_id));
    }

    #[test]
    fn test_audit_log_builder_with_user_agent() {
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow)
            .with_user_agent("Test Agent/1.0");
        assert_eq!(builder.user_agent, Some("Test Agent/1.0".to_string()));
    }

    #[test]
    fn test_audit_log_builder_with_request_path() {
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow)
            .with_request_path("/api/v1/test");
        assert_eq!(builder.request_path, Some("/api/v1/test".to_string()));
    }

    #[test]
    fn test_audit_log_builder_with_request_method() {
        let builder =
            AuditLogBuilder::new("test_action", AuditDecision::Allow).with_request_method("POST");
        assert_eq!(builder.request_method, Some("POST".to_string()));
    }

    #[test]
    fn test_audit_log_builder_with_metadata() {
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow)
            .with_metadata("key1", json!("value1"))
            .with_metadata("key2", json!(123));

        match &builder.metadata {
            serde_json::Value::Object(map) => {
                assert_eq!(map.get("key1"), Some(&json!("value1")));
                assert_eq!(map.get("key2"), Some(&json!(123)));
            }
            _ => panic!("Expected object metadata"),
        }
    }

    #[test]
    fn test_audit_log_builder_build_returns_entry() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let ip: std::net::IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();

        let entry = AuditLogBuilder::new("test_action", AuditDecision::Allow)
            .with_api_key_id(api_key_id)
            .with_team_id(team_id)
            .with_trace_id(trace_id)
            .with_ip_address(ip)
            .with_request_path("/api/test")
            .with_request_method("GET")
            .build();

        assert_eq!(entry.requested_action, "test_action");
        assert_eq!(entry.decision, AuditDecision::Allow);
        assert_eq!(entry.api_key_id, Some(api_key_id));
        assert_eq!(entry.team_id, Some(team_id));
        assert_eq!(entry.trace_id, Some(trace_id));
        assert_eq!(entry.request_path, Some("/api/test".to_string()));
        assert_eq!(entry.request_method, Some("GET".to_string()));
        assert!(entry.id != Uuid::nil());
        assert!(entry.created_at <= chrono::Utc::now());
    }

    #[test]
    fn test_audit_log_builder_with_scope() {
        let scope = ApiKeyScope::default();
        let builder =
            AuditLogBuilder::new("test_action", AuditDecision::Allow).with_scope(scope.clone());
        assert_eq!(builder.scope_used, Some(scope));
    }

    // ---- maybe_with_* methods (M-2 fix: preserve None semantics) ----

    #[test]
    fn test_maybe_with_api_key_id_none_preserves_none() {
        // None must stay None — writing NULL to DB, not nil UUID.
        let builder =
            AuditLogBuilder::new("anon.deny", AuditDecision::Deny).maybe_with_api_key_id(None);
        assert_eq!(builder.api_key_id, None);
    }

    #[test]
    fn test_maybe_with_api_key_id_some_sets_value() {
        let id = Uuid::new_v4();
        let builder =
            AuditLogBuilder::new("authed.deny", AuditDecision::Deny).maybe_with_api_key_id(Some(id));
        assert_eq!(builder.api_key_id, Some(id));
    }

    #[test]
    fn test_maybe_with_team_id_none_preserves_none() {
        let builder =
            AuditLogBuilder::new("anon.deny", AuditDecision::Deny).maybe_with_team_id(None);
        assert_eq!(builder.team_id, None);
    }

    #[test]
    fn test_maybe_with_team_id_some_sets_value() {
        let id = Uuid::new_v4();
        let builder =
            AuditLogBuilder::new("team.deny", AuditDecision::Deny).maybe_with_team_id(Some(id));
        assert_eq!(builder.team_id, Some(id));
    }

    #[test]
    fn test_maybe_with_scope_none_preserves_none() {
        let builder = AuditLogBuilder::new("anon.deny", AuditDecision::Deny).maybe_with_scope(None);
        assert_eq!(builder.scope_used, None);
    }

    #[test]
    fn test_maybe_with_scope_some_sets_value() {
        let scope = ApiKeyScope::full_access();
        let builder =
            AuditLogBuilder::new("authed.deny", AuditDecision::Deny).maybe_with_scope(Some(scope.clone()));
        assert_eq!(builder.scope_used, Some(scope));
    }

    #[test]
    fn test_maybe_with_methods_build_entry_with_none_fields() {
        // End-to-end: maybe_with_* None must produce an entry whose fields are None,
        // not nil UUID / default scope. This is the M-2 regression guard.
        let entry = AuditLogBuilder::new("anonymous.action", AuditDecision::Deny)
            .maybe_with_api_key_id(None)
            .maybe_with_team_id(None)
            .with_denial_reason("auth required")
            .maybe_with_scope(None)
            .build();

        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.api_key_id, None);
        assert_eq!(entry.team_id, None);
        assert_eq!(entry.scope_used, None);
        assert_eq!(entry.denial_reason.as_deref(), Some("auth required"));
        // ID must still be a fresh UUID, not nil
        assert_ne!(entry.id, Uuid::nil());
    }

    #[test]
    fn test_maybe_with_methods_build_entry_with_some_fields() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let scope = ApiKeyScope::read_only();

        let entry = AuditLogBuilder::new("authed.action", AuditDecision::Deny)
            .maybe_with_api_key_id(Some(api_key_id))
            .maybe_with_team_id(Some(team_id))
            .with_denial_reason("insufficient scope")
            .maybe_with_scope(Some(scope.clone()))
            .build();

        assert_eq!(entry.api_key_id, Some(api_key_id));
        assert_eq!(entry.team_id, Some(team_id));
        assert_eq!(entry.scope_used, Some(scope));
    }

    #[test]
    fn test_maybe_with_overrides_previous_with_value() {
        // maybe_with_* should override a prior with_* call (last-write-wins),
        // including clearing back to None.
        let id = Uuid::new_v4();
        let builder = AuditLogBuilder::new("action", AuditDecision::Allow)
            .with_api_key_id(id)
            .maybe_with_api_key_id(None);
        assert_eq!(builder.api_key_id, None);
    }

    #[test]
    fn test_audit_log_builder_build_includes_metadata_and_scope() {
        let scope = ApiKeyScope::full_access();
        let entry = AuditLogBuilder::new("delete_resource", AuditDecision::Deny)
            .with_denial_reason("no permission")
            .with_scope(scope.clone())
            .with_metadata("resource_id", json!("res-123"))
            .with_user_agent("AuditClient/2.0")
            .build();

        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.denial_reason.as_deref(), Some("no permission"));
        assert_eq!(entry.scope_used, Some(scope));
        assert_eq!(entry.user_agent.as_deref(), Some("AuditClient/2.0"));
        match &entry.metadata {
            serde_json::Value::Object(map) => {
                assert_eq!(map.get("resource_id"), Some(&json!("res-123")));
            }
            _ => panic!("Expected object metadata"),
        }
    }

    #[test]
    fn test_audit_log_builder_with_metadata_after_non_object_is_noop() {
        // Start with default (Object). Verify metadata insertion works.
        let builder = AuditLogBuilder::new("test_action", AuditDecision::Allow)
            .with_metadata("k1", json!("v1"));
        // The builder always starts with Object metadata, so insertion should succeed
        match &builder.metadata {
            serde_json::Value::Object(map) => {
                assert_eq!(map.get("k1"), Some(&json!("v1")));
            }
            _ => panic!("Expected object metadata"),
        }
    }
}
