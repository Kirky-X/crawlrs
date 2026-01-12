// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Service for audit logging

use crate::domain::auth::{ApiKeyScope, AuditDecision, AuditLogEntry};
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepository;
use sea_orm::DatabaseConnection;
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

/// Service for managing audit logs
#[derive(Clone)]
pub struct AuditService {
    audit_repo: AuditLogRepository,
}

impl AuditService {
    /// Create a new service
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            audit_repo: AuditLogRepository::new(db),
        }
    }

    /// Create a new audit log entry
    pub async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
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
    pub async fn log_allow(
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

        self.log(entry).await
    }

    /// Log a deny decision
    pub async fn log_deny(
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

        self.log(entry).await
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
