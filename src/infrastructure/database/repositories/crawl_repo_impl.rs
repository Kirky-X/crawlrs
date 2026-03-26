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
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use std::sync::Arc;
use uuid::Uuid;

/// Crawl repository implementation using Sea-ORM
pub struct CrawlRepositoryImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl CrawlRepositoryImpl {
    /// Create new crawl repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// Get database pool reference
    pub fn pool(&self) -> &Arc<DbPool> {
        &self.pool
    }
}

#[async_trait]
impl CrawlRepository for CrawlRepositoryImpl {
    async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = CrawlMapper::to_entity(crawl);
        let active_model = crawl::ActiveModel::from(entity);

        active_model
            .insert(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = crawl::Entity::find_by_id(id)
            .one(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(CrawlMapper::to_domain))
    }

    async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = CrawlMapper::to_entity(crawl);
        let active_model = crawl::ActiveModel::from(entity);

        active_model
            .update(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(crawl.clone())
    }

    async fn increment_completed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_completed_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_failed_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_failed_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn update_status(&self, id: Uuid, status: CrawlStatus) -> Result<(), RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.status = status;
            domain.updated_at = chrono::Utc::now();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(conn)
                .await
                .map_err(|e| RepositoryError::Database(e.into()))?;
        }

        Ok(())
    }

    async fn increment_total_tasks(&self, id: Uuid) -> Result<(), RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;
        
        if let Some(entity) = crawl::Entity::find_by_id(id)
            .one(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?
        {
            let mut domain = CrawlMapper::to_domain(entity);
            domain.increment_total_tasks();

            let updated_entity = CrawlMapper::to_entity(&domain);
            let active_model = crawl::ActiveModel::from(updated_entity);

            active_model
                .update(conn)
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
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entities = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
            .order_by_desc(crawl::Column::CreatedAt)
            .limit(limit as u64)
            .offset(offset as u64)
            .all(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(CrawlMapper::to_domain_list(entities))
    }

    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let count = crawl::Entity::find()
            .filter(crawl::Column::TeamId.eq(team_id))
            .count(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(count)
    }
}
