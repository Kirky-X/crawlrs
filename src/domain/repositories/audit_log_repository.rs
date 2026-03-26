// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 审计日志仓储接口
//!
//! 定义了审计日志数据访问的抽象契约，遵循依赖倒置原则。
//! 具体实现由基础设施层提供。

use async_trait::async_trait;
use shaku::Interface;
use uuid::Uuid;

use crate::domain::auth::AuditLogEntry;

/// 仓储操作错误
#[derive(Debug, thiserror::Error)]
pub enum AuditRepositoryError {
    /// 数据库错误
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),

    /// 审计日志未找到
    #[error("Audit log not found")]
    NotFound,
}

/// 实现 From<dbnexus::config::DbError> trait，支持 ? 操作符自动转换
impl From<dbnexus::config::DbError> for AuditRepositoryError {
    fn from(err: dbnexus::config::DbError) -> Self {
        match err {
            dbnexus::config::DbError::Connection(db_err) => AuditRepositoryError::DatabaseError(db_err),
            dbnexus::config::DbError::Config(msg) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Configuration error: {}", msg)))
            }
            dbnexus::config::DbError::Permission(msg) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Permission denied: {}", msg)))
            }
            dbnexus::config::DbError::Transaction(msg) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Transaction error: {}", msg)))
            }
            dbnexus::config::DbError::Migration(msg) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Migration error: {}", msg)))
            }
        }
    }
}

/// 实现 From<dbnexus::error::DbError> trait，支持 ? 操作符自动转换
impl From<dbnexus::error::DbError> for AuditRepositoryError {
    fn from(err: dbnexus::error::DbError) -> Self {
        AuditRepositoryError::DatabaseError(err.inner().clone())
    }
}

/// 实现 From<dbnexus::error::DbNexusError> trait，支持 ? 操作符自动转换
impl From<dbnexus::error::DbNexusError> for AuditRepositoryError {
    fn from(err: dbnexus::error::DbNexusError) -> Self {
        match err {
            dbnexus::error::DbNexusError::Database(db_err) => db_err.into(),
            dbnexus::error::DbNexusError::Pool(pool_err) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Pool error: {}", pool_err)))
            }
            dbnexus::error::DbNexusError::Permission(perm_err) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Permission error: {}", perm_err)))
            }
            dbnexus::error::DbNexusError::Config(config_err) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Config error: {}", config_err)))
            }
            dbnexus::error::DbNexusError::Migration(mig_err) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Migration error: {}", mig_err)))
            }
            dbnexus::error::DbNexusError::Audit(audit_err) => {
                AuditRepositoryError::DatabaseError(sea_orm::DbErr::Custom(format!("Audit error: {}", audit_err)))
            }
        }
    }
}

/// 审计日志仓储接口
///
/// 定义了审计日志的创建、查询和删除操作。
/// 领域层依赖这个接口，而非具体实现。
#[async_trait]
pub trait AuditLogRepository: Interface + Send + Sync {
    /// 创建审计日志条目
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditRepositoryError>;

    /// 根据 API Key ID 查询审计日志
    async fn find_by_api_key_id(
        &self,
        api_key_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 根据团队 ID 查询审计日志
    async fn find_by_team_id(
        &self,
        team_id: Uuid,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 查询被拒绝的请求
    async fn find_denied_for_key(
        &self,
        api_key_id: Uuid,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 清理旧的审计日志
    async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditRepositoryError>;
}
