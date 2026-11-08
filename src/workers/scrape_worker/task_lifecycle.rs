// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `ScrapeWorker` 任务准入与结果提取辅助（从 `mod.rs` 拆分的 partial impl block）

use super::extraction_billing::{ExtractionSettlement, ExtractionSpec};
use super::ScrapeWorker;
use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::domain::models::Task;
use crate::domain::services::llm::TokenUsage;
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::{Admission, MemoryState};
use anyhow::Result;
use chrono::Utc;
use log::warn;
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;

impl ScrapeWorker {
    /// 任务过期检查（自 [`ScrapeWorker::process_task`] 提取）。
    ///
    /// 返回 `true` 表示任务已过期并按过期处理完毕（调用方直接返回 Ok(())）：
    /// 标记失败；已被其他 worker 终结/认领时不重复发失败 webhook。
    pub(super) async fn handle_expired_task(&self, task: &Task) -> Result<bool> {
        if let Some(expires_at) = task.expires_at {
            if Utc::now() > expires_at {
                warn!("Task {} expired at {}", task.id, expires_at);
                let marked = self
                    .repository
                    .mark_failed(task.id, Some(self.worker_id))
                    .await?;
                if marked == 0 {
                    // 已被其他 worker 终结/认领，不重复发失败 webhook
                    warn!(
                        "Task {} already terminal or re-claimed, skip expiry handling",
                        task.id
                    );
                    return Ok(true);
                }
                // Trigger failure webhook if needed
                self.trigger_webhook(task, Some("Task expired".to_string()))
                    .await;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 内存感知准入检查（自 [`ScrapeWorker::process_task`] 提取）
    ///
    /// 在获取并发许可（Team Semaphore）之前先采样内存状态并决定是否放行：
    /// - Normal → Proceed：进入并发获取流程
    /// - Pressure → Defer：复用现有 backlog 重排逻辑延后（
    ///   不新建持久队列，复用 scheduled_at + Queued 状态机）
    /// - Critical → Reschedule：同样经 backlog 重排，但语义为拒绝当前批次
    ///
    /// 采样策略：每次 process_task 调用 `update_state()` 读取最新内存使用率。
    /// 这与 `MemoryScheduler::spawn_monitor` 的后台 1s 采样互补——后台采样驱动
    /// 优雅关闭信号，此处 per-task 采样保证 admit 返回最新决策。
    ///
    /// F10 AIMD 内存联动：update_state 之后、admit 之前把水位广播给
    /// TeamSemaphore（Pressure 冻结 additive-increase / Critical CAS 半减），
    /// 消除「门控在暂停派发、AIMD 却在加并发」的矛盾。放在 admit match 之前
    /// 以覆盖全部状态；Normal 不发信号，AIMD 自然恢复 +1（无粘滞）。
    /// Defer/Reschedule 分支在 requeue 前不 acquire，信号先行不影响回队。
    ///
    /// 返回 `true` 表示任务已延后/重排（调用方直接返回 Ok(())）。
    #[cfg(feature = "metrics")]
    pub(super) async fn admit_task_by_memory(&self, task: &Task) -> Result<bool> {
        self.memory_scheduler.update_state();
        match self.memory_scheduler.state() {
            MemoryState::Pressure => {
                self.team_semaphore.on_memory_pressure(false);
            }
            MemoryState::Critical => {
                self.team_semaphore.on_memory_pressure(true);
            }
            MemoryState::Normal => {}
        }
        match self.memory_scheduler.admit().await {
            Admission::Proceed => Ok(false),
            Admission::Defer => {
                warn!(
                    "Memory pressure detected, deferring task {} (team_id={}) via backlog reschedule",
                    task.id, task.team_id
                );
                // 复用现有 backlog 重排逻辑：延后 30 秒重新入队
                // （条件回队：锁守卫防止覆盖并发发生的取消/终结）
                let scheduled_at = Utc::now() + chrono::Duration::seconds(30);
                self.repository
                    .requeue_task(task.id, task.lock_token, Some(scheduled_at))
                    .await?;
                Ok(true)
            }
            Admission::Reschedule => {
                warn!(
                    "Memory critical detected, rescheduling task {} (team_id={}) to backlog",
                    task.id, task.team_id
                );
                // Critical 同样复用 backlog 重排逻辑（不新建持久队列）
                let scheduled_at = Utc::now() + chrono::Duration::seconds(30);
                self.repository
                    .requeue_task(task.id, task.lock_token, Some(scheduled_at))
                    .await?;
                Ok(true)
            }
        }
    }

    /// 团队并发许可获取（自 [`ScrapeWorker::process_task`] 提取）
    ///
    /// 许可由调用方持有至任务处理结束（drop 自动释放）；返回 `None` 表示
    /// 团队并发已满、任务已按 Backlog 逻辑重排（调用方直接返回 Ok(())）。
    pub(super) async fn acquire_permit_or_reschedule(
        &self,
        task: &Task,
    ) -> Result<Option<OwnedSemaphorePermit>> {
        match self.acquire_concurrency_permit(task) {
            Some(p) => Ok(Some(p)),
            None => {
                warn!(
                    "Team {} concurrency limit exceeded, rescheduling task {}",
                    task.team_id, task.id
                );
                // Reschedule logic (Backlog)
                // Delay by 30 seconds（条件回队，锁守卫防覆盖并发状态迁移）
                let scheduled_at = Utc::now() + chrono::Duration::seconds(30);
                self.repository
                    .requeue_task(task.id, task.lock_token, Some(scheduled_at))
                    .await?;
                Ok(None)
            }
        }
    }

    /// 提取阶段（自 [`ScrapeWorker::handle_scrape_success`] 提取）：解析
    /// ScrapeRequest 以检查是否有提取规则，按优先级依次尝试
    /// extraction_rules → extraction_prompt（空串视为未设置）→ extraction_schema，
    /// 命中即经共享 [`ScrapeWorker::dispatch_extraction`] 执行（Deferred 结算：
    /// 只记录 token 用量，扣费延后至守卫胜利后）。返回
    /// (提取结果, 待守卫胜利后扣除的用量)。
    ///
    /// 提取失败仅 error 留痕不阻断（markdown 同策略），错误文案逐字保留。
    pub(super) async fn run_extraction_stage(
        &self,
        task: &Task,
        parsed_req: Option<&ScrapeRequestDto>,
        content: &str,
    ) -> (Option<Value>, Vec<TokenUsage>) {
        let mut pending_token_usages: Vec<TokenUsage> = Vec::new();
        let mut extracted_data = None;
        if let Some(req) = parsed_req {
            let spec = if let Some(rules) = &req.extraction_rules {
                Some(ExtractionSpec::Rules(rules))
            } else if let Some(prompt) = &req.extraction_prompt {
                // extraction_prompt 为空字符串时视为未设置
                (!prompt.is_empty()).then_some(ExtractionSpec::Prompt(prompt.as_str()))
            } else {
                req.extraction_schema.as_ref().map(ExtractionSpec::Schema)
            };

            if let Some(spec) = spec {
                match self
                    .dispatch_extraction(
                        task,
                        content,
                        &task.url,
                        &spec,
                        ExtractionSettlement::Deferred {
                            usage_acc: &mut pending_token_usages,
                        },
                    )
                    .await
                {
                    Ok(data) => extracted_data = Some(data),
                    Err(e) => spec.log_failure(&task.url, &e),
                }
            }
        }
        (extracted_data, pending_token_usages)
    }
}
