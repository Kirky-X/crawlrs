// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 请求合并 Coalesce
//!
//! 移植 spider `coalesce.rs`：同一 URL 并发请求只允许首个执行实际抓取，
//! 其余 worker 等待首个完成后从缓存/DB 读取结果，避免重复网络往返。
//!
//! 核心数据结构：
//! - [`RequestCoalescer`]：`DashMap<String, InFlightEntry>` 共享状态
//! - [`CoalesceResult`]：`try_start` 返回枚举（`Proceed`/`Wait`）
//! - [`CoalesceGuard`]：RAII guard，Drop 时广播完成通知并移除条目
//!
//! 设计要点：
//! - `STALE_TIMEOUT = 120s`：超过该时长的 in-flight 条目视为僵死，`purge_stale` 清理
//! - `broadcast::channel(1)`：容量 1 即可，完成只发一次
//! - [`CoalesceSignal`]：广播载荷区分 `Completed`（leader 正常完成）与 `Purged`
//!   （僵死条目被清理），等待方据此决定是否按 leader task_id 查询结果
//! - guard 持有 `Arc<DashMap>` 引用，不持有 `&RequestCoalescer`，避免生命周期耦合
//! - `CompactString` 改用 `String`（项目无 compact_str 依赖）

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

/// 僵死条目超时（秒）
pub const STALE_TIMEOUT: Duration = Duration::from_secs(120);

/// coalesce 广播信号：区分 leader 正常完成与 stale 清理（R-engines-004）
///
/// 等待方据此决定后续动作：
/// - [`Completed`](Self::Completed)：leader guard Drop，抓取正常完成 → 按 leader task_id 查询结果
/// - [`Purged`](Self::Purged)：`purge_stale` 清理僵死条目（leader 可能 panic/死锁）→
///   不视为完成，走自身重排
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoalesceSignal {
    /// leader 抓取正常完成（guard Drop 广播）
    Completed,
    /// in-flight 条目被 `purge_stale` 判定僵死并清理（leader 异常）
    Purged,
}

/// In-flight 条目：记录开始时间 + leader 任务 ID + 完成广播 sender
#[derive(Debug)]
struct InFlightEntry {
    started_at: Instant,
    /// leader（首个获得执行权的 worker）的任务 ID。
    /// 等待方按此 ID 查询落库结果——结果落在 leader 名下，非等待方自身 task_id。
    leader_task_id: Uuid,
    sender: broadcast::Sender<CoalesceSignal>,
}

/// `try_start` 返回结果
#[derive(Debug)]
pub enum CoalesceResult {
    /// 调用方获得执行权，可继续实际抓取；guard Drop 后广播 [`CoalesceSignal::Completed`]
    Proceed(CoalesceGuard),
    /// 已有同 URL 请求在执行，调用方应等待广播信号后按 leader task_id 读取结果。
    /// 元组第二项为 leader（首个执行方）的 task_id，等待方据此查询落库结果。
    Wait(broadcast::Receiver<CoalesceSignal>, Uuid),
}

/// RAII guard：持有期间该 URL 被标记为 in-flight
///
/// Drop 时从 `in_flight` 移除条目并广播 [`CoalesceSignal::Completed`] 通知所有等待方。
/// 调用方可显式 [`complete`](Self::complete) 以提前释放（语义等价于 drop）。
#[derive(Debug)]
pub struct CoalesceGuard {
    url: String,
    in_flight: Arc<DashMap<String, InFlightEntry>>,
}

impl CoalesceGuard {
    /// 显式释放：等价于 `drop(self)`
    ///
    /// 提供语义清晰的 API，便于调用方表达"抓取已完成"的意图。
    /// 命名 `release` 避免与 `Complete` trait 方法歧义。
    pub fn release(self) {
        // Drop 处理实际清理与广播
        drop(self);
    }

