// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeResult repository implementation using dbnexus

use crate::domain::models::ScrapeResult;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::scrape_result as db_entity;
use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

/// ScrapeResult repository implementation using dbnexus
pub struct ScrapeResultRepositoryImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl ScrapeResultRepositoryImpl {
    /// Create new ScrapeResult repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }

    /// Convert domain model to database active model
    fn to_active_model(result: &ScrapeResult) -> db_entity::ActiveModel {
        use chrono::FixedOffset;
        db_entity::ActiveModel {
            id: Set(result.id),
            task_id: Set(result.task_id),
            url: Set(result.url.clone()),
            status_code: Set(result.status_code),
            content: Set(result.content.clone()),
            content_type: Set(result.content_type.clone()),
            headers: Set(Some(result.headers.clone())),
            meta_data: Set(Some(result.meta_data.clone())),
            screenshot: Set(result.screenshot.clone()),
            response_time_ms: Set(result.response_time_ms),
            created_at: Set(result.created_at.and_utc().with_timezone(&FixedOffset::east_opt(0).unwrap())),
        }
    }

    /// Convert database model to domain model
    fn to_domain(model: db_entity::Model) -> ScrapeResult {
        ScrapeResult {
            id: model.id,
            task_id: model.task_id,
            url: model.url,
            status_code: model.status_code,
            content: model.content,
            content_type: model.content_type,
            headers: model.headers.unwrap_or(serde_json::json!({})),
            meta_data: model.meta_data.unwrap_or(serde_json::json!({})),
            screenshot: model.screenshot,
            response_time_ms: model.response_time_ms,
            created_at: model.created_at.naive_utc(),
        }
    }
}

#[async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryImpl {
    async fn save(&self, result: ScrapeResult) -> anyhow::Result<()> {
        let session = self.pool.get_session("scraper").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        let conn = session.connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;
        
        let active_model = Self::to_active_model(&result);

        active_model.insert(conn).await
            .map_err(|e| anyhow::anyhow!("Failed to insert: {}", e))?;
        
        Ok(())
    }

    async fn find_by_task_id(&self, task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        let conn = session.connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;
        
        let result = db_entity::Entity::find()
            .filter(db_entity::Column::TaskId.eq(task_id))
            .one(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;
        
        Ok(result.map(Self::to_domain))
    }

    async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }

        let session = self.pool.get_session("admin").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        let conn = session.connection()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;
        
        let results = db_entity::Entity::find()
            .filter(db_entity::Column::TaskId.is_in(task_ids.to_vec()))
            .all(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;
        
        Ok(results.into_iter().map(Self::to_domain).collect())
    }

    async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
        tracing::warn!("Team average response time tracking not yet implemented - returning 0.0");
        Ok(0.0)
    }
}
