// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub requested_action: String,
    pub decision: String,
    pub denial_reason: Option<String>,
    pub scope_used: Option<sea_orm::prelude::Json>,
    pub ip_address: Option<String>,
    pub trace_id: Option<Uuid>,
    pub user_agent: Option<String>,
    pub request_path: Option<String>,
    pub request_method: Option<String>,
    pub metadata: sea_orm::prelude::Json,
    pub created_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for crate::domain::auth::AuditLogEntry {
    fn from(model: Model) -> crate::domain::auth::AuditLogEntry {
        crate::domain::auth::AuditLogEntry {
            id: model.id,
            api_key_id: model.api_key_id,
            team_id: model.team_id,
            requested_action: model.requested_action,
            decision: match model.decision.as_str() {
                "ALLOW" => crate::domain::auth::AuditDecision::Allow,
                _ => crate::domain::auth::AuditDecision::Deny,
            },
            denial_reason: model.denial_reason,
            scope_used: model
                .scope_used
                .map(|s| serde_json::from_value(s).unwrap_or_default()),
            ip_address: model.ip_address.map(|ip| {
                ip.parse()
                    .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)))
            }),
            trace_id: model.trace_id,
            user_agent: model.user_agent,
            request_path: model.request_path,
            request_method: model.request_method,
            metadata: model.metadata,
            created_at: model.created_at.with_timezone(&chrono::Utc),
        }
    }
}
