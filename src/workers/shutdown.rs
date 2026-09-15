// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 优雅退出协调器
//!
//! 提供 worker service 的统一关闭编排：
//! - `ShutdownCoordinator`：共享的关闭 flag + 完成通知，供 worker 循环轮询；
//!   内嵌 trait-kit `AsyncShutdownCoordinator` 承载分阶段停机 hook
//!   （StopRequests → DrainQueue → CloseConnections，每阶段独立超时）
//! - `listen_unix_signals`：监听 SIGTERM/SIGINT 并触发关闭
//!
//! 设计接收信号 → 设置 `AtomicBool` flag → 等待活跃任务完成
//! （graceful period 30s，可配置）→ 按阶段执行注册的停机 hook
//! （如 in-flight 任务回滚）→ 强制退出。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

use crate::domain::repositories::task_repository::TaskRepository;
use log::{error, info};
use trait_kit::kit::shutdown::AsyncShutdownCoordinator;
use uuid::Uuid;

/// 分阶段停机阶段（re-export 自研 trait-kit `shutdown` kit）：
/// StopRequests → DrainQueue → CloseConnections。
pub use trait_kit::kit::shutdown::ShutdownPhase;

/// 默认优雅退出门限（秒）。
pub const DEFAULT_GRACEFUL_PERIOD_SECS: u64 = 30;

/// 优雅退出协调器。
///
/// 通过 `Arc` 共享给所有 worker 与信号监听任务。
pub struct ShutdownCoordinator {
    /// 关闭 flag：置位后 worker 停止接受新任务。
    flag: AtomicBool,
    /// 关闭/完成通知：`trigger()` 唤醒等待方。
    notify: Arc<Notify>,
    /// 等待活跃任务完成的宽限时长。
    graceful_period: Duration,
    /// 分阶段停机 hook 执行器（吸收自研 trait-kit `shutdown` kit）：
    /// 全局超时取 `graceful_period`，阶段间按序执行，超时标记不 panic。
    phased: AsyncShutdownCoordinator,
}

impl ShutdownCoordinator {
    /// 创建协调器。
    pub fn new(graceful_period: Duration) -> Self {
        let phased = AsyncShutdownCoordinator::new();
        // 全局超时与宽限期对齐：阶段间软限制，超预算跳过剩余 hook 并标记 timed_out
        let _ = phased.set_global_timeout(graceful_period);
        Self {
            flag: AtomicBool::new(false),
            notify: Arc::new(Notify::new()),
            graceful_period,
            phased,
        }
    }

    /// 注册分阶段停机 hook。
    ///
    /// hook 在 `run_phased_shutdown()` 中按
    /// StopRequests → DrainQueue → CloseConnections 顺序执行；
    /// 注册失败（内部锁中毒，不可恢复）仅记录 error。
    pub fn register_shutdown_hook<F>(&self, phase: ShutdownPhase, hook: F)
    where
        F: FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
            + Send
            + Sync
            + 'static,
    {
        if let Err(e) = self.phased.register_hook(phase, hook) {
            error!("Failed to register shutdown hook: {}", e);
        }
    }

    /// 执行分阶段停机 hook（信号触发、worker 排空之后调用）。
    ///
    /// 返回所有阶段是否成功（无超时、无失败）。
    pub async fn run_phased_shutdown(&self) -> bool {
        match self.phased.shutdown().await {
            Ok(result) => {
                let timed_out = result.timed_out_phases();
                if !timed_out.is_empty() {
                    let names = timed_out
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    error!("Phased shutdown timed out in phase(s): {}", names);
                }
                let ok = result.is_ok();
                info!("Phased shutdown complete (ok={}, phases executed)", ok);
                ok
            }
            Err(e) => {
                error!("Phased shutdown failed: {}", e);
                false
            }
        }
    }

    /// 使用默认宽限期（30s）创建协调器。
    pub fn with_defaults() -> Self {
        Self::new(Duration::from_secs(DEFAULT_GRACEFUL_PERIOD_SECS))
    }

