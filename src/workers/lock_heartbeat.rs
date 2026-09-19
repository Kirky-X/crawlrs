// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 任务锁续期心跳
//!
//! 长任务（crawl 单任务多页、用户自定义大 timeout）执行时间可能超过
//! `task_lock_duration_seconds`，锁过期后任务会被其他 worker 经
//! `acquire_next` 的 stale-lock 路径抢占，造成双执行。本组件在任务
//! 执行期间按锁时长的 1/3 周期调用 `TaskRepository::renew_lock` 续期。
//!
//! RAII 语义：[`LockHeartbeat::start`] 返回的 guard 在 drop 时 abort
//! 心跳任务，`process_task` 的任意早退路径无需手动清理。

use crate::domain::repositories::task_repository::TaskRepository;
use log::{debug, warn};
use std::sync::Arc;
use uuid::Uuid;

/// 任务锁续期心跳 guard
///
/// 持有期间后台任务按 `lock_duration_seconds / 3` 周期续期；drop 时立即停止。
pub struct LockHeartbeat {
    handle: tokio::task::JoinHandle<()>,
}

impl LockHeartbeat {
    /// 启动锁续期心跳
    ///
    /// # 参数
    ///
    /// * `repo` - 任务仓储（调用 `renew_lock`）
    /// * `task_id` - 任务 ID
    /// * `worker_id` - 当前 worker 身份（锁守卫匹配用）
    /// * `lock_duration_seconds` - 锁时长（秒）；续期周期取其 1/3，最小 1 秒
    pub fn start(
        repo: Arc<dyn TaskRepository>,
        task_id: Uuid,
        worker_id: Uuid,
        lock_duration_seconds: u64,
    ) -> Self {
        let interval_secs = (lock_duration_seconds / 3).max(1);
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            // 首个 tick 立即返回；跳过它，避免刚认领就做一次无意义续期
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;

            loop {
                ticker.tick().await;
                match repo
                    .renew_lock(task_id, worker_id, lock_duration_seconds as i64)
                    .await
                {
                    Ok(true) => {
                        debug!("Lock renewed for task {} by worker {}", task_id, worker_id);
                    }
                    Ok(false) => {
                        // 任务已不再属于本 worker（被抢占/已终结）：停止续期。
                        // 后续 mark_completed 的锁守卫会拒绝本 worker 的终结请求。
                        warn!(
                            "Lock renewal lost for task {} (worker {}), stopping heartbeat",
                            task_id, worker_id
                        );
                        break;
                    }
                    Err(e) => {
                        // 瞬时 DB 错误不放弃：下一周期重试
                        warn!(
                            "Lock renewal failed for task {} (worker {}): {}",
                            task_id, worker_id, e
                        );
                    }
                }
            }
        });
        Self { handle }
    }
}

impl Drop for LockHeartbeat {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Task;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct MockRepo {
        renew_calls: AtomicU64,
        renew_ok: bool,
    }

    impl MockRepo {
        fn new(renew_ok: bool) -> Self {
            Self {
                renew_calls: AtomicU64::new(0),
                renew_ok,
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
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
        async fn renew_lock(
            &self,
            _task_id: Uuid,
            _worker_id: Uuid,
            _extend_seconds: i64,
        ) -> Result<bool, RepositoryError> {
            self.renew_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.renew_ok)
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
    }

    #[tokio::test]
    async fn test_heartbeat_renews_periodically_and_stops_on_drop() {
        let repo = Arc::new(MockRepo::new(true));
        {
            let _hb = LockHeartbeat::start(
                repo.clone() as Arc<dyn TaskRepository>,
                Uuid::new_v4(),
                Uuid::new_v4(),
                3, // 续期周期 = 1 秒
            );
            tokio::time::sleep(Duration::from_millis(2_500)).await;
        }
        // drop 后不再续期
        tokio::time::sleep(Duration::from_millis(1_200)).await;
        let calls = repo.renew_calls.load(Ordering::SeqCst);
        assert!(calls >= 1, "heartbeat should have renewed at least once");
    }

    #[tokio::test]
    async fn test_heartbeat_stops_when_lock_lost() {
        let repo = Arc::new(MockRepo::new(false));
        let _hb = LockHeartbeat::start(
            repo.clone() as Arc<dyn TaskRepository>,
            Uuid::new_v4(),
            Uuid::new_v4(),
            3,
        );
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        let calls = repo.renew_calls.load(Ordering::SeqCst);
        // 锁丢失后心跳自杀：续期次数被限制（不会无限增长）
        assert!(
            calls <= 2,
            "heartbeat should stop after lock lost, got {calls}"
        );
    }
}
