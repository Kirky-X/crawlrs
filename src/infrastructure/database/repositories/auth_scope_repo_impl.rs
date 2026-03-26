// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dbnexus::DbPool;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;
use crate::common::time_utils;

use crate::domain::auth::ApiKeyScope;
use crate::domain::repositories::auth_scope_repository::{AuthScopeRepository, RepositoryError};
use crate::infrastructure::database::entities::api_key::{
    Column as ApiKeyColumn, Entity as ApiKeyEntity,
};
use crate::infrastructure::database::entities::auth::scope::{
    Column as ScopeColumn, Entity as ScopeEntity,
};

#[derive(Clone)]
pub struct AuthScopeRepositoryImpl {
    pool: Arc<DbPool>,
}

impl AuthScopeRepositoryImpl {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AuthScopeRepository for AuthScopeRepositoryImpl {
    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyScope>, RepositoryError> {
        let session = self.pool.get_session("admin").await?;
        
        let conn = session.connection()?;
        
        let scope = ScopeEntity::find()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .one(conn)
            .await?;

        Ok(scope.map(|s| ApiKeyScope {
            read: s.read,
            write: s.write,
            admin: s.admin,
            search_limit: s.search_limit as u32,
            scrape_limit: s.scrape_limit as u32,
        }))
    }

    async fn find_by_api_key(&self, key: &str) -> Result<Option<ApiKeyScope>, RepositoryError> {
        let session = self.pool.get_session("admin").await?;
        
        let conn = session.connection()?;
        
        let api_key = ApiKeyEntity::find()
            .filter(ApiKeyColumn::Key.eq(key))
            .one(conn)
            .await?;

        match api_key {
            Some(key) => self.find_by_api_key_id(key.id).await,
            None => Ok(None),
        }
    }

    async fn upsert(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, RepositoryError> {
        let session = self.pool.get_session("admin").await?;
        
        let conn = session.connection()?;
        
        let existing = self.find_by_api_key_id(api_key_id).await?;

        match existing {
            Some(_existing_scope) => {
                // Update existing scope - need to get the ID from existing record
                let existing_model = ScopeEntity::find()
                    .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
                    .one(conn)
                    .await?
                    .ok_or_else(|| RepositoryError::NotFound(format!("Scope not found for api_key_id: {}", api_key_id)))?;

                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        id: sea_orm::ActiveValue::Unchanged(existing_model.id),
                        api_key_id: sea_orm::ActiveValue::Unchanged(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        created_at: sea_orm::ActiveValue::Unchanged(existing_model.created_at),
                        updated_at: Set(Utc::now().with_timezone(&time_utils::UTC_OFFSET)),
                    };

                ScopeEntity::update(scope_active_model)
                    .exec(conn)
                    .await?;

                Ok(scope)
            }
            None => {
                let now = Utc::now().with_timezone(&time_utils::UTC_OFFSET);
                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        id: Set(Uuid::new_v4()),
                        api_key_id: Set(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        created_at: Set(now),
                        updated_at: Set(now),
                    };

                ScopeEntity::insert(scope_active_model)
                    .exec(conn)
                    .await?;
                Ok(scope)
            }
        }
    }

    async fn delete_by_api_key_id(&self, api_key_id: Uuid) -> Result<bool, RepositoryError> {
        let session = self.pool.get_session("admin").await?;
        
        let conn = session.connection()?;
        
        let result = ScopeEntity::delete_many()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .exec(conn)
            .await?;

        Ok(result.rows_affected > 0)
    }
}
