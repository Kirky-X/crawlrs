// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 并发控制接口
//!
//! 定义并发控制器的统一接口，提供任务并发管理功能

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 并发控制结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencyResult {
    /// 允许执行
    Allowed,
    /// 被拒绝（并发限制已达到）
    Denied { reason: String },
}

/// 并发控制器接口
///
/// 定义了并发控制的核心操作，包括信号量获取和释放。
/// 该 trait 设计为支持多种实现（本地、分布式等）。
#[async_trait]
pub trait ConcurrencyController: Send + Sync {
    /// 检查团队并发限制
    ///
    /// 检查给定团队是否可以在指定任务上获取并发槽位。
    /// 此方法不实际获取槽位，仅进行检查。
    ///
    /// # Arguments
    ///
    /// * `team_id` - 团队 ID
    /// * `task_id` - 任务 ID
    ///
    /// # Returns
    ///
    /// 如果可以获取槽位返回 `Ok(ConcurrencyResult::Allowed)`，
    /// 如果达到限制返回 `Ok(ConcurrencyResult::Denied)`，错误返回 `Err`
    async fn check_team_concurrency(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<ConcurrencyResult>;

    /// 获取信号量许可
    ///
    /// 尝试为指定任务获取信号量许可。如果成功获取，返回 `Ok(true)`；
    /// 如果已达到并发限制，返回 `Ok(false)`。
    ///
    /// # Arguments
    ///
    /// * `team_id` - 团队 ID
    /// * `task_id` - 任务 ID
    ///
    /// # Returns
    ///
    /// 如果获取成功返回 `Ok(true)`，如果达到限制返回 `Ok(false)`，错误返回 `Err`
    async fn acquire_semaphore(&self, team_id: Uuid, task_id: Uuid) -> Result<bool>;

    /// 释放信号量许可
    ///
    /// 释放之前获取的信号量许可，允许其他任务使用该槽位。
    ///
    /// # Arguments
    ///
    /// * `team_id` - 团队 ID
    /// * `task_id` - 任务 ID
    ///
    /// # Returns
    ///
    /// 成功返回 `Ok(())`，错误返回 `Err`
    async fn release_semaphore(&self, team_id: Uuid, task_id: Uuid) -> Result<()>;
}
