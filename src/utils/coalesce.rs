// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 请求合并 Single-flight 原语
//!
//! 移植 spider `coalesce.rs`：同一合并键的并发请求只允许首个执行实际抓取，
//! 其余 worker 等待首个完成后从缓存/DB 读取结果，避免重复网络往返。
//!
//! 本模块提供与业务无关的 single-flight 原语：合并键 `K` 与 leader 身份 `L`
//! 全部泛型化，由调用方（`workers::coalesce_coordinator`）提供具体类型。
//! 这样 `utils` 层不依赖 `domain` 层（M3：层依赖修复）。
//!
//! 核心数据结构：
//! - `RequestCoalescer<K, L>`：`DashMap<K, InFlightEntry<L>>` 共享状态
//! - `CoalesceResult<K, L>`：`try_start` 返回枚举（`Proceed`/`Wait`）
//! - `CoalesceGuard<K, L>`：RAII guard，Drop 时广播完成通知并移除条目
//!
//! `ScrapeOptions` 请求身份指纹（`request_fingerprint` / `coalesce_fingerprint`）
//! 已迁至 `crate::workers::request_identity`：指纹实现依赖 `engines`/`common`，
//! 放在 worker 层避免 `utils` 反向依赖引擎类型。
//!
//! 设计要点：
//! - `STALE_TIMEOUT = 120s`：超过该时长的 in-flight 条目视为僵死，`purge_stale` 清理
//! - `broadcast::channel(1)`：容量 1 即可，完成只发一次
//! - `CoalesceSignal`：广播载荷区分 `Completed { success }`（leader 结束，携带成败）
//!   与 `Purged`（僵死条目被清理），等待方据此决定是否按 leader 身份查询结果
//! - `generation`：每次插入 in-flight 条目时单调递增。guard 与 `purge_stale`
//!   只删除 generation 匹配的条目，防止陈旧 guard / 过期清理快照误删同键新条目
//! - guard 持有 `Arc<DashMap>` 引用，不持有 `&RequestCoalescer`，避免生命周期耦合

use dashmap::DashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// 僵死条目超时（秒）
pub const STALE_TIMEOUT: Duration = Duration::from_secs(120);

// === Single-flight 原语 ===

/// coalesce 广播信号：区分 leader 结束结果与 stale 清理
///
/// 等待方据此决定后续动作：
/// - [`Completed { success: true }`](Self::Completed)：leader 已成功落库结果 →
///   按 leader 身份查询结果并收养
/// - [`Completed { success: false }`](Self::Completed)：leader 失败/取消（结果
///   未落库）→ 不查询结果，直接走等待方自身重排
/// - [`Purged`](Self::Purged)：`purge_stale` 清理僵死条目（leader 可能 panic/死锁）→
///   不视为完成，走自身重排
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoalesceSignal {
    /// leader 结束（guard Drop 或显式释放广播），`success` 表示结果是否已落库
    Completed {
        /// `true`：leader 成功完成，等待方可按 leader 身份收养结果；
        /// `false`：leader 失败/取消，等待方不得查询结果
        success: bool,
    },
    /// in-flight 条目被 `purge_stale` 判定僵死并清理（leader 异常）
    Purged,
}

/// In-flight 条目：记录开始时间 + leader 身份 + 完成广播 sender + 条目代号
#[derive(Debug)]
struct InFlightEntry<L> {
    started_at: Instant,
    /// leader（首个获得执行权的 worker）的身份载荷。
    /// 等待方按此载荷查询落库结果——结果落在 leader 名下，非等待方自身身份。
    leader: L,
    sender: broadcast::Sender<CoalesceSignal>,
    /// 条目代号：每次插入时从 `RequestCoalescer` 的单调计数器取新值。
    /// guard Drop 与 `purge_stale` 只删除 generation 匹配的条目，防止陈旧
    /// guard / 过期清理快照误删同键的新条目。
    generation: u64,
}

