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

    #[error("Not found: {0}")]
    NotFound(String),
}

/// 实现 From<dbnexus::DbError> trait，支持 ? 操作符自动转换
impl From<dbnexus::DbError> for RepositoryError {
    fn from(err: dbnexus::DbError) -> Self {
        use dbnexus::DbError;
        match err {
            DbError::Connection(db_err) => RepositoryError::Database(db_err),
            DbError::Config(msg) => RepositoryError::Database(sea_orm::DbErr::Custom(format!("Config: {}", msg))),
            DbError::Permission(msg) => RepositoryError::Database(sea_orm::DbErr::Custom(format!("Permission: {}", msg))),
            DbError::Transaction(msg) => RepositoryError::Database(sea_orm::DbErr::Custom(format!("Transaction: {}", msg))),
            DbError::Migration(msg) => RepositoryError::Database(sea_orm::DbErr::Custom(format!("Migration: {}", msg))),
        }
    }
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
