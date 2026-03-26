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

/// 实现 From<dbnexus::config::DbError> trait，支持 ? 操作符自动转换
impl From<dbnexus::config::DbError> for RepositoryError {
    fn from(err: dbnexus::config::DbError) -> Self {
        match err {
            dbnexus::config::DbError::Connection(db_err) => RepositoryError::Database(db_err),
            dbnexus::config::DbError::Config(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Configuration error: {}", msg)))
            }
            dbnexus::config::DbError::Permission(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Permission denied: {}", msg)))
            }
            dbnexus::config::DbError::Transaction(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Transaction error: {}", msg)))
            }
            dbnexus::config::DbError::Migration(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Migration error: {}", msg)))
            }
        }
    }
}

/// 实现 From<dbnexus::error::DbError> trait，支持 ? 操作符自动转换
impl From<dbnexus::error::DbError> for RepositoryError {
    fn from(err: dbnexus::error::DbError) -> Self {
        RepositoryError::Database(err.inner().clone())
    }
}

/// 实现 From<dbnexus::error::DbNexusError> trait，支持 ? 操作符自动转换
impl From<dbnexus::error::DbNexusError> for RepositoryError {
    fn from(err: dbnexus::error::DbNexusError) -> Self {
        match err {
            dbnexus::error::DbNexusError::Database(db_err) => db_err.into(),
            dbnexus::error::DbNexusError::Pool(pool_err) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Pool error: {}", pool_err)))
            }
            dbnexus::error::DbNexusError::Permission(perm_err) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Permission error: {}", perm_err)))
            }
            dbnexus::error::DbNexusError::Config(config_err) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Config error: {}", config_err)))
            }
            dbnexus::error::DbNexusError::Migration(mig_err) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Migration error: {}", mig_err)))
            }
            dbnexus::error::DbNexusError::Audit(audit_err) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Audit error: {}", audit_err)))
            }
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
