// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 进程内实例热池
//!
//! 纯内存结构，零外部依赖：DB 登记簿是唯一共享真相，热池只是「当前可用实例」的
//! 缓存（可从 DB + 探活重建）。锁用 `parking_lot`（短临界区惯例），轮转游标用
//! `AtomicUsize`，`acquire` 在单次加锁内完成选择，不做任何 await。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::PoolMember;

/// 热池
pub struct InstancePool {
    members: Arc<RwLock<Vec<Arc<PoolMember>>>>,
    /// 轮转游标（单调递增，取模落到成员数）
    cursor: AtomicUsize,
    /// 目标成员数
    target: usize,
}

impl InstancePool {
    /// 以目标成员数构造空池
    pub fn new(target: usize) -> Self {
        Self {
            members: Arc::new(RwLock::new(Vec::new())),
            cursor: AtomicUsize::new(0),
            target,
        }
    }

    /// 目标成员数（控制回路尽力目标）
    pub fn target(&self) -> usize {
        self.target
    }

    /// 当前成员数
    pub fn len(&self) -> usize {
        self.members.read().len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 是否已满（达到目标）
    pub fn is_full(&self) -> bool {
        self.len() >= self.target
    }

    /// 池内是否已有该 url
    pub fn contains(&self, url: &str) -> bool {
        self.members.read().iter().any(|m| m.url() == url)
    }

    /// 插入成员（按 url 去重）。返回 `true` 表示实际插入。
    pub fn insert(&self, member: Arc<PoolMember>) -> bool {
        let mut members = self.members.write();
        if members.iter().any(|m| m.url() == member.url()) {
            return false;
        }
        members.push(member);
        true
    }

    /// 逐出指定 url 的成员（返回被逐出的成员）
    pub fn evict(&self, url: &str) -> Option<Arc<PoolMember>> {
        let mut members = self.members.write();
        let position = members.iter().position(|m| m.url() == url)?;
        Some(members.remove(position))
    }

    /// 用新成员替换同 url 的旧成员（不存在时插入）。返回是否发生了替换。
    pub fn replace(&self, member: Arc<PoolMember>) -> bool {
        let mut members = self.members.write();
        match members.iter_mut().find(|m| m.url() == member.url()) {
            Some(slot) => {
                *slot = member;
                true
            }
            None => {
                members.push(member);
                false
            }
        }
    }

    /// 成员快照（轮转无关，按插入顺序）
    pub fn snapshot(&self) -> Vec<Arc<PoolMember>> {
        self.members.read().clone()
    }

    /// 清空池（重建前的收敛点）
    pub fn clear(&self) {
        self.members.write().clear();
    }

    /// 轮转取一个可用成员：跳过 `exclude` 中的 url 与 `is_available` 判定不可用者。
    ///
    /// `is_available` 由调用方提供（熔断镜像态由服务侧持有，池不感知熔断器）。
    pub fn acquire(
        &self,
        exclude: &[String],
        is_available: &dyn Fn(&str) -> bool,
    ) -> Option<Arc<PoolMember>> {
        let members = self.members.read();
        let len = members.len();
        if len == 0 {
            return None;
        }
        let start = self.cursor.fetch_add(1, Ordering::Relaxed) % len;
        for offset in 0..len {
            let member = &members[(start + offset) % len];
            if exclude.iter().any(|e| e == member.url()) {
                continue;
            }
            if !is_available(member.url()) {
                continue;
            }
            return Some(Arc::clone(member));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::searxng_instance_repository::{
        SearxngInstance, SearxngInstanceSource, SearxngInstanceState,
    };
    use chrono::Utc;

    fn member(url: &str) -> Arc<PoolMember> {
        let now = Utc::now();
        Arc::new(PoolMember {
            instance: SearxngInstance {
                url: url.to_string(),
                source: SearxngInstanceSource::Seed,
                state: SearxngInstanceState::Active,
                network_type: None,
                version: None,
                api_grade: None,
                http_grade: None,
                tls_grade: None,
                cdn_grade: None,
                uptime_day: Some(99.0),
                uptime_month: None,
                latency_ms: Some(100),
                consecutive_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                dataset_miss_count: 0,
                created_at: now,
                updated_at: now,
            },
        })
    }

    fn always_available(_: &str) -> bool {
        true
    }

    #[test]
    fn test_insert_dedupes_by_url() {
        let pool = InstancePool::new(3);
        assert!(pool.insert(member("https://a.example")));
        assert!(!pool.insert(member("https://a.example")));
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_acquire_rotates_in_insertion_order() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));
        pool.insert(member("https://b.example"));
        pool.insert(member("https://c.example"));

        let first = pool.acquire(&[], &always_available).expect("member");
        let second = pool.acquire(&[], &always_available).expect("member");
        let third = pool.acquire(&[], &always_available).expect("member");
        let fourth = pool.acquire(&[], &always_available).expect("member");

        assert_eq!(first.url(), "https://a.example");
        assert_eq!(second.url(), "https://b.example");
        assert_eq!(third.url(), "https://c.example");
        assert_eq!(fourth.url(), "https://a.example", "cursor must wrap around");
    }

    #[test]
    fn test_acquire_skips_excluded_urls() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));
        pool.insert(member("https://b.example"));

        let acquired = pool
            .acquire(&["https://a.example".to_string()], &always_available)
            .expect("member");
        assert_eq!(acquired.url(), "https://b.example");
    }

    #[test]
    fn test_acquire_skips_unavailable_members() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));
        pool.insert(member("https://b.example"));

        let breaker_open = |url: &str| url != "https://a.example";
        let acquired = pool.acquire(&[], &breaker_open).expect("member");
        assert_eq!(acquired.url(), "https://b.example");
    }

    #[test]
    fn test_acquire_returns_none_when_all_excluded_or_unavailable() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));
        assert!(pool
            .acquire(&["https://a.example".to_string()], &always_available)
            .is_none());
        assert!(pool.acquire(&[], &|_| false).is_none());
    }

    #[test]
    fn test_acquire_empty_pool_returns_none() {
        let pool = InstancePool::new(3);
        assert!(pool.acquire(&[], &always_available).is_none());
    }

    #[test]
    fn test_evict_then_acquire_falls_to_remaining() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));
        pool.insert(member("https://b.example"));

        let evicted = pool.evict("https://a.example").expect("evicted");
        assert_eq!(evicted.url(), "https://a.example");
        assert_eq!(pool.len(), 1);

        let acquired = pool.acquire(&[], &always_available).expect("member");
        assert_eq!(acquired.url(), "https://b.example");

        assert!(pool.evict("https://missing.example").is_none());
    }

    #[test]
    fn test_replace_updates_existing_and_inserts_missing() {
        let pool = InstancePool::new(3);
        pool.insert(member("https://a.example"));

        assert!(
            pool.replace(member("https://a.example")),
            "replace existing"
        );
        assert_eq!(pool.len(), 1);
        assert!(!pool.replace(member("https://b.example")), "insert missing");
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn test_snapshot_and_flags() {
        let pool = InstancePool::new(2);
        assert!(pool.is_empty());
        assert!(!pool.is_full());
        pool.insert(member("https://a.example"));
        pool.insert(member("https://b.example"));
        assert!(pool.is_full());
        assert_eq!(pool.snapshot().len(), 2);
        assert!(pool.contains("https://a.example"));
        assert!(!pool.contains("https://c.example"));
        assert_eq!(pool.target(), 2);

        pool.clear();
        assert!(pool.is_empty());
    }
}