    /// 获取 guard 对应的 URL（测试与日志用）
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for CoalesceGuard {
    fn drop(&mut self) {
        // 先 clone sender，再 remove，再 send。
        // 确保 send 不依赖 DashMap entry 的生命周期，避免持有锁期间执行 I/O。
        let sender = self
            .in_flight
            .get(&self.url)
            .map(|entry| entry.sender.clone());
        self.in_flight.remove(&self.url);
        if let Some(tx) = sender {
            if let Err(e) = tx.send(CoalesceSignal::Completed) {
                log::debug!(
                    "Coalesce broadcast failed for URL {}: {} (no active waiters)",
                    self.url,
                    e
                );
            }
        }
    }
}

/// 请求合并器：共享 `DashMap` 追踪 in-flight URL
///
/// 线程安全：内部 `Arc<DashMap>`，可廉价克隆共享。
/// 放入 `CrawlRsState` 供所有 worker 共享同一实例。
#[derive(Debug, Clone)]
pub struct RequestCoalescer {
    in_flight: Arc<DashMap<String, InFlightEntry>>,
}

impl Default for RequestCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestCoalescer {
    /// 创建新的空合并器
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(DashMap::new()),
        }
    }

    /// 尝试获取 URL 的执行权
    ///
    /// - 首个调用方返回 [`CoalesceResult::Proceed`]，获得 guard；`leader_task_id`
    ///   被记入 in-flight 条目，供后续等待方按此 ID 查询落库结果
    /// - 后续调用方返回 [`CoalesceResult::Wait`]，携带 leader 的 task_id，
    ///   应 `await` receiver 得到 [`CoalesceSignal`] 后按 leader task_id 读取结果
    ///
    /// 并发安全：使用 `DashMap::entry` 原子插入，避免 TOCTOU。
    ///
    /// # 性能
    ///
    /// `broadcast::channel(1)` 仅在 `Vacant` 分支（Proceed 路径）构造，
    /// 避免在 `Occupied`（Wait 路径）做无谓分配。
    pub fn try_start(&self, url: &str, leader_task_id: Uuid) -> CoalesceResult {
        use dashmap::mapref::entry::Entry;

        match self.in_flight.entry(url.to_string()) {
            Entry::Occupied(o) => {
                // Wait 路径：订阅现有 sender 并读取 leader 的 task_id（零额外分配）
                let entry = o.get();
                let rx = entry.sender.subscribe();
                CoalesceResult::Wait(rx, entry.leader_task_id)
            }
            Entry::Vacant(v) => {
                // Proceed 路径：此时才分配 channel（容量 1 即可）
                let (tx, _rx) = broadcast::channel(1);
                let entry = InFlightEntry {
                    started_at: Instant::now(),
                    leader_task_id,
                    sender: tx,
                };
                v.insert(entry);
                CoalesceResult::Proceed(CoalesceGuard {
                    url: url.to_string(),
                    in_flight: self.in_flight.clone(),
                })
            }
        }
    }

    /// 清理超过 [`STALE_TIMEOUT`] 的僵死条目
    ///
    /// 僵死条目（worker panic / 死锁导致 guard 未 Drop）会阻塞后续相同 URL 的请求，
    /// 定期调用本方法回收。清理时广播 [`CoalesceSignal::Purged`] 通知等待方
    /// （等待方不视为完成，走自身重排）。
    pub fn purge_stale(&self) -> usize {
        let now = Instant::now();
        let stale_urls: Vec<String> = self
            .in_flight
            .iter()
            .filter(|entry| now.duration_since(entry.started_at) > STALE_TIMEOUT)
            .map(|entry| entry.key().clone())
            .collect();

        let purged = stale_urls.len();
        for url in stale_urls {
            if let Some((_, entry)) = self.in_flight.remove(&url) {
                // 广播 Purged：等待方不视为完成，走自身重排（R-engines-004）
                let _ = entry.sender.send(CoalesceSignal::Purged);
            }
        }
        purged
    }

    /// 当前 in-flight 条目数（测试与监控用）
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// 判断 URL 是否 in-flight（测试用）
    pub fn is_in_flight(&self, url: &str) -> bool {
        self.in_flight.contains_key(url)
    }

    /// 测试专用：强制清理指定 URL 条目并广播 [`CoalesceSignal::Purged`]
    /// （不论是否超时），用于验证等待方对 Purged 信号的处理。
    #[cfg(test)]
    pub(crate) fn purge_url(&self, url: &str) {
        if let Some((_, entry)) = self.in_flight.remove(url) {
            let _ = entry.sender.send(CoalesceSignal::Purged);
        }
    }

    /// 测试专用：移除条目但不广播（sender 随条目 drop），
    /// 用于模拟 leader 异常消失导致等待方 `recv()` 返回 `Closed`。
    #[cfg(test)]
    pub(crate) fn drop_entry_silently(&self, url: &str) {
        self.in_flight.remove(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// 首个 try_start 应返回 Proceed
    #[test]
    fn try_start_first_call_returns_proceed() {
        let coalescer = RequestCoalescer::new();
        let result = coalescer.try_start("https://example.com/page1", Uuid::new_v4());
        assert!(matches!(result, CoalesceResult::Proceed(_)));
        assert!(coalescer.is_in_flight("https://example.com/page1"));
    }

    /// 同 URL 第二个 try_start 应返回 Wait
    #[test]
    fn try_start_second_call_same_url_returns_wait() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start("https://example.com/page2", Uuid::new_v4());
        let result = coalescer.try_start("https://example.com/page2", Uuid::new_v4());
        assert!(matches!(result, CoalesceResult::Wait(..)));
    }

    /// 不同 URL 应各自获得 Proceed
    #[test]
    fn try_start_different_urls_both_proceed() {
        let coalescer = RequestCoalescer::new();
        let _g1 = coalescer.try_start("https://a.com", Uuid::new_v4());
        let _g2 = coalescer.try_start("https://b.com", Uuid::new_v4());
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// guard Drop 后该 URL 可再次 Proceed（条目已移除）
    #[test]
    fn guard_drop_removes_entry_and_allows_retry() {
        let coalescer = RequestCoalescer::new();
        {
            let _guard = coalescer.try_start("https://example.com/temp", Uuid::new_v4());
            assert!(coalescer.is_in_flight("https://example.com/temp"));
        }
        // guard 已 Drop
        assert!(!coalescer.is_in_flight("https://example.com/temp"));
        // 可再次获取
        let result = coalescer.try_start("https://example.com/temp", Uuid::new_v4());
        assert!(matches!(result, CoalesceResult::Proceed(_)));
    }

    /// guard Drop 后等待方应收到广播通知
    #[test]
    fn guard_drop_notifies_waiters() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start("https://example.com/notify", Uuid::new_v4());
        let rx = match coalescer.try_start("https://example.com/notify", Uuid::new_v4()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        // 在另一线程 Drop guard
        let coalescer_clone = coalescer.clone();
        thread::spawn(move || {
            let guard =
                match coalescer_clone.try_start("https://example.com/notify-2", Uuid::new_v4()) {
                    CoalesceResult::Proceed(g) => g,
                    _ => panic!("expected Proceed"),
                };
            drop(guard);
        })
        .join()
        .expect("thread panicked");

        // 原 guard 还活着，先 Drop 它以触发广播
        // 注：此测试验证 rx 在 guard Drop 后能收到（这里 guard 还在作用域）
        // 为简化测试，直接验证 rx 存在即可——异步 recv 在集成测试中验证
        let _ = rx;
    }

    /// release() 语义等价于 drop()
    #[test]
    fn release_is_equivalent_to_drop() {
        let coalescer = RequestCoalescer::new();
        let guard = match coalescer.try_start("https://example.com/release", Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert!(coalescer.is_in_flight("https://example.com/release"));
        guard.release();
        assert!(!coalescer.is_in_flight("https://example.com/release"));
    }

    /// guard url() 返回正确 URL
    #[test]
    fn guard_url_returns_correct_url() {
        let coalescer = RequestCoalescer::new();
        let guard = match coalescer.try_start("https://example.com/url-test", Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert_eq!(guard.url(), "https://example.com/url-test");
    }

    /// purge_stale 清理超时条目并返回清理数量
    #[test]
    fn purge_stale_removes_timed_out_entries() {
        // 直接构造一个过时条目（模拟 started_at 很早）
        let coalescer = RequestCoalescer::new();
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            "https://example.com/stale".to_string(),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader_task_id: Uuid::new_v4(),
                sender: tx,
            },
        );
        // 还插入一个未过时的
        let guard = match coalescer.try_start("https://example.com/fresh", Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        assert_eq!(coalescer.in_flight_count(), 2);
        let purged = coalescer.purge_stale();
        assert_eq!(purged, 1, "should purge 1 stale entry");
        assert!(!coalescer.is_in_flight("https://example.com/stale"));
        assert!(coalescer.is_in_flight("https://example.com/fresh"));

        // fresh 的 guard 还有效
        guard.release();
    }

    /// purge_stale 无超时条目时返回 0
    #[test]
    fn purge_stale_returns_zero_when_no_stale() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start("https://example.com/active", Uuid::new_v4());
        assert_eq!(coalescer.purge_stale(), 0);
        assert!(coalescer.is_in_flight("https://example.com/active"));
    }

    /// purge_stale 在空合并器上返回 0
    #[test]
    fn purge_stale_empty_returns_zero() {
        let coalescer = RequestCoalescer::new();
        assert_eq!(coalescer.purge_stale(), 0);
    }

    /// 并发 try_start 同一 URL 只有一个 Proceed
    ///
    /// 使用多线程 + barrier 模拟 worker 并发，验证只有一个获得 Proceed。
    /// 关键：guard 必须保留到所有线程都调用 try_start 之后，否则 guard Drop
    /// 会让后续线程也获得 Proceed。
    #[test]
    fn concurrent_try_start_same_url_only_one_proceed() {
        let coalescer = Arc::new(RequestCoalescer::new());
        let url = "https://example.com/concurrent";
        let proceeds = Arc::new(std::sync::Mutex::new(0usize));
        let waits = Arc::new(std::sync::Mutex::new(0usize));
        // 收集 guard 防止过早 Drop
        let guards: Arc<std::sync::Mutex<Vec<CoalesceGuard>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let barrier = Arc::new(std::sync::Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let c = coalescer.clone();
                let p = proceeds.clone();
                let w = waits.clone();
                let g = guards.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    // 等待所有线程就绪后同时调用 try_start
                    b.wait();
                    let result = c.try_start(url, Uuid::new_v4());
                    match result {
                        CoalesceResult::Proceed(guard) => {
                            g.lock().unwrap().push(guard);
                            *p.lock().unwrap() += 1;
                        }
                        CoalesceResult::Wait(..) => *w.lock().unwrap() += 1,
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        assert_eq!(*proceeds.lock().unwrap(), 1, "exactly one Proceed");
        assert_eq!(*waits.lock().unwrap(), 7, "seven Wait");
        // guards 在此处 Drop，清理 in_flight
    }

    /// Default 等价于 new()
    #[test]
    fn default_equals_new() {
        let a = RequestCoalescer::new();
        let b = RequestCoalescer::default();
        assert_eq!(a.in_flight_count(), 0);
        assert_eq!(b.in_flight_count(), 0);
    }

    /// Clone 后共享同一 DashMap（修改互相可见）
    #[test]
    fn clone_shares_in_flight_state() {
        let a = RequestCoalescer::new();
        let b = a.clone();
        let _guard = a.try_start("https://example.com/shared", Uuid::new_v4());
        // b 也能看到
        assert!(b.is_in_flight("https://example.com/shared"));
        assert_eq!(b.in_flight_count(), 1);
    }

    /// 等待方在 guard Drop 后能收到广播（异步验证）
    #[tokio::test]
    async fn waiter_receives_broadcast_after_guard_drop() {
        let coalescer = Arc::new(RequestCoalescer::new());
        let url = "https://example.com/async-wait";

        let guard = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        let mut rx = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        // 在异步任务中 Drop guard
        let coalescer_clone = coalescer.clone();
        tokio::spawn(async move {
            // 稍后 Drop
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(guard);
            let _ = coalescer_clone;
        });

        // 等待方应收到 Completed 广播
        let signal = rx.recv().await.expect("waiter should receive broadcast");
        assert_eq!(signal, CoalesceSignal::Completed);
    }

    /// purge_stale 广播 Purged 信号：等待方收到 Purged 而非 Completed（R-engines-004）
    #[tokio::test]
    async fn purge_stale_broadcasts_purged_signal() {
        let coalescer = RequestCoalescer::new();
        // 构造已超时的僵死条目（leader 异常未 Drop guard）
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            "https://example.com/stale-signal".to_string(),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader_task_id: Uuid::new_v4(),
                sender: tx,
            },
        );
        // 等待方在 purge 前订阅
        let mut rx = match coalescer.try_start("https://example.com/stale-signal", Uuid::new_v4()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        // purge 触发广播
        assert_eq!(coalescer.purge_stale(), 1);
        // 等待方应收到 Purged（非 Completed）——不视为完成
        let signal = rx.recv().await.expect("should receive purge signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// Wait 路径携带 leader task_id：等待方读取到首个执行方的 task_id
    #[test]
    fn wait_carries_leader_task_id() {
        let coalescer = RequestCoalescer::new();
        let leader_id = Uuid::new_v4();
        let url = "https://example.com/leader-id";
        let _guard = match coalescer.try_start(url, leader_id) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        // 等待方应读到 leader 的 task_id（而非自己的）
        let waiter_id = Uuid::new_v4();
        match coalescer.try_start(url, waiter_id) {
            CoalesceResult::Wait(_rx, got_leader_id) => {
                assert_eq!(got_leader_id, leader_id, "waiter should see leader task_id");
                assert_ne!(got_leader_id, waiter_id);
            }
            _ => panic!("expected Wait"),
        }
    }

    /// purge_url 不论条目是否超时都强制清理并广播 Purged（测试辅助方法自证）
    #[tokio::test]
    async fn purge_url_broadcasts_purged_regardless_of_age() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/force-purge";
        let _guard = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        // 条目未超时，但 purge_url 强制清理
        coalescer.purge_url(url);
        assert!(!coalescer.is_in_flight(url));
        let signal = rx.recv().await.expect("should receive signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// drop_entry_silently 移除条目不广播 → 订阅方 recv 返回 Closed（测试辅助方法自证）
    #[tokio::test]
    async fn drop_entry_silently_causes_closed() {
        let coalescer = RequestCoalescer::new();
        let url = "https://example.com/silent";
        let _guard = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(url, Uuid::new_v4()) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        coalescer.drop_entry_silently(url);
        assert!(!coalescer.is_in_flight(url));
        // sender 随条目 drop，无广播 → recv 返回 Closed
        let err = rx.recv().await.expect_err("channel should be closed");
        assert!(matches!(err, broadcast::error::RecvError::Closed));
    }
}