/// `try_start` 返回结果
#[derive(Debug)]
pub enum CoalesceResult<K: Eq + Hash + Clone, L> {
    /// 调用方获得执行权，可继续实际抓取；guard Drop 后广播
    /// [`CoalesceSignal::Completed`]（默认 `success: false`，成功须调用
    /// [`CoalesceGuard::mark_success`]）
    Proceed(CoalesceGuard<K, L>),
    /// 已有同合并键的请求在执行，调用方应等待广播信号后按 leader 身份读取结果。
    /// 元组第二项为 leader（首个执行方）的身份载荷，等待方据此查询落库结果。
    Wait(broadcast::Receiver<CoalesceSignal>, L),
}

/// RAII guard：持有期间该合并键被标记为 in-flight
///
/// Drop 时按 generation 匹配移除条目并广播 [`CoalesceSignal::Completed`]。
/// 默认 `success: false`：只有调用 [`mark_success`](Self::mark_success) 后 Drop
/// 才广播成功信号。失败、panic、任务取消等路径保持默认值，等待方据此跳过
/// 结果查询直接重排，避免一次无效查询。
#[derive(Debug)]
pub struct CoalesceGuard<K: Eq + Hash + Clone, L> {
    key: K,
    /// 本 guard 插入条目时的代号（见 [`InFlightEntry::generation`]）
    generation: u64,
    /// leader 是否已成功落库结果（默认 `false`，成功路径须显式标记）
    success: bool,
    in_flight: Arc<DashMap<K, InFlightEntry<L>>>,
}

impl<K: Eq + Hash + Clone, L> CoalesceGuard<K, L> {
    /// 标记 leader 已成功完成（结果已落库），Drop 时广播 `success: true`
    ///
    /// 必须在结果持久化之后调用；未调用时 Drop 广播 `success: false`。
    pub fn mark_success(&mut self) {
        self.success = true;
    }

    /// 显式释放：等价于 `drop(self)`
    ///
    /// 提供语义清晰的 API，便于调用方表达"leader 已结束"的意图。
    /// 释放时按当前成功标记广播（未调用 [`mark_success`](Self::mark_success)
    /// 则广播失败信号）。命名 `release` 避免与 `Complete` trait 方法歧义。
    pub fn release(self) {
        // Drop 处理实际清理与广播
        drop(self);
    }

    /// 获取 guard 对应的合并键（测试用）
    #[cfg(test)]
    pub fn key(&self) -> &K {
        &self.key
    }
}

impl<K: Eq + Hash + Clone, L> Drop for CoalesceGuard<K, L> {
    fn drop(&mut self) {
        // 仅删除自己 generation 的条目：若条目已被 purge_stale 清理且同键已有
        // 新 guard 插入，旧 guard 的 Drop 不得删除新条目，也不得向新条目的
        // 等待方广播 Completed。
        //
        // 顺序保持「先移除匹配条目，再广播」：remove_if 返回拥有所有权的条目，
        // 广播不持有 DashMap 锁。
        if let Some((_, entry)) = self
            .in_flight
            .remove_if(&self.key, |_, entry| entry.generation == self.generation)
        {
            if let Err(e) = entry.sender.send(CoalesceSignal::Completed {
                success: self.success,
            }) {
                log::debug!("Coalesce broadcast failed: {} (no active waiters)", e);
            }
        }
    }
}

/// 请求合并器：共享 `DashMap` 追踪 in-flight 合并键
///
/// 线程安全：内部 `Arc<DashMap>`，可廉价克隆共享。
/// 泛型 `K` 为合并键、`L` 为 leader 身份载荷；具体键类型与身份类型由调用方决定，
/// `utils` 层不依赖 `domain`。
#[derive(Debug, Clone)]
pub struct RequestCoalescer<K: Eq + Hash + Clone, L> {
    in_flight: Arc<DashMap<K, InFlightEntry<L>>>,
    /// 条目代号单调计数器（所有 clone 共享）
    next_generation: Arc<AtomicU64>,
}

