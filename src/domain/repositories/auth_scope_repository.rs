// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::auth::ApiKeyScope;
use async_trait::async_trait;
use shaku::Interface;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),
}

#[async_trait]
pub trait AuthScopeRepository: Interface + Send + Sync {
    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyScope>, RepositoryError>;
    async fn find_by_api_key(&self, key: &str) -> Result<Option<ApiKeyScope>, RepositoryError>;
    async fn upsert(
        &self,
        api_key_id: Uuid,
        scope: ApiKeyScope,
    ) -> Result<ApiKeyScope, RepositoryError>;
    async fn delete_by_api_key_id(&self, api_key_id: Uuid) -> Result<bool, RepositoryError>;
}
