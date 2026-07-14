// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Team 仓库接口
//!
//! 定义团队数据的持久化操作契约。
//! 具体实现由基础设施层提供。

use super::task_repository::RepositoryError;
use crate::domain::models::Team;
use async_trait::async_trait;
use uuid::Uuid;

/// 团队仓库特质
///
/// 定义团队 CRUD 操作的数据访问接口。
/// 与 `GeoRestrictionRepository` 互补——后者管理地理限制配置，
/// 本接口管理团队基本信息。
#[async_trait]
pub trait TeamRepository: Send + Sync {
    /// 创建新团队
    ///
    /// # 参数
    /// * `team` - 待创建的团队实体
    ///
    /// # 返回值
    /// * `Ok(Team)` - 创建成功后的团队（含数据库生成的时间戳等）
    /// * `Err(RepositoryError)` - 创建失败
    async fn create(&self, team: &Team) -> Result<Team, RepositoryError>;

    /// 根据 ID 查找团队
    ///
    /// # 参数
    /// * `id` - 团队 ID
    ///
    /// # 返回值
    /// * `Ok(Some(Team))` - 找到团队
    /// * `Ok(None)` - 团队不存在
    /// * `Err(RepositoryError)` - 查询失败
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Team>, RepositoryError>;
}
