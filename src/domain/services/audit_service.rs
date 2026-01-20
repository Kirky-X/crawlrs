// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for audit logging

use crate::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepository;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AuditServiceError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
}

/// Builder for creating audit log entries
#[derive(Clone, Debug)]
pub struct AuditLogBuilder {
    api_key_id: Option<Uuid>,
    team_id: Option<Uuid>,
    requested_action: String,
    decision: AuditDecision,
    denial_reason: Option<String>,
    scope_used: Option<ApiKeyScope>,
    ip_address: Option<std::net::IpAddr>,
    trace_id: Option<Uuid>,
    user_agent: Option<String>,
    request_path: Option<String>,
    request_method: Option<String>,
    metadata: serde_json::Value,
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
}

/// Trait for AuditService - enables dependency injection
#[async_trait::async_trait]
pub trait AuditServiceTrait: Send + Sync {
    /// Create a new audit log entry
    async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError>;

    /// Log an allow decision
    async fn log_allow(
        &self,
        action: String,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError>;

    /// Log a deny decision
    async fn log_deny(
        &self,
        action: String,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: String,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError>;
}

/// Service for managing audit logs
#[derive(Clone)]
pub struct AuditService {
    audit_repo: AuditLogRepository,
}

impl AuditService {
    /// Create a new service
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            audit_repo: AuditLogRepository::new(db),
        }
    }

    /// Create a new audit log entry
    pub async fn _log_impl(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        debug!(
            "Creating audit log: action={}, decision={}",
            entry.requested_action, entry.decision
        );
        self.audit_repo
            .create(entry)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    /// Log an allow decision
    pub async fn _log_allow_impl(
        &self,
        action: impl Into<String>,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        let entry = AuditLogBuilder::new(action, AuditDecision::Allow)
            .with_api_key_id(api_key_id)
            .with_team_id(team_id)
            .with_scope(scope)
            .build();

        self._log_impl(entry).await
    }

    /// Log a deny decision
    pub async fn _log_deny_impl(
        &self,
        action: impl Into<String>,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: impl Into<String>,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        let entry = AuditLogBuilder::new(action, AuditDecision::Deny)
            .with_api_key_id(api_key_id.unwrap_or_default())
            .with_team_id(team_id.unwrap_or_default())
            .with_denial_reason(reason)
            .with_scope(scope.unwrap_or_default())
            .build();

        self._log_impl(entry).await
    }

    /// Create a new audit log entry (public wrapper)
    pub async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        self._log_impl(entry).await
    }

    /// Log an allow decision (public wrapper)
    pub async fn log_allow(
        &self,
        action: impl Into<String>,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        self._log_allow_impl(action, api_key_id, team_id, scope)
            .await
    }

    /// Log a deny decision (public wrapper)
    pub async fn log_deny(
        &self,
        action: impl Into<String>,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: impl Into<String>,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        self._log_deny_impl(action, api_key_id, team_id, reason, scope)
            .await
    }

    /// Get audit logs for an API Key
    pub async fn get_logs_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_api_key_id(api_key_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    /// Get audit logs for a team
    pub async fn get_logs_for_team(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_by_team_id(team_id, limit, offset)
            .await
            .map_err(Into::into)
    }

    /// Get denied requests for an API Key
    pub async fn get_denied_requests(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
        self.audit_repo
            .find_denied_for_key(api_key_id, limit)
            .await
            .map_err(Into::into)
    }

    /// Clean up old audit logs
    pub async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditServiceError> {
        self.audit_repo
            .cleanup_old_logs(retention_days)
            .await
            .map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl AuditServiceTrait for AuditService {
    async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
        self._log_impl(entry).await
    }

    async fn log_allow(
        &self,
        action: String,
        api_key_id: Uuid,
        team_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<(), AuditServiceError> {
        self._log_allow_impl(action, api_key_id, team_id, scope)
            .await
    }

    async fn log_deny(
        &self,
        action: String,
        api_key_id: Option<Uuid>,
        team_id: Option<Uuid>,
        reason: String,
        scope: Option<ApiKeyScope>,
    ) -> Result<(), AuditServiceError> {
        self._log_deny_impl(action, api_key_id, team_id, reason, scope)
            .await
    }
}
