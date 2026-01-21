// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect, Set,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::auth::{AuditDecision, AuditLogEntry};
use crate::domain::repositories::audit_log_repository::{AuditLogRepository, AuditRepositoryError};
use crate::infrastructure::database::entities::auth::audit_log::{
    Column as AuditColumn, Entity as AuditEntity,
};

#[derive(Clone)]
pub struct AuditLogRepositoryImpl {
    db: Arc<DatabaseConnection>,
}

impl AuditLogRepositoryImpl {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditLogRepository for AuditLogRepositoryImpl {
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditRepositoryError> {
        let entry_cloned = entry.clone();
        let metadata_value = serde_json::to_value(entry_cloned.metadata).unwrap_or_default();
        let scope_used_value = entry_cloned
            .scope_used
            .map(|s| serde_json::to_value(s).unwrap_or_default());
        let ip_address_value = entry_cloned.ip_address.map(|ip| ip.to_string());
        let active_model =
            crate::infrastructure::database::entities::auth::audit_log::ActiveModel {
                id: Set(entry_cloned.id),
                api_key_id: Set(entry_cloned.api_key_id),
                team_id: Set(entry_cloned.team_id),
                requested_action: Set(entry_cloned.requested_action),
                decision: Set(entry_cloned.decision.to_string()),
                denial_reason: Set(entry_cloned.denial_reason),
                scope_used: Set(scope_used_value),
                ip_address: Set(ip_address_value),
                trace_id: Set(entry_cloned.trace_id),
                user_agent: Set(entry_cloned.user_agent),
                request_path: Set(entry_cloned.request_path),
                request_method: Set(entry_cloned.request_method),
                metadata: Set(metadata_value),
                ..Default::default()
            };

        AuditEntity::insert(active_model).exec(&*self.db).await?;
        Ok(entry.clone())
    }

    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let logs = AuditEntity::find()
            .filter(AuditColumn::ApiKeyId.eq(api_key_id))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .offset(offset)
            .all(&*self.db)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn find_by_team_id(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let logs = AuditEntity::find()
            .filter(AuditColumn::TeamId.eq(team_id))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .offset(offset)
            .all(&*self.db)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn find_denied_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError> {
        let logs = AuditEntity::find()
            .filter(AuditColumn::ApiKeyId.eq(api_key_id))
            .filter(AuditColumn::Decision.eq(AuditDecision::Deny.to_string()))
            .order_by(AuditColumn::CreatedAt, Order::Desc)
            .limit(limit)
            .all(&*self.db)
            .await?;

        Ok(logs.into_iter().map(|l| l.into()).collect())
    }

    async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditRepositoryError> {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days);

        let result = AuditEntity::delete_many()
            .filter(AuditColumn::CreatedAt.lt(cutoff))
            .exec(&*self.db)
            .await?;

        Ok(result.rows_affected as u64)
    }
}
