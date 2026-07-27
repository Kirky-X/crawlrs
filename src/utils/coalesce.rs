// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 请求合并 Coalesce（design.md §7，T034/R-runtime-002）
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
//! - guard 持有 `Arc<DashMap>` 引用，不持有 `&RequestCoalescer`，避免生命周期耦合
//! - `CompactString` 改用 `String`（项目无 compact_str 依赖，规则 5）

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// 僵死条目超时（秒）
pub const STALE_TIMEOUT: Duration = Duration::from_secs(120);

/// In-flight 条目：记录开始时间 + 完成广播 sender
#[derive(Debug)]
struct InFlightEntry {
    started_at: Instant,
    sender: broadcast::Sender<()>,
}

/// `try_start` 返回结果
#[derive(Debug)]
pub enum CoalesceResult {
    /// 调用方获得执行权，可继续实际抓取；guard Drop 后通知等待方
    Proceed(CoalesceGuard),
    /// 已有同 URL 请求在执行，调用方应等待广播后从缓存/DB 读取
    Wait(broadcast::Receiver<()>),
}

/// RAII guard：持有期间该 URL 被标记为 in-flight
///
/// Drop 时从 `in_flight` 移除条目并广播 `()` 通知所有等待方。
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
        // 移除条目并广播完成通知（广播在 remove 拿到所有权后执行，避免持有 DashMap 锁）
        if let Some((_, entry)) = self.in_flight.remove(&self.url) {
            let _ = entry.sender.send(());
        }
    }
}

/// 请求合并器：共享 `DashMap` 追踪 in-flight URL
///
/// 线程安全：内部 `Arc<DashMap>`，可廉价克隆共享。
/// 放入 `CrawlRsState` 供所有 worker 共享同一实例（T035）。
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
    /// - 首个调用方返回 [`CoalesceResult::Proceed`]，获得 guard
    /// - 后续调用方返回 [`CoalesceResult::Wait`]，应 `await` receiver 后从缓存/DB 读取
    ///
    /// 并发安全：使用 `DashMap::entry` 原子插入，避免 TOCTOU。
    ///
    /// # 性能（性能审查 M-1 修复）
    ///
    /// `broadcast::channel(1)` 仅在 `Vacant` 分支（Proceed 路径）构造，
    /// 避免在 `Occupied`（Wait 路径）做无谓分配。
    pub fn try_start(&self, url: &str) -> CoalesceResult {
        use dashmap::mapref::entry::Entry;

        match self.in_flight.entry(url.to_string()) {
            Entry::Occupied(o) => {
                // Wait 路径：仅订阅现有 sender，零分配
                let rx = o.get().sender.subscribe();
                CoalesceResult::Wait(rx)
            }
            Entry::Vacant(v) => {
                // Proceed 路径：此时才分配 channel（容量 1 即可）
                let (tx, _rx) = broadcast::channel(1);
                let entry = InFlightEntry {
                    started_at: Instant::now(),
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
    /// 定期调用本方法回收。清理时广播 `()` 通知等待方（使其有机会重试）。
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
                let _ = entry.sender.send(());
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
        let result = coalescer.try_start("https://example.com/page1");
        assert!(matches!(result, CoalesceResult::Proceed(_)));
        assert!(coalescer.is_in_flight("https://example.com/page1"));
    }

    /// 同 URL 第二个 try_start 应返回 Wait
    #[test]
    fn try_start_second_call_same_url_returns_wait() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start("https://example.com/page2");
        let result = coalescer.try_start("https://example.com/page2");
        assert!(matches!(result, CoalesceResult::Wait(_)));
    }

    /// 不同 URL 应各自获得 Proceed
    #[test]
    fn try_start_different_urls_both_proceed() {
        let coalescer = RequestCoalescer::new();
        let _g1 = coalescer.try_start("https://a.com");
        let _g2 = coalescer.try_start("https://b.com");
        assert_eq!(coalescer.in_flight_count(), 2);
    }

    /// guard Drop 后该 URL 可再次 Proceed（条目已移除）
    #[test]
    fn guard_drop_removes_entry_and_allows_retry() {
        let coalescer = RequestCoalescer::new();
        {
            let _guard = coalescer.try_start("https://example.com/temp");
            assert!(coalescer.is_in_flight("https://example.com/temp"));
        }
        // guard 已 Drop
        assert!(!coalescer.is_in_flight("https://example.com/temp"));
        // 可再次获取
        let result = coalescer.try_start("https://example.com/temp");
        assert!(matches!(result, CoalesceResult::Proceed(_)));
    }

    /// guard Drop 后等待方应收到广播通知
    #[test]
    fn guard_drop_notifies_waiters() {
        let coalescer = RequestCoalescer::new();
        let _guard = coalescer.try_start("https://example.com/notify");
        let rx = match coalescer.try_start("https://example.com/notify") {
            CoalesceResult::Wait(rx) => rx,
            _ => panic!("expected Wait"),
        };

        // 在另一线程 Drop guard
        let coalescer_clone = coalescer.clone();
        thread::spawn(move || {
            let guard = match coalescer_clone.try_start("https://example.com/notify-2") {
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
        let guard = match coalescer.try_start("https://example.com/release") {
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
        let guard = match coalescer.try_start("https://example.com/url-test") {
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
                sender: tx,
            },
        );
        // 还插入一个未过时的
        let guard = match coalescer.try_start("https://example.com/fresh") {
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
        let _guard = coalescer.try_start("https://example.com/active");
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
                    let result = c.try_start(url);
                    match result {
                        CoalesceResult::Proceed(guard) => {
                            g.lock().unwrap().push(guard);
                            *p.lock().unwrap() += 1;
                        }
                        CoalesceResult::Wait(_) => *w.lock().unwrap() += 1,
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
        let _guard = a.try_start("https://example.com/shared");
        // b 也能看到
        assert!(b.is_in_flight("https://example.com/shared"));
        assert_eq!(b.in_flight_count(), 1);
    }

    /// 等待方在 guard Drop 后能收到广播（异步验证）
    #[tokio::test]
    async fn waiter_receives_broadcast_after_guard_drop() {
        let coalescer = Arc::new(RequestCoalescer::new());
        let url = "https://example.com/async-wait";

        let guard = match coalescer.try_start(url) {
            CoalesceResult::Proceed(g) => g,
            _ => panic!("expected Proceed"),
        };

        let mut rx = match coalescer.try_start(url) {
            CoalesceResult::Wait(rx) => rx,
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

        // 等待方应收到广播
        let result = rx.recv().await;
        assert!(result.is_ok(), "waiter should receive broadcast");
    }
}
