// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::auth::ApiKeyScope;
use async_trait::async_trait;
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
            DbError::Config(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Config: {}", msg)))
            }
            DbError::Permission(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Permission: {}", msg)))
            }
            DbError::Transaction(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Transaction: {}", msg)))
            }
            DbError::Migration(msg) => {
                RepositoryError::Database(sea_orm::DbErr::Custom(format!("Migration: {}", msg)))
            }
        }
    }
}

/// 旧作用域仓库 trait（已弃用）。
///
/// # 弃用说明（R-key-lifecycle-003 / T028）
///
/// garrison-auth-migration 变更后，权限/作用域管理由 garrison RBAC 接管：
/// - 旧 `scopes` 表的读写路径不再被认证/签发调用
/// - `AuthScopeRepositoryImpl` 仅供历史数据迁移/查询保留，不参与热路径
/// - 权限串改为 `crawlrs:read`/`crawlrs:write`/`crawlrs:admin`，由 garrison
///   `CrawlrsGarrisonInterface::get_permission_list` 返回，经
///   `auth_bridge::map_perms_to_scope` 映射为 `ApiKeyScope`
///
/// # 后续清理
///
/// 待 `tools/reissue_api_keys.rs` 完成全量重签后，旧 `scopes` 表数据可清理，
/// 届时移除此 trait 及其实现。当前保留供运维核对。
#[deprecated(
    since = "0.2.0",
    note = "garrison RBAC 接管权限管理；使用 auth_bridge::map_perms_to_scope 替代"
)]
#[async_trait]
pub trait AuthScopeRepository: Send + Sync {
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
