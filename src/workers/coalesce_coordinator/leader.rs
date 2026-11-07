// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! leader 信号等待与结果收养（从 `coalesce_coordinator.rs` 拆分的 partial impl block）

use super::CoalesceCoordinator;
use crate::domain::models::Task;
use crate::utils::coalesce::CoalesceSignal;
use anyhow::Result;
use chrono::Utc;
use log::debug;
use tokio::sync::broadcast;

impl CoalesceCoordinator {
    /// Completed 信号处理（自 [`CoalesceCoordinator::try_coalesce`] 提取）：
    /// 按 leader task_id 查询落库结果，命中则复制给等待方并 mark_completed。
    ///
    /// 返回 `false` 表示 leader 已完成但结果尚未落库/不可见——调用方延后重排；
    /// `Err` 为仓储错误（错误文案逐字保留）。
    pub(super) async fn adopt_leader_result(
        &self,
        task: &Task,
        leader_task_id: crate::domain::id::Id,
        worker_id: crate::domain::id::Id,
    ) -> Result<bool> {
        match self.result_repository.find_by_task_id(leader_task_id).await {
            Ok(Some(mut leader_result)) => {
                debug!(
                    "Coalesced task {} resolved from leader {} result, marking completed",
                    task.id, leader_task_id
                );
                // 结果须为等待方复制一份：等待方完成后 API 按
                // 自身 task_id 查结果，缺失即 completed 任务
                // content 空、status_code 缺失（静默失败）。
                // 重复投递时等待方可能已有结果，幂等跳过。
                if self
                    .result_repository
                    .find_by_task_id(task.id)
                    .await?
                    .is_none()
                {
                    leader_result.id = crate::domain::id::Id::generate();
                    leader_result.task_id = task.id;
                    leader_result.created_at = Utc::now();
                    self.result_repository.save(leader_result).await?;
                }
                self.repository
                    .mark_completed(task.id, Some(worker_id))
                    .await?;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(anyhow::anyhow!("coalesce find_by_task_id failed: {}", e)),
        }
    }
}

/// `rx.recv()` 带超时的结果分类（自 [`CoalesceCoordinator::try_coalesce`] 提取）
pub(super) enum LeaderSignal {
    /// 收到 leader 广播的正常信号（Completed / Purged）
    Signal(CoalesceSignal),
    /// 等待超时：leader 仍可能在跑
    TimedOut,
    /// leader 的 sender 全部 drop 却未广播完成（panic / 异常终止）
    Closed,
    /// 等待方消费过慢丢失信号
    Lagged(u64),
}

/// 带超时等待 leader 广播信号（自 [`CoalesceCoordinator::try_coalesce`] 提取）
pub(super) async fn recv_leader_signal(
    rx: &mut broadcast::Receiver<CoalesceSignal>,
    timeout_secs: u64,
) -> LeaderSignal {
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx.recv()).await {
        Err(_) => LeaderSignal::TimedOut,
        Ok(Ok(sig)) => LeaderSignal::Signal(sig),
        Ok(Err(broadcast::error::RecvError::Closed)) => LeaderSignal::Closed,
        Ok(Err(broadcast::error::RecvError::Lagged(n))) => LeaderSignal::Lagged(n),
    }
}
