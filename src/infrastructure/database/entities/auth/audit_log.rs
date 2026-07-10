// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_logs")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::{AuditDecision, AuditLogEntry};
    use sea_orm::ActiveValue;

    fn make_model() -> Model {
        Model {
            id: Uuid::new_v4(),
            api_key_id: Some(Uuid::new_v4()),
            team_id: Some(Uuid::new_v4()),
            requested_action: "scrape:create".to_string(),
            decision: "ALLOW".to_string(),
            denial_reason: None,
            scope_used: Some(serde_json::json!({"read": true, "write": false, "admin": false})),
            ip_address: Some("192.168.1.1".to_string()),
            trace_id: Some(Uuid::new_v4()),
            user_agent: Some("test-agent/1.0".to_string()),
            request_path: Some("/v1/scrape".to_string()),
            request_method: Some("POST".to_string()),
            metadata: serde_json::json!({"key": "value"}),
            created_at: chrono::Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_model_construction() {
        let id = Uuid::new_v4();
        let model = Model {
            id,
            api_key_id: None,
            team_id: None,
            requested_action: "task:query".to_string(),
            decision: "DENY".to_string(),
            denial_reason: Some("Insufficient permissions".to_string()),
            scope_used: None,
            ip_address: None,
            trace_id: None,
            user_agent: None,
            request_path: None,
            request_method: None,
            metadata: serde_json::json!({}),
            created_at: chrono::Utc::now().fixed_offset(),
        };
        assert_eq!(model.id, id);
        assert!(model.api_key_id.is_none());
        assert!(model.team_id.is_none());
        assert_eq!(model.requested_action, "task:query");
        assert_eq!(model.decision, "DENY");
        assert_eq!(
            model.denial_reason,
            Some("Insufficient permissions".to_string())
        );
        assert!(model.scope_used.is_none());
    }

    #[test]
    fn test_model_clone() {
        let model = make_model();
        let cloned = model.clone();
        assert_eq!(model, cloned);
    }

    #[test]
    fn test_model_debug() {
        let model = make_model();
        let debug = format!("{:?}", model);
        assert!(debug.contains("Model"));
        assert!(debug.contains("scrape:create"));
        assert!(debug.contains("ALLOW"));
    }

    #[test]
    fn test_model_partial_eq() {
        let model1 = make_model();
        let model2 = model1.clone();
        assert_eq!(model1, model2);

        let model3 = Model {
            decision: "DENY".to_string(),
            ..make_model()
        };
        assert_ne!(model1, model3);
    }

    #[test]
    fn test_from_model_to_audit_log_entry_allow() {
        let model = make_model();
        let entry: AuditLogEntry = model.into();

        assert_eq!(entry.decision, AuditDecision::Allow);
        assert!(entry.denial_reason.is_none());
        assert_eq!(entry.requested_action, "scrape:create");
        assert!(entry.api_key_id.is_some());
        assert!(entry.team_id.is_some());
    }

    #[test]
    fn test_from_model_to_audit_log_entry_deny() {
        let model = Model {
            decision: "DENY".to_string(),
            denial_reason: Some("Rate limit exceeded".to_string()),
            ..make_model()
        };
        let entry: AuditLogEntry = model.into();

        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.denial_reason, Some("Rate limit exceeded".to_string()));
    }

    #[test]
    fn test_from_model_to_audit_log_entry_unknown_decision_defaults_to_deny() {
        let model = Model {
            decision: "UNKNOWN".to_string(),
            ..make_model()
        };
        let entry: AuditLogEntry = model.into();
        assert_eq!(entry.decision, AuditDecision::Deny);
    }

    #[test]
    fn test_from_model_to_audit_log_entry_ip_parsing() {
        let model = Model {
            ip_address: Some("10.0.0.1".to_string()),
            ..make_model()
        };
        let entry: AuditLogEntry = model.into();
        assert!(entry.ip_address.is_some());
    }

    #[test]
    fn test_from_model_to_audit_log_entry_invalid_ip_fallback() {
        let model = Model {
            ip_address: Some("not-an-ip".to_string()),
            ..make_model()
        };
        let entry: AuditLogEntry = model.into();
        // Invalid IP should fall back to 0.0.0.0
        assert_eq!(
            entry.ip_address,
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)))
        );
    }

    #[test]
    fn test_from_model_to_audit_log_entry_scope_used() {
        let model = make_model();
        let entry: AuditLogEntry = model.into();
        assert!(entry.scope_used.is_some());
        let scope = entry.scope_used.unwrap();
        assert!(scope.read);
        assert!(!scope.write);
    }

    #[test]
    fn test_active_model_with_set_values() {
        let id = Uuid::new_v4();
        let active = ActiveModel {
            id: ActiveValue::Set(id),
            api_key_id: ActiveValue::Set(None),
            team_id: ActiveValue::Set(None),
            requested_action: ActiveValue::Set("search:query".to_string()),
            decision: ActiveValue::Set("ALLOW".to_string()),
            denial_reason: ActiveValue::Set(None),
            scope_used: ActiveValue::Set(None),
            ip_address: ActiveValue::Set(None),
            trace_id: ActiveValue::Set(None),
            user_agent: ActiveValue::Set(None),
            request_path: ActiveValue::Set(None),
            request_method: ActiveValue::Set(None),
            metadata: ActiveValue::Set(serde_json::json!({})),
            created_at: ActiveValue::Set(chrono::Utc::now().fixed_offset()),
        };
        assert_eq!(active.id.as_ref(), &id);
        assert_eq!(
            active.requested_action.as_ref(),
            &"search:query".to_string()
        );
    }

    #[test]
    fn test_relation_enum_is_empty() {
        // audit_logs has no relations - the Relation enum has no variants.
        // We verify it compiles and can be instantiated (Copy + Clone).
        fn assert_relation_is_copy<T: Copy + Clone>() {}
        assert_relation_is_copy::<Relation>();
    }
}
