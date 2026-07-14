// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

/// 每队并发信号量管理器
///
/// 为每个团队提供一个独立的并发信号量，以限制其并发请求数。
#[derive(Clone, Debug)]
pub struct TeamSemaphore {
    /// 存储每队的信号量
    semaphores: Arc<DashMap<Uuid, Arc<Semaphore>>>,
    /// 默认并发数
    default_permits: usize,
}

impl TeamSemaphore {
    /// 创建一个新的TeamSemaphore实例
    ///
    /// # 参数
    ///
    /// * `default_permits` - 每个团队的默认并发许可数
    ///
    /// # 返回值
    ///
    /// 返回新的TeamSemaphore实例
    pub fn new(default_permits: usize) -> Self {
        Self {
            semaphores: Arc::new(DashMap::new()),
            default_permits,
        }
    }

    /// 获取指定团队的信号量许可
    ///
    /// 如果该团队的信号量不存在，则会创建一个新的。
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回一个信号量许可
    pub async fn acquire(
        &self,
        team_id: Uuid,
    ) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        self.get_or_create(team_id).acquire_owned().await
    }

    /// 获取或创建指定团队的信号量
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队的唯一标识符
    ///
    /// # 返回值
    ///
    /// 返回一个Arc包装的信号量
    fn get_or_create(&self, team_id: Uuid) -> Arc<Semaphore> {
        self.semaphores
            .entry(team_id)
            .or_insert_with(|| Arc::new(Semaphore::new(self.default_permits)))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_stores_default_permits() {
        let sem = TeamSemaphore::new(5);
        assert_eq!(sem.default_permits, 5);
    }

    #[test]
    fn test_new_with_zero_permits() {
        let sem = TeamSemaphore::new(0);
        assert_eq!(sem.default_permits, 0);
    }

    #[test]
    fn test_new_starts_with_empty_map() {
        let sem = TeamSemaphore::new(3);
        let team_id = Uuid::new_v4();
        // Map should be empty before any acquire
        assert!(!sem.semaphores.contains_key(&team_id));
    }

    #[test]
    fn test_get_or_create_creates_new_semaphore() {
        let sem = TeamSemaphore::new(4);
        let team_id = Uuid::new_v4();
        let semaphore = sem.get_or_create(team_id);
        // Newly created semaphore should have all permits available
        assert!(semaphore.try_acquire_owned().is_ok());
        // Map should now contain an entry for this team
        assert!(sem.semaphores.contains_key(&team_id));
    }

    #[test]
    fn test_get_or_create_reuses_existing_semaphore() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        let first = sem.get_or_create(team_id);
        let second = sem.get_or_create(team_id);
        // Both should point to the same underlying semaphore (Arc equality)
        assert!(Arc::ptr_eq(&first, &second));
        // Only one entry should exist for this team
        assert_eq!(sem.semaphores.len(), 1);
    }

    #[test]
    fn test_get_or_create_isolates_different_teams() {
        let sem = TeamSemaphore::new(2);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let sem_a = sem.get_or_create(team_a);
        let sem_b = sem.get_or_create(team_b);
        // Different teams must get distinct semaphores
        assert!(!Arc::ptr_eq(&sem_a, &sem_b));
        assert_eq!(sem.semaphores.len(), 2);
    }

    #[tokio::test]
    async fn test_acquire_returns_permit() {
        let sem = TeamSemaphore::new(3);
        let team_id = Uuid::new_v4();
        let permit = sem.acquire(team_id).await;
        assert!(permit.is_ok());
        // Acquiring should have created an entry for the team
        assert!(sem.semaphores.contains_key(&team_id));
    }

    #[tokio::test]
    async fn test_acquire_within_limit_succeeds() {
        let sem = TeamSemaphore::new(2);
        let team_id = Uuid::new_v4();
        let p1 = sem
            .acquire(team_id)
            .await
            .expect("first acquire should succeed");
        let p2 = sem
            .acquire(team_id)
            .await
            .expect("second acquire should succeed");
        // Hold the permits; both must succeed since limit is 2
        let _ = (p1, p2);
    }

    #[tokio::test]
    async fn test_acquire_over_limit_blocks_until_timeout() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        // Exhaust the single permit (held until end of test)
        let _held_permit = sem
            .acquire(team_id)
            .await
            .expect("first acquire should succeed");
        // Second acquire should block since the permit is held; verify via timeout
        let result = tokio::time::timeout(Duration::from_millis(50), sem.acquire(team_id)).await;
        assert!(
            result.is_err(),
            "second acquire should time out when permits are exhausted"
        );
    }

    #[tokio::test]
    async fn test_acquire_permit_release_allows_next() {
        let sem = TeamSemaphore::new(1);
        let team_id = Uuid::new_v4();
        {
            let _permit = sem
                .acquire(team_id)
                .await
                .expect("first acquire should succeed");
            // permit dropped here
        }
        // After the permit is dropped, a new acquire should succeed immediately
        let result = tokio::time::timeout(Duration::from_millis(200), sem.acquire(team_id)).await;
        assert!(
            result.is_ok(),
            "acquire should succeed after permit is dropped"
        );
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_acquire_isolates_concurrency_per_team() {
        // team A has limit 1, team B has limit 1; exhausting A must not block B
        let sem = TeamSemaphore::new(1);
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();
        let _held_a = sem
            .acquire(team_a)
            .await
            .expect("team A acquire should succeed");
        // team A is now exhausted; team B should still acquire immediately
        let result = tokio::time::timeout(Duration::from_millis(200), sem.acquire(team_b)).await;
        assert!(
            result.is_ok(),
            "team B acquire should not be blocked by team A"
        );
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_clone_shares_underlying_state() {
        let sem = TeamSemaphore::new(2);
        let team_id = Uuid::new_v4();
        // Populate the map on the original
        let _ = sem.get_or_create(team_id);
        let cloned = sem.clone();
        // Clone should share the same DashMap (Arc semantics)
        assert!(cloned.semaphores.contains_key(&team_id));
        assert!(Arc::ptr_eq(&sem.semaphores, &cloned.semaphores));
    }
}
