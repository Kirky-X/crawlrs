// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl repository implementation using Sea-ORM with Mapper

use crate::domain::models::{Crawl, CrawlStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::infrastructure::database::entities::crawl;
use crate::infrastructure::persistence::mappers::CrawlMapper;
use async_trait::async_trait;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use std::sync::Arc;
use uuid::Uuid;

/// Crawl repository implementation using Sea-ORM
pub struct CrawlRepositoryImpl {
    /// Database connection
    db: Arc<DatabaseConnection>,
}

impl CrawlRepositoryImpl {
    /// Create new crawl repository instance
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }

    /// Get database connection reference
    pub fn db(&self) -> &Arc<DatabaseConnection> {
        &self.db
    }
}

#[async_trait]
impl CrawlRepository for CrawlRepositoryImpl {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let entity = CrawlMapper::to_entity(crawl);
        let active_model = crawl::ActiveModel::from(entity);

        active_model
            .insert(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
        let entity = crawl::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(CrawlMapper::to_domain))
    }

    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let entity = CrawlMapper::to_entity(crawl);
        let active_model = crawl::ActiveModel::from(entity);

        active_model
            .update(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_completed_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_failed_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn update_status(&self, id: Uuid, status: CrawlStatus) -> Result<(), RepositoryError> {
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.status = status;
            domain.updated_at = chrono::Utc::now();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_total_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(self.db.as_ref())
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Crawl>, RepositoryError> {
        let entities = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
            .order_by_desc(crawl::Column::CreatedAt)
            .limit(limit as u64)
            .offset(offset as u64)
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(CrawlMapper::to_domain_list(entities))
    }

    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError> {
        let count = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
            .count(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(count)
    }
}
