// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::domain::auth::ApiKeyScope;
use crate::infrastructure::database::entities::api_key::{
    Column as ApiKeyColumn, Entity as ApiKeyEntity,
};
use crate::infrastructure::database::entities::auth::scope::{
    Column as ScopeColumn, Entity as ScopeEntity,
};

#[derive(Clone)]
pub struct AuthScopeRepository {
    db: DatabaseConnection,
}

impl AuthScopeRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyScope>, sea_orm::DbErr> {
        let scope = ScopeEntity::find()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .one(&self.db)
            .await?;

        Ok(scope.map(|s| ApiKeyScope {
            read: s.read,
            write: s.write,
            admin: s.admin,
            search_limit: s.search_limit as u32,
            scrape_limit: s.scrape_limit as u32,
        }))
    }

    pub async fn find_by_api_key(&self, key: &str) -> Result<Option<ApiKeyScope>, sea_orm::DbErr> {
        let api_key = ApiKeyEntity::find()
            .filter(ApiKeyColumn::Key.eq(key))
            .one(&self.db)
            .await?;

        match api_key {
            Some(key) => self.find_by_api_key_id(key.id).await,
            None => Ok(None),
        }
    }

    pub async fn upsert(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, sea_orm::DbErr> {
        let existing = self.find_by_api_key_id(api_key_id).await?;

        match existing {
            Some(_) => {
                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        api_key_id: sea_orm::ActiveValue::Unchanged(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        ..Default::default()
                    };

                ScopeEntity::update(scope_active_model)
                    .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
                    .exec(&self.db)
                    .await?;

                Ok(scope)
            }
            None => {
                let scope_active_model =
                    crate::infrastructure::database::entities::auth::scope::ActiveModel {
                        id: Set(Uuid::new_v4()),
                        api_key_id: Set(api_key_id),
                        read: Set(scope.read),
                        write: Set(scope.write),
                        admin: Set(scope.admin),
                        search_limit: Set(scope.search_limit as i32),
                        scrape_limit: Set(scope.scrape_limit as i32),
                        ..Default::default()
                    };

                ScopeEntity::insert(scope_active_model)
                    .exec(&self.db)
                    .await?;
                Ok(scope)
            }
        }
    }

    pub async fn delete_by_api_key_id(&self, api_key_id: Uuid) -> Result<bool, sea_orm::DbErr> {
        let result = ScopeEntity::delete_many()
            .filter(ScopeColumn::ApiKeyId.eq(api_key_id))
            .exec(&self.db)
            .await?;

        Ok(result.rows_affected > 0)
    }
}