impl<K: Eq + Hash + Clone, L: Clone> Default for RequestCoalescer<K, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + Clone, L: Clone> RequestCoalescer<K, L> {
    /// 创建新的空合并器
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(DashMap::new()),
            next_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 取下一个条目代号（单调递增，进程内唯一）
    fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 尝试获取该合并键的执行权
    ///
    /// - 首个调用方返回 [`CoalesceResult::Proceed`]，获得 guard；`leader` 被记入
    ///   in-flight 条目，供后续等待方按此身份查询落库结果
    /// - 后续调用方返回 [`CoalesceResult::Wait`]，携带 leader 身份，应 `await`
    ///   receiver 得到 [`CoalesceSignal`] 后按 leader 身份读取结果
    ///
    /// 合并粒度由调用方提供的 `K` 决定：只有 `K` 相等才命中同一 entry。
    ///
    /// 并发安全：使用 `DashMap::entry` 原子插入，避免 TOCTOU。
    ///
    /// # 性能
    ///
    /// `broadcast::channel(1)` 仅在 `Vacant` 分支（Proceed 路径）构造，
    /// 避免在 `Occupied`（Wait 路径）做无谓分配。
    pub fn try_start(&self, key: &K, leader: L) -> CoalesceResult<K, L> {
        use dashmap::mapref::entry::Entry;

        match self.in_flight.entry(key.clone()) {
            Entry::Occupied(o) => {
                // Wait 路径：订阅现有 sender 并读取 leader 身份（零额外分配）
                let entry = o.get();
                let rx = entry.sender.subscribe();
                CoalesceResult::Wait(rx, entry.leader.clone())
            }
            Entry::Vacant(v) => {
                // Proceed 路径：此时才分配 channel（容量 1 即可）
                let (tx, _rx) = broadcast::channel(1);
                let generation = self.next_generation();
                let entry = InFlightEntry {
                    started_at: Instant::now(),
                    leader,
                    sender: tx,
                    generation,
                };
                v.insert(entry);
                CoalesceResult::Proceed(CoalesceGuard {
                    key: key.clone(),
                    generation,
                    success: false,
                    in_flight: self.in_flight.clone(),
                })
            }
        }
    }

    /// 清理超过 [`STALE_TIMEOUT`] 的僵死条目
    ///
    /// 僵死条目（worker panic / 死锁导致 guard 未 Drop）会阻塞相同合并键的后续请求，
    /// 定期调用本方法回收。清理时广播 [`CoalesceSignal::Purged`] 通知等待方
    /// （等待方不视为完成，走自身重排）。
    ///
    /// 返回实际清理的条目数。删除按 `(key, generation)` 条件执行：收集快照后
    /// 条目可能已被 guard Drop 移除并重新插入，generation 不匹配时保留新条目。
    pub fn purge_stale(&self) -> usize {
        let now = Instant::now();
        let stale: Vec<(K, u64)> = self
            .in_flight
            .iter()
            .filter(|entry| now.duration_since(entry.started_at) > STALE_TIMEOUT)
            .map(|entry| (entry.key().clone(), entry.generation))
            .collect();

        let mut purged = 0;
        for (key, generation) in stale {
            if let Some((_, entry)) = self
                .in_flight
                .remove_if(&key, |_, entry| entry.generation == generation)
            {
                purged += 1;
                // 广播 Purged：等待方不视为完成，走自身重排
                let _ = entry.sender.send(CoalesceSignal::Purged);
            }
        }
        purged
    }

    /// 当前 in-flight 条目数（测试用）
    #[cfg(test)]
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// 判断合并键是否 in-flight（测试用）
    #[cfg(test)]
    pub fn is_in_flight(&self, key: &K) -> bool {
        self.in_flight.contains_key(key)
    }

    /// 测试专用：强制清理指定合并键条目并广播 [`CoalesceSignal::Purged`]
    /// （不论是否超时），用于验证等待方对 Purged 信号的处理。
    #[cfg(test)]
    pub(crate) fn purge_key(&self, key: &K) {
        if let Some((_, entry)) = self.in_flight.remove(key) {
            let _ = entry.sender.send(CoalesceSignal::Purged);
        }
    }