    /// 触发关闭：置位 flag 并唤醒所有 `wait_for_completion` 等待方。
    pub fn trigger(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// 查询是否进入关闭流程。
    pub fn is_shutting_down(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 获取配置的宽限期。
    pub fn graceful_period(&self) -> Duration {
        self.graceful_period
    }

    /// 等待活跃任务完成。
    ///
    /// - 若 `trigger()` 已触发（信号到达），立即返回 `true`（已收到完成通知）。
    /// - 若在 `graceful_period` 内未触发，返回 `false`（超时，应强制退出）。
    ///
    /// 用于替代裸 `tokio::signal::ctrl_c().await`：信号监听任务一旦触发，
    /// 本方法即刻返回；否则最迟在宽限期后返回。
    pub async fn wait_for_completion(&self) -> bool {
        // 若已在关闭中，直接返回（信号可能先于本调用到达）。
        if self.is_shutting_down() {
            return true;
        }
        tokio::select! {
            _ = self.notify.notified() => true,
            _ = tokio::time::sleep(self.graceful_period) => false,
        }
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 监听 SIGTERM/SIGINT 并在任一信号到达时触发关闭（Unix 平台）。
///
/// # 说明
///
/// 在非 Unix 平台（Windows）降级为监听 Ctrl+C（`ctrl_c()`），保证编译通过，
/// 运行时行为与 Unix 一致。
#[cfg(unix)]
pub async fn listen_unix_signals(coordinator: Arc<ShutdownCoordinator>) -> std::io::Result<()> {
    use log::info;
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => {
            info!("SIGTERM received, initiating graceful shutdown");
        }
        _ = sigint.recv() => {
            info!("SIGINT received, initiating graceful shutdown");
        }
    }

    coordinator.trigger();
    Ok(())
}

/// 监听关闭信号的降级实现（非 Unix 平台）。
#[cfg(not(unix))]
pub async fn listen_unix_signals(coordinator: Arc<ShutdownCoordinator>) -> std::io::Result<()> {
    use log::info;
    tokio::signal::ctrl_c().await?;
    info!("Ctrl+C received, initiating graceful shutdown");
    coordinator.trigger();
    Ok(())
}

/// 回滚已锁定但未完成的任务状态。
///
/// 优雅退出期间，`acquire_next` 已锁定（`Active`）但未完成的任务会永久卡在
/// 执行态；本函数将其批量重置回 `Queued`（待处理），避免任务丢失。
///
/// 语义上等价于 `TaskRepository::update_status(task_id, Pending)`：
/// - 本代码库任务状态为 `TaskStatus::Queued`（待处理）/ `TaskStatus::Active`（处理中）；
/// - 复用 `reset_stuck_tasks_for_workers(timeout=0)` 的批量 UPDATE（Active → Queued），
///   立即重置所有已锁定任务，无需 N+1 循环。
///
/// 身份限定（R-data-integrity-004）：仅回滚 `worker_ids` 中本进程 worker
/// 认领的任务（lock_token == worker_id），多副本部署下不触碰其他副本在跑任务。
///
/// 该操作是 best-effort：以 `graceful_period` 为超时上限，数据库不可达时
/// 记录 error 后由调用方继续强制退出。
pub async fn rollback_pending_tasks(
    repository: &Arc<dyn TaskRepository>,
    graceful_period: Duration,
    worker_ids: &[Uuid],
) {
    match tokio::time::timeout(
        graceful_period,
        repository.reset_stuck_tasks_for_workers(chrono::Duration::zero(), worker_ids),
    )
    .await
    {
        Ok(Ok(affected)) => {
            info!(
                "Rolled back {} in-flight tasks to queued during shutdown (workers: {})",
                affected,
                worker_ids.len()
            );
        }
        Ok(Err(e)) => {
            error!("Failed to roll back in-flight tasks during shutdown: {}", e);
        }
        Err(_) => {
            error!(
                "Roll back of in-flight tasks timed out after {:?}",
                graceful_period
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[test]
    fn test_initial_state_not_shutting_down() {
        let coordinator = ShutdownCoordinator::with_defaults();
        assert!(!coordinator.is_shutting_down());
    }

    #[test]
    fn test_trigger_sets_shutting_down() {
        let coordinator = ShutdownCoordinator::with_defaults();
        coordinator.trigger();
        assert!(coordinator.is_shutting_down());
    }

    #[test]
    fn test_default_graceful_period_is_30s() {
        let coordinator = ShutdownCoordinator::with_defaults();
        assert_eq!(coordinator.graceful_period(), Duration::from_secs(30));
    }

    #[test]
    fn test_custom_graceful_period() {
        let coordinator = ShutdownCoordinator::new(Duration::from_millis(100));
        assert_eq!(coordinator.graceful_period(), Duration::from_millis(100));
        assert!(!coordinator.is_shutting_down());
    }

    #[tokio::test]
    async fn test_trigger_before_wait_returns_immediately() {
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_secs(60)));
        coordinator.trigger();
        let result = coordinator.wait_for_completion().await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_wait_for_completion_times_out_after_graceful_period() {
        // 无 trigger：wait_for_completion 应在宽限期后超时返回 false。
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_millis(50)));
        let start = std::time::Instant::now();
        let result = coordinator.wait_for_completion().await;
        let elapsed = start.elapsed();
        assert!(!result, "timeout should return false");
        assert!(
            elapsed >= Duration::from_millis(40),
            "timeout should wait at least ~graceful_period, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_wait_for_completion_unblocks_on_trigger() {
        // 后台任务触发 trigger，wait_for_completion 应提前返回 true（而非等满宽限期）。
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_secs(30)));
        let coord_for_task = coordinator.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            coord_for_task.trigger();
        });
        let start = std::time::Instant::now();
        let result = coordinator.wait_for_completion().await;
        let elapsed = start.elapsed();
        assert!(result);
        assert!(
            elapsed < Duration::from_secs(5),
            "should unblock promptly on trigger, got {:?}",
            elapsed
        );
    }

