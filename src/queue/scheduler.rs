// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::queue::task_queue::QueueError;
use chrono::{DateTime, Duration, Utc};
use log::{error, info};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration as TokioDuration};

/// 任务调度器
pub struct TaskScheduler<R: TaskRepository + Send + Sync + 'static> {
    /// 任务仓库
    repository: Arc<R>,
}

impl<R: TaskRepository + Send + Sync + 'static> TaskScheduler<R> {
    /// 创建新的任务调度器实例
    ///
    /// # 参数
    ///
    /// * `repository` - 任务仓库
    ///
    /// # 返回值
    ///
    /// 返回新的任务调度器实例
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    /// 启动调度器后台任务
    ///
    /// 这里的调度器目前主要负责清理过期任务或处理卡住的任务等维护工作
    /// 实际的任务调度（获取任务）由Worker通过acquire_next主动拉取
    ///
    /// # 返回值
    ///
    /// 返回后台任务的句柄
    pub fn start(&self) -> JoinHandle<()> {
        let repository = self.repository.clone();

        tokio::spawn(async move {
            let mut interval = interval(TokioDuration::from_secs(60)); // 每分钟检查一次

            loop {
                interval.tick().await;

                // 这里可以添加定期维护逻辑
                // 例如：重置长时间处于Active状态但未更新心跳的任务
                // 目前TaskRepository接口还没有暴露相关方法，这里作为预留扩展点
                match repository.reset_stuck_tasks(Duration::minutes(30)).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Reset {} stuck tasks", count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to reset stuck tasks: {}", e);
                    }
                }

                match repository.expire_tasks().await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Expired {} tasks", count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to expire tasks: {}", e);
                    }
                }

                info!("Scheduler maintenance tick");
            }
        })
    }

    /// 在特定时间调度任务执行
    ///
    /// # 参数
    ///
    /// * `task` - 要调度的任务
    /// * `time` - 执行时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度成功的任务
    /// * `Err(QueueError)` - 调度失败
    pub async fn schedule_at(
        &self,
        mut task: Task,
        time: DateTime<Utc>,
    ) -> Result<Task, QueueError> {
        task.scheduled_at = Some(time);
        task.status = TaskStatus::Queued;

        // Ensure created_at is set if not already (Task::new sets it, but good to be safe)
        if task.created_at.timestamp() == 0 {
            task.created_at = Utc::now();
        }

        let created = self.repository.create(&task).await?;
        Ok(created)
    }

    /// 在一段时间后调度任务执行
    ///
    /// # 参数
    ///
    /// * `task` - 要调度的任务
    /// * `duration` - 延迟时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度成功的任务
    /// * `Err(QueueError)` - 调度失败
    pub async fn schedule_in(&self, task: Task, duration: Duration) -> Result<Task, QueueError> {
        let time = Utc::now() + duration;
        self.schedule_at(task, time).await
    }

    /// 重新调度失败的任务进行重试，支持指数退避或固定延迟
    ///
    /// # 参数
    ///
    /// * `task` - 需要重试的任务
    /// * `delay` - 重试延迟时间
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 重调度后的任务
    /// * `Err(QueueError)` - 重调度失败
    pub async fn reschedule_retry(
        &self,
        mut task: Task,
        delay: Duration,
    ) -> Result<Task, QueueError> {
        if !task.can_retry() {
            // If cannot retry, mark as failed permanently
            task.status = TaskStatus::Failed;
            task.completed_at = Some(Utc::now());
            let updated = self.repository.update(&task).await?;
            return Ok(updated);
        }

        task.status = TaskStatus::Queued;
        task.attempt_count += 1;
        task.scheduled_at = Some(Utc::now() + delay);
        task.started_at = None; // Reset started_at as it's queued again
        task.completed_at = None;

        let updated = self.repository.update(&task).await?;
        Ok(updated)
    }

    /// 以高优先级调度任务
    ///
    /// # 参数
    ///
    /// * `task` - 需要调度的任务
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 调度后的任务
    /// * `Err(QueueError)` - 调度失败
    ///
    /// # 说明
    ///
    /// 将任务优先级设置为100（高优先级），并立即调度执行
    pub async fn schedule_urgent(&self, mut task: Task) -> Result<Task, QueueError> {
        task.priority = 100; // Assuming 100 is high priority
        task.scheduled_at = Some(Utc::now()); // Immediate
        task.status = TaskStatus::Queued;

        let created = self.repository.create(&task).await?;
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::task_domain::{TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Mock TaskRepository with configurable behavior and call counters
    struct MockTaskRepository {
        /// Whether create() should fail
        create_should_fail: bool,
        /// Whether update() should fail
        update_should_fail: bool,
        /// Whether reset_stuck_tasks() should fail
        reset_stuck_should_fail: bool,
        /// Whether expire_tasks() should fail
        expire_should_fail: bool,
        /// reset_stuck_tasks return value
        reset_stuck_count: u64,
        /// expire_tasks return value
        expire_count: u64,
        /// Call counters
        create_calls: Mutex<u32>,
        update_calls: Mutex<u32>,
        reset_stuck_calls: Mutex<u32>,
        expire_calls: Mutex<u32>,
        /// Last task passed to create()
        last_created_task: Mutex<Option<Task>>,
        /// Last task passed to update()
        last_updated_task: Mutex<Option<Task>>,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                create_should_fail: false,
                update_should_fail: false,
                reset_stuck_should_fail: false,
                expire_should_fail: false,
                reset_stuck_count: 0,
                expire_count: 0,
                create_calls: Mutex::new(0),
                update_calls: Mutex::new(0),
                reset_stuck_calls: Mutex::new(0),
                expire_calls: Mutex::new(0),
                last_created_task: Mutex::new(None),
                last_updated_task: Mutex::new(None),
            }
        }
        fn with_failing_create() -> Self {
            let mut me = Self::new();
            me.create_should_fail = true;
            me
        }
        fn with_failing_update() -> Self {
            let mut me = Self::new();
            me.update_should_fail = true;
            me
        }
        fn with_failing_reset_stuck() -> Self {
            let mut me = Self::new();
            me.reset_stuck_should_fail = true;
            me
        }
        fn with_failing_expire() -> Self {
            let mut me = Self::new();
            me.expire_should_fail = true;
            me
        }
        fn with_counts(reset_stuck_count: u64, expire_count: u64) -> Self {
            let mut me = Self::new();
            me.reset_stuck_count = reset_stuck_count;
            me.expire_count = expire_count;
            me
        }
        fn create_call_count(&self) -> u32 {
            *self.create_calls.lock().unwrap()
        }
        fn update_call_count(&self) -> u32 {
            *self.update_calls.lock().unwrap()
        }
        fn reset_stuck_call_count(&self) -> u32 {
            *self.reset_stuck_calls.lock().unwrap()
        }
        fn expire_call_count(&self) -> u32 {
            *self.expire_calls.lock().unwrap()
        }
        fn last_created_task(&self) -> Option<Task> {
            self.last_created_task.lock().unwrap().clone()
        }
        fn last_updated_task(&self) -> Option<Task> {
            self.last_updated_task.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            *self.create_calls.lock().unwrap() += 1;
            *self.last_created_task.lock().unwrap() = Some(task.clone());
            if self.create_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock create failed"
                )));
            }
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            *self.update_calls.lock().unwrap() += 1;
            *self.last_updated_task.lock().unwrap() = Some(task.clone());
            if self.update_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock update failed"
                )));
            }
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
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
        async fn reset_stuck_tasks(&self, _timeout: Duration) -> Result<u64, RepositoryError> {
            *self.reset_stuck_calls.lock().unwrap() += 1;
            if self.reset_stuck_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock reset_stuck failed"
                )));
            }
            Ok(self.reset_stuck_count)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            *self.expire_calls.lock().unwrap() += 1;
            if self.expire_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock expire failed"
                )));
            }
            Ok(self.expire_count)
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
    }

    fn make_task() -> Task {
        // Ensure retry_count < max_retries by default so can_retry() is true
        Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        )
    }

    fn make_no_retry_task() -> Task {
        let mut task = make_task();
        task.retry_count = task.max_retries; // can_retry() == false
        task
    }

    // ========== new() tests ==========

    #[test]
    fn test_new_returns_scheduler_with_repository() {
        let repo = Arc::new(MockTaskRepository::new());
        // Scheduler holds the repo; we verify via Arc strong count (repo + scheduler).
        let _scheduler: TaskScheduler<MockTaskRepository> = TaskScheduler::new(repo.clone());
        assert_eq!(Arc::strong_count(&repo), 2);
    }

    // ========== schedule_at tests ==========

    #[tokio::test]
    async fn test_schedule_at_sets_scheduled_at_and_queued_status() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let task = make_task();
        let scheduled_time = Utc::now() + Duration::hours(1);
        let result = scheduler.schedule_at(task, scheduled_time).await;

        assert!(result.is_ok());
        let created = result.unwrap();
        assert_eq!(created.scheduled_at, Some(scheduled_time));
        assert_eq!(created.status, TaskStatus::Queued);
        assert_eq!(repo.create_call_count(), 1);

        // Verify the task passed to create had the right fields
        let last = repo.last_created_task().unwrap();
        assert_eq!(last.scheduled_at, Some(scheduled_time));
        assert_eq!(last.status, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn test_schedule_at_fixes_zero_created_at() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let mut task = make_task();
        // Force created_at to epoch zero (timestamp == 0)
        task.created_at = DateTime::from_timestamp(0, 0).unwrap();

        let result = scheduler
            .schedule_at(task, Utc::now() + Duration::minutes(5))
            .await;

        assert!(result.is_ok());
        let last = repo.last_created_task().unwrap();
        assert_ne!(last.created_at.timestamp(), 0);
    }

    #[tokio::test]
    async fn test_schedule_at_preserves_nonzero_created_at() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let mut task = make_task();
        let original_created_at = Utc::now() - Duration::days(1);
        task.created_at = original_created_at;

        let result = scheduler
            .schedule_at(task, Utc::now() + Duration::minutes(5))
            .await;

        assert!(result.is_ok());
        let last = repo.last_created_task().unwrap();
        assert_eq!(last.created_at, original_created_at);
    }

    #[tokio::test]
    async fn test_schedule_at_create_fails_returns_error() {
        let repo = Arc::new(MockTaskRepository::with_failing_create());
        let scheduler = TaskScheduler::new(repo.clone());

        let result = scheduler
            .schedule_at(make_task(), Utc::now() + Duration::minutes(5))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::Repository(_) => {}
            other => panic!("Expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(repo.create_call_count(), 1);
    }

    // ========== schedule_in tests ==========

    #[tokio::test]
    async fn test_schedule_in_delegates_to_schedule_at() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let delay = Duration::minutes(30);
        let before = Utc::now();
        let result = scheduler.schedule_in(make_task(), delay).await;
        let after = Utc::now();

        assert!(result.is_ok());
        let created = result.unwrap();
        assert_eq!(created.status, TaskStatus::Queued);
        // scheduled_at should be approximately now + delay
        let scheduled = created.scheduled_at.unwrap();
        let expected_min = before + delay - Duration::seconds(1);
        let expected_max = after + delay + Duration::seconds(1);
        assert!(scheduled >= expected_min && scheduled <= expected_max);
        assert_eq!(repo.create_call_count(), 1);
    }

    // ========== reschedule_retry tests ==========

    #[tokio::test]
    async fn test_reschedule_retry_can_retry_queues_task() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let task = make_task();
        let original_attempt_count = task.attempt_count;
        let delay = Duration::minutes(5);

        let result = scheduler.reschedule_retry(task, delay).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.status, TaskStatus::Queued);
        assert_eq!(updated.attempt_count, original_attempt_count + 1);
        assert!(updated.scheduled_at.is_some());
        assert!(updated.started_at.is_none());
        assert!(updated.completed_at.is_none());
        assert_eq!(repo.update_call_count(), 1);
    }

    #[tokio::test]
    async fn test_reschedule_retry_cannot_retry_marks_failed() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let task = make_no_retry_task();
        let result = scheduler.reschedule_retry(task, Duration::minutes(5)).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.status, TaskStatus::Failed);
        assert!(updated.completed_at.is_some());
        assert_eq!(repo.update_call_count(), 1);

        // Verify the task passed to update had Failed status
        let last = repo.last_updated_task().unwrap();
        assert_eq!(last.status, TaskStatus::Failed);
    }

    #[tokio::test]
    async fn test_reschedule_retry_update_fails_returns_error() {
        let repo = Arc::new(MockTaskRepository::with_failing_update());
        let scheduler = TaskScheduler::new(repo.clone());

        let result = scheduler
            .reschedule_retry(make_task(), Duration::minutes(5))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::Repository(_) => {}
            other => panic!("Expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(repo.update_call_count(), 1);
    }

    #[tokio::test]
    async fn test_reschedule_retry_cannot_retry_update_fails_returns_error() {
        // Even when cannot retry, if update fails, error is returned
        let me = MockTaskRepository::with_failing_update();
        let repo = Arc::new(me);
        let scheduler = TaskScheduler::new(repo.clone());

        let result = scheduler
            .reschedule_retry(make_no_retry_task(), Duration::minutes(5))
            .await;

        assert!(result.is_err());
        assert_eq!(repo.update_call_count(), 1);
    }

    // ========== schedule_urgent tests ==========

    #[tokio::test]
    async fn test_schedule_urgent_sets_priority_and_immediate_schedule() {
        let repo = Arc::new(MockTaskRepository::new());
        let scheduler = TaskScheduler::new(repo.clone());

        let before = Utc::now();
        let result = scheduler.schedule_urgent(make_task()).await;
        let after = Utc::now();

        assert!(result.is_ok());
        let created = result.unwrap();
        assert_eq!(created.priority, 100);
        assert_eq!(created.status, TaskStatus::Queued);
        assert!(created.scheduled_at.is_some());
        let scheduled = created.scheduled_at.unwrap();
        assert!(scheduled >= before && scheduled <= after);
        assert_eq!(repo.create_call_count(), 1);
    }

    #[tokio::test]
    async fn test_schedule_urgent_create_fails_returns_error() {
        let repo = Arc::new(MockTaskRepository::with_failing_create());
        let scheduler = TaskScheduler::new(repo.clone());

        let result = scheduler.schedule_urgent(make_task()).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::Repository(_) => {}
            other => panic!("Expected QueueError::Repository, got {:?}", other),
        }
        assert_eq!(repo.create_call_count(), 1);
    }

    // ========== start() tests ==========
    //
    // The start() method spawns a tokio task with a 60-second interval loop.
    // We use tokio::time::pause() + advance() to fast-forward and verify the
    // background task calls reset_stuck_tasks and expire_tasks on each tick.

    #[tokio::test(start_paused = true)]
    async fn test_start_runs_one_tick_calling_reset_and_expire() {
        let repo = Arc::new(MockTaskRepository::with_counts(3, 2));
        let scheduler = TaskScheduler::new(repo.clone());

        let handle = scheduler.start();

        // Advance time past one 60s tick (with a small buffer for scheduling).
        tokio::time::advance(TokioDuration::from_secs(61)).await;
        // Yield to let the spawned task run.
        tokio::task::yield_now().await;

        assert!(
            repo.reset_stuck_call_count() >= 1,
            "reset_stuck_tasks should be called at least once"
        );
        assert!(
            repo.expire_call_count() >= 1,
            "expire_tasks should be called at least once"
        );

        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn test_start_handles_reset_stuck_error_without_panicking() {
        let repo = Arc::new(MockTaskRepository::with_failing_reset_stuck());
        let scheduler = TaskScheduler::new(repo.clone());

        let handle = scheduler.start();

        tokio::time::advance(TokioDuration::from_secs(61)).await;
        tokio::task::yield_now().await;

        // Even on error, the loop continues — expire should still be called.
        assert!(repo.reset_stuck_call_count() >= 1);
        assert!(repo.expire_call_count() >= 1);

        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn test_start_handles_expire_error_without_panicking() {
        let repo = Arc::new(MockTaskRepository::with_failing_expire());
        let scheduler = TaskScheduler::new(repo.clone());

        let handle = scheduler.start();

        tokio::time::advance(TokioDuration::from_secs(61)).await;
        tokio::task::yield_now().await;

        assert!(repo.reset_stuck_call_count() >= 1);
        assert!(repo.expire_call_count() >= 1);

        handle.abort();
    }
}