    /// 测试专用：移除条目但不广播（sender 随条目 drop），
    /// 用于模拟 leader 异常消失导致等待方 `recv()` 返回 `Closed`。
    #[cfg(test)]
    pub(crate) fn drop_entry_silently(&self, key: &K) {
        self.in_flight.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// 测试合并键：URL 字符串本身（泛型原语不关心业务结构）
    fn key(url: &str) -> String {
        url.to_string()
    }

    /// 首个 try_start 应返回 Proceed
    #[test]
    fn try_start_first_call_returns_proceed() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let result = coalescer.try_start(&key("https://example.com/page1"), 1);
        assert!(matches!(result, CoalesceResult::Proceed(_)));
        assert!(coalescer.is_in_flight(&key("https://example.com/page1")));
    }

    /// 同键第二个 try_start 应返回 Wait
    #[test]
    fn try_start_second_call_same_key_returns_wait() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let _guard = coalescer.try_start(&key("https://example.com/page2"), 1);
        let result = coalescer.try_start(&key("https://example.com/page2"), 2);
        assert!(matches!(result, CoalesceResult::Wait(..)));
    }

    /// 不同键应各自获得 Proceed
    #[test]
    fn try_start_different_keys_both_proceed() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let _g1 = coalescer.try_start(&key("https://a.com"), 1);
        let _g2 = coalescer.try_start(&key("https://b.com"), 2);
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// guard Drop 后该键可再次 Proceed（条目已移除）
    #[test]
    fn guard_drop_removes_entry_and_allows_retry() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        {
            let _guard = coalescer.try_start(&key("https://example.com/temp"), 1);
            assert!(coalescer.is_in_flight(&key("https://example.com/temp")));
        }
        // guard 已 Drop
        assert!(!coalescer.is_in_flight(&key("https://example.com/temp")));
        // 可再次获取
        let result = coalescer.try_start(&key("https://example.com/temp"), 2);
        assert!(matches!(result, CoalesceResult::Proceed(_)));
    }

    /// release 语义等价于 drop
    #[test]
    fn release_is_equivalent_to_drop() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let guard = match coalescer.try_start(&key("https://example.com/release"), 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert!(coalescer.is_in_flight(&key("https://example.com/release")));
        guard.release();
        assert!(!coalescer.is_in_flight(&key("https://example.com/release")));
    }

    /// guard key 与传入键一致
    #[test]
    fn guard_key_matches_inserted_key() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let inserted = key("https://example.com/key-test");
        let guard = match coalescer.try_start(&inserted, 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        assert_eq!(guard.key(), &inserted);
    }

    /// purge_stale 清理超时条目并返回清理数量
    #[test]
    fn purge_stale_removes_timed_out_entries() {
        // 直接构造一个过时条目（模拟 started_at 很早）
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            key("https://example.com/stale"),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader: 1,
                sender: tx,
                generation: coalescer.next_generation(),
            },
        );
        // 还插入一个未过时的
        let guard = match coalescer.try_start(&key("https://example.com/fresh"), 2) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        assert_eq!(coalescer.in_flight_count(), 2);
        let purged = coalescer.purge_stale();
        assert_eq!(purged, 1, "should purge 1 stale entry");
        assert!(!coalescer.is_in_flight(&key("https://example.com/stale")));
        assert!(coalescer.is_in_flight(&key("https://example.com/fresh")));

        // fresh 的 guard 还有效
        guard.release();
    }

    /// purge_stale 无超时条目时返回 0
    #[test]
    fn purge_stale_returns_zero_when_no_stale() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let _guard = coalescer.try_start(&key("https://example.com/active"), 1);
        assert_eq!(coalescer.purge_stale(), 0);
        assert!(coalescer.is_in_flight(&key("https://example.com/active")));
    }

    /// purge_stale 在空合并器上返回 0
    #[test]
    fn purge_stale_empty_returns_zero() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        assert_eq!(coalescer.purge_stale(), 0);
    }

    /// 并发 try_start 同一键只有一个 Proceed
    ///
    /// 使用多线程 + barrier 模拟 worker 并发，验证只有一个获得 Proceed。
    /// 关键：guard 必须保留到所有线程都调用 try_start 之后，否则 guard Drop
    /// 会让后续线程也获得 Proceed。
    #[test]
    fn concurrent_try_start_same_key_only_one_proceed() {
        let coalescer = Arc::new(RequestCoalescer::<String, u64>::new());
        let url = "https://example.com/concurrent";
        let proceeds = Arc::new(std::sync::Mutex::new(0usize));
        let waits = Arc::new(std::sync::Mutex::new(0usize));
        // 收集 guard 防止过早 Drop
        let guards: Arc<std::sync::Mutex<Vec<CoalesceGuard<String, u64>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let barrier = Arc::new(std::sync::Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let c = coalescer.clone();
                let p = proceeds.clone();
                let w = waits.clone();
                let g = guards.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    // 等待所有线程就绪后同时调用 try_start
                    b.wait();
                    let result = c.try_start(&key(url), i as u64);
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

    /// Default 等价于 new
    #[test]
    fn default_equals_new() {
        let a: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let b: RequestCoalescer<String, u64> = RequestCoalescer::default();
        assert_eq!(a.in_flight_count(), 0);
        assert_eq!(b.in_flight_count(), 0);
    }

    /// Clone 后共享同一 DashMap（修改互相可见）
    #[test]
    fn clone_shares_in_flight_state() {
        let a: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let b = a.clone();
        let _guard = a.try_start(&key("https://example.com/shared"), 1);
        // b 也能看到
        assert!(b.is_in_flight(&key("https://example.com/shared")));
        assert_eq!(b.in_flight_count(), 1);
    }

    /// 等待方在 guard 标记成功后 Drop 时收到成功广播（异步验证）
    #[tokio::test]
    async fn waiter_receives_success_broadcast_after_guard_drop() {
        let coalescer = Arc::new(RequestCoalescer::<String, u64>::new());
        let url = "https://example.com/async-wait";

        let mut guard = match coalescer.try_start(&key(url), 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        guard.mark_success();

        let mut rx = match coalescer.try_start(&key(url), 2) {
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

        // 等待方应收到成功广播
        let signal = rx.recv().await.expect("waiter should receive broadcast");
        assert_eq!(signal, CoalesceSignal::Completed { success: true });
    }

    /// 未标记成功的 guard Drop 广播失败信号：等待方据此跳过结果查询
    #[tokio::test]
    async fn unmarked_guard_broadcasts_failure_signal() {
        let coalescer = RequestCoalescer::<String, u64>::new();
        let url = "https://example.com/failed";

        let guard = match coalescer.try_start(&key(url), 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(&key(url), 2) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        drop(guard);
        let signal = rx.recv().await.expect("waiter should receive broadcast");
        assert_eq!(
            signal,
            CoalesceSignal::Completed { success: false },
            "unmarked guard must report failure"
        );
    }

    /// purge_stale 广播 Purged 信号：等待方收到 Purged 而非 Completed
    #[tokio::test]
    async fn purge_stale_broadcasts_purged_signal() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        // 构造已超时的僵死条目（leader 异常未 Drop guard）
        let (tx, _rx) = broadcast::channel(1);
        coalescer.in_flight.insert(
            key("https://example.com/stale-signal"),
            InFlightEntry {
                started_at: Instant::now() - Duration::from_secs(200),
                leader: 1,
                sender: tx,
                generation: coalescer.next_generation(),
            },
        );
        let mut rx = match coalescer.try_start(&key("https://example.com/stale-signal"), 2) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        // purge 触发广播
        assert_eq!(coalescer.purge_stale(), 1);
        // 等待方应收到 Purged（非 Completed）——不视为完成
        let signal = rx.recv().await.expect("should receive purge signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// Wait 路径携带 leader 身份：等待方读取到首个执行方的身份
    #[test]
    fn wait_carries_leader_identity() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let leader_id = 0xDEAD_BEEF_u64;
        let url = "https://example.com/leader-id";
        let _guard = match coalescer.try_start(&key(url), leader_id) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        // 等待方应读到 leader 的身份（而非自己）
        let waiter_id = 42_u64;
        match coalescer.try_start(&key(url), waiter_id) {
            CoalesceResult::Wait(_rx, got_leader_id) => {
                assert_eq!(
                    got_leader_id, leader_id,
                    "waiter should see leader identity"
                );
                assert_ne!(got_leader_id, waiter_id);
            }
            _ => panic!("expected Wait"),
        }
    }

    /// purge_key 不论条目是否超时都强制清理并广播 Purged（测试辅助方法自证）
    #[tokio::test]
    async fn purge_key_broadcasts_purged_regardless_of_age() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let url = "https://example.com/force-purge";
        let _guard = match coalescer.try_start(&key(url), 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(&key(url), 2) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        // 条目未超时，但 purge_key 强制清理
        coalescer.purge_key(&key(url));
        assert!(!coalescer.is_in_flight(&key(url)));
        let signal = rx.recv().await.expect("should receive signal");
        assert_eq!(signal, CoalesceSignal::Purged);
    }

    /// drop_entry_silently 移除条目不广播 → 订阅方 recv 返回 Closed（测试辅助方法自证）
    #[tokio::test]
    async fn drop_entry_silently_causes_closed() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let url = "https://example.com/silent";
        let _guard = match coalescer.try_start(&key(url), 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx = match coalescer.try_start(&key(url), 2) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };
        coalescer.drop_entry_silently(&key(url));
        assert!(!coalescer.is_in_flight(&key(url)));
        // sender 随条目 drop，无广播 → recv 返回 Closed
        let err = rx.recv().await.expect_err("channel should be closed");
        assert!(matches!(err, broadcast::error::RecvError::Closed));
    }

    /// H2 回归：陈旧 guard Drop 不得删除同键新 generation 的条目
    ///
    /// 场景：旧 guard 仍存活时条目被 purge，同键插入新 guard。旧 guard Drop
    /// 若按 key 无条件删除，会误删新条目并向新等待方广播 Completed（30473）。
    #[test]
    fn stale_guard_drop_does_not_remove_replacement_entry() {
        let coalescer: RequestCoalescer<String, u64> = RequestCoalescer::new();
        let url = "https://example.com/generation";
        let coalesce_key = key(url);

        let old_guard = match coalescer.try_start(&coalesce_key, 1) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        // 模拟 purge_stale：旧条目被清理，但旧 guard 尚未 Drop
        coalescer.purge_key(&coalesce_key);
        assert!(!coalescer.is_in_flight(&coalesce_key));

        // 同键新条目（新 generation）与新等待方
        let new_guard = match coalescer.try_start(&coalesce_key, 2) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };
        let mut rx_new = match coalescer.try_start(&coalesce_key, 3) {
            CoalesceResult::Wait(rx, _) => rx,
            _ => panic!("expected Wait"),
        };

        // 旧 guard Drop：generation 不匹配，不得删除新条目/广播 Completed
        drop(old_guard);
        assert!(
            coalescer.is_in_flight(&coalesce_key),
            "stale guard must not remove replacement entry"
        );
        assert_eq!(coalescer.in_flight_count(), 1);
        assert!(
            matches!(
                rx_new.try_recv(),
                Err(broadcast::error::TryRecvError::Empty)
            ),
            "new waiters must not receive stale Completed"
        );

        // 新 guard Drop 仍正常广播并清理
        drop(new_guard);
        assert!(!coalescer.is_in_flight(&coalesce_key));
        assert!(
            matches!(
                rx_new.try_recv().expect("new guard must broadcast"),
                CoalesceSignal::Completed { .. }
            ),
            "new guard must broadcast a completion signal"
        );
    }
}