    // ========== 分阶段停机 hook（trait-kit shutdown 吸收） ==========

    #[tokio::test]
    async fn test_phased_shutdown_runs_registered_hook() {
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_secs(30)));
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter_in_hook = counter.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::DrainQueue, move || {
            let counter = counter_in_hook.clone();
            Box::pin(async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        });

        assert!(coordinator.run_phased_shutdown().await);
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "registered DrainQueue hook must execute exactly once"
        );
    }

    #[tokio::test]
    async fn test_phased_shutdown_executes_phases_in_order() {
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_secs(30)));
        let order = Arc::new(parking_lot::Mutex::<Vec<&'static str>>::new(Vec::new()));

        // 故意按逆序注册，验证执行顺序由阶段决定而非注册顺序
        let o_close = order.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::CloseConnections, move || {
            let o = o_close.clone();
            Box::pin(async move { o.lock().push("close") })
        });
        let o_stop = order.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::StopRequests, move || {
            let o = o_stop.clone();
            Box::pin(async move { o.lock().push("stop") })
        });
        let o_drain = order.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::DrainQueue, move || {
            let o = o_drain.clone();
            Box::pin(async move { o.lock().push("drain") })
        });

        assert!(coordinator.run_phased_shutdown().await);
        assert_eq!(
            *order.lock(),
            vec!["stop", "drain", "close"],
            "hooks must run in phase order regardless of registration order"
        );
    }

    #[tokio::test]
    async fn test_phased_shutdown_skips_remaining_hooks_on_global_timeout() {
        // 全局超时 = graceful_period（50ms）；首个 hook 睡 100ms 耗尽预算后，
        // 同阶段剩余 hook 应被跳过且阶段标记 timed_out（返回 false）。
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_millis(50)));
        let slow_ran = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let skipped_ran = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let slow_flag = slow_ran.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::DrainQueue, move || {
            let flag = slow_flag.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                flag.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        });
        let skipped_flag = skipped_ran.clone();
        coordinator.register_shutdown_hook(ShutdownPhase::DrainQueue, move || {
            let flag = skipped_flag.clone();
            Box::pin(async move {
                flag.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        });

        assert!(
            !coordinator.run_phased_shutdown().await,
            "budget-exceeding shutdown must report not-ok"
        );
        assert_eq!(slow_ran.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            skipped_ran.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "hook after global budget exhaustion must be skipped"
        );
    }

    // ========== rollback_pending_tasks integration tests ==========

    use crate::domain::models::{Task, TaskType};
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use std::collections::HashSet;
    use uuid::Uuid;

    /// 极简 mock：仅记录 `reset_stuck_tasks` 调用次数与最近一次 timeout。
    struct RecordingTaskRepository {
        reset_calls: std::sync::atomic::AtomicU32,
        last_timeout: parking_lot::Mutex<Option<chrono::Duration>>,
    }

    impl RecordingTaskRepository {
        fn new() -> Self {
            Self {
                reset_calls: std::sync::atomic::AtomicU32::new(0),
                last_timeout: parking_lot::Mutex::new(None),
            }
        }

        fn reset_calls(&self) -> u32 {
            self.reset_calls.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn last_timeout(&self) -> Option<chrono::Duration> {
            *self.last_timeout.lock()
        }
    }

    #[async_trait]
    impl TaskRepository for RecordingTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            self.reset_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.last_timeout.lock() = Some(timeout);
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(Vec::new())
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((Vec::new(), 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_rollback_pending_tasks_invokes_reset_with_zero_timeout() {
        let concrete = Arc::new(RecordingTaskRepository::new());
        let repo: Arc<dyn TaskRepository> = concrete.clone();
        rollback_pending_tasks(&repo, Duration::from_millis(100), &[]).await;

        assert_eq!(
            concrete.reset_calls(),
            1,
            "rollback must invoke reset_stuck_tasks once"
        );
        assert_eq!(
            concrete.last_timeout(),
            Some(chrono::Duration::zero()),
            "rollback must reset ALL in-flight tasks immediately (timeout=0)"
        );
    }

    #[tokio::test]
    async fn test_shutdown_flow_trigger_rollback_order() {
        // 主流程编排：trigger → wait_for_completion 返回 → rollback。
        let coordinator = Arc::new(ShutdownCoordinator::new(Duration::from_millis(100)));
        let repo: Arc<dyn TaskRepository> = Arc::new(RecordingTaskRepository::new());
        let coord_in_task = coordinator.clone();
        let worker_task = tokio::spawn(async move {
            coord_in_task.wait_for_completion().await;
            rollback_pending_tasks(&repo, Duration::from_millis(100), &[]).await;
        });

        // 模拟外部 SIGTERM：先让 worker 进入等待，再触发。
        tokio::time::sleep(Duration::from_millis(20)).await;
        coordinator.trigger();

        let _ = tokio::time::timeout(Duration::from_secs(5), worker_task)
            .await
            .expect("worker shutdown flow should complete within timeout");
        assert!(
            coordinator.is_shutting_down(),
            "coordinator should be shutting down"
        );
    }

    #[test]
    fn test_task_type_enum_external_reference() {
        // 仅验证 TaskType 可被引用（编译期断言），保证模块编译不依赖未用依赖。
        let _ = TaskType::Scrape;
    }

    // ========== rollback error/timeout paths ==========

    struct ErrorTaskRepository;

    #[async_trait]
    impl TaskRepository for ErrorTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Err(RepositoryError::NotFound)
        }
        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!(
                "db connection lost"
            )))
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }

        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct SlowTaskRepository;

    #[async_trait]
    impl TaskRepository for SlowTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Ok(Task::new(
                Uuid::new_v4(),
                TaskType::Scrape,
                Uuid::new_v4(),
                Uuid::new_v4(),
                String::new(),
                serde_json::json!({}),
            ))
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            Ok(Task::new(
                Uuid::new_v4(),
                TaskType::Scrape,
                Uuid::new_v4(),
                Uuid::new_v4(),
                String::new(),
                serde_json::json!({}),
            ))
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_failed(
            &self,
            _id: Uuid,
            _lock_token: Option<Uuid>,
        ) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<u64, RepositoryError> {
            Ok(1)
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            // 模拟超时的数据库操作
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }

        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_rollback_pending_tasks_handles_repository_error() {
        let repo: Arc<dyn TaskRepository> = Arc::new(ErrorTaskRepository);
        // 不应 panic，应优雅处理错误
        rollback_pending_tasks(&repo, Duration::from_secs(1), &[]).await;
    }

    #[tokio::test]
    async fn test_rollback_pending_tasks_handles_timeout() {
        let repo: Arc<dyn TaskRepository> = Arc::new(SlowTaskRepository);
        // 10ms 超时，SlowTaskRepository 需要 10s
        rollback_pending_tasks(&repo, Duration::from_millis(10), &[]).await;
    }
}
