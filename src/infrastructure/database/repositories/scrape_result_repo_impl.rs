// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeResult repository implementation using dbnexus

use crate::domain::models::scrape_result::ScrapeResult;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
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
}

#[async_trait]
impl ScrapeResultRepository for ScrapeResultRepositoryImpl {
    async fn save(&self, result: ScrapeResult) -> anyhow::Result<()> {
        let session = self.pool.get_session("scraper").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        ScrapeResult::insert(&session, result).await
            .map_err(|e| anyhow::anyhow!("Failed to insert: {}", e))?;
        
        Ok(())
    }

    async fn find_by_task_id(&self, task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        let results = ScrapeResult::find()
            .filter("task_id", task_id)
            .limit(1)
            .execute(&session)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;
        
        Ok(results.first().cloned())
    }

    async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }

        let session = self.pool.get_session("admin").await
            .map_err(|e| anyhow::anyhow!("Failed to get session: {}", e))?;
        
        let mut results = Vec::new();
        
        for task_id in task_ids {
            let task_results = ScrapeResult::find()
                .filter("task_id", task_id)
                .execute(&session)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to find: {}", e))?;
            
            results.extend(task_results);
        }
        
        Ok(results)
    }

    async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
        tracing::warn!("Team average response time tracking not yet implemented - returning 0.0");
        Ok(0.0)
    }
}
