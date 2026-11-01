// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 审计日志仓储接口
//!
//! 定义了审计日志数据访问的抽象契约，遵循依赖倒置原则。
//! 具体实现由基础设施层提供。

use async_trait::async_trait;

use crate::domain::auth::AuditLogEntry;

/// 仓储操作错误（领域层契约；`VersionConflict` 携带存储表名与行 id 以便上层定位）
#[derive(Debug, thiserror::Error)]
pub enum AuditRepositoryError {
    /// 数据库或连接层错误
    #[error("Database error: {0}")]
    DatabaseError(anyhow::Error),

    /// 数据库限流拒绝；保留建议重试秒数供仓储调用方程序化处理
    #[error("Database error: Rate limited (retry_after={retry_after_secs:?})")]
    RateLimited {
        /// 建议等待时间（秒）；`None` = 限流后端未提供
        retry_after_secs: Option<u64>,
    },

    /// 乐观锁版本冲突；保留表名与行 id 供仓储调用方程序化处理
    #[error("Database error: Version conflict on table '{table}' for id {id}")]
    VersionConflict {
        /// 冲突的表名
        table: String,
        /// 冲突的行 id
        id: i64,
    },

    /// 审计日志未找到
    #[error("Audit log not found")]
    NotFound,
}

/// 审计日志仓储接口
///
/// 定义了审计日志的创建、查询和删除操作。
/// 领域层依赖这个接口，而非具体实现。
#[async_trait]
pub trait AuditLogRepository: Send + Sync {
    /// 创建审计日志条目
    async fn create(&self, entry: &AuditLogEntry) -> Result<AuditLogEntry, AuditRepositoryError>;

    /// 根据 API Key ID 查询审计日志
    async fn find_by_api_key_id(
        &self,
        api_key_id: crate::domain::id::Id,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 根据团队 ID 查询审计日志
    async fn find_by_team_id(
        &self,
        team_id: crate::domain::id::Id,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 查询被拒绝的请求
    async fn find_denied_for_key(
        &self,
        api_key_id: crate::domain::id::Id,
        limit: u64,
    ) -> Result<Vec<AuditLogEntry>, AuditRepositoryError>;

    /// 清理旧的审计日志
    async fn cleanup_old_logs(&self, retention_days: i64) -> Result<u64, AuditRepositoryError>;
}
