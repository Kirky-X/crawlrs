// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-host 纪律组件
//!
//! 每个 host 独立维护三件事：
//! 1. **礼貌限速**：`gate` 在 per-host 预约时刻表上串行发放放行时刻，
//!    同 host 相邻请求（含并发到达的）间隔 ≥ `min_interval`
//! 2. **429 冷却**：收到 429 后按 `max(retry_after, cooldown_default)` 冷却，
//!    冷却期内 `gate` 有界等待（单次钳制 ≤5s，避免占用 worker 过久）
//! 3. **裸 403 连击**：无 WAF 指纹的 403 连续 `bare403_threshold` 次后置
//!    `tls_suspect`，提示调用方升级 TLS 指纹（带 challenge 页的 403 不计数——
//!    那是 JS 挑战而非 TLS 指纹问题）
//!
//! # 并发模型
//!
//! 与 `EngineRouter::host_browser_hint` 同款 `DashMap` 约定：先 drop `get`
//! 守卫再做任何写操作（同 shard 的 get 守卫存活期间写入会死锁，
//! 见 `host_needs_browser` 的过期清除路径）。`gate` 的检查与预约在同一个
//! `entry` 临界区内完成（读 `max(next_allowed, cooldown_until)` → 写
//! `next_allowed = release_at + min_interval`），守卫块尾即放、不跨
//! `await` 持有——并发同 host 请求各自取到严格递增的预约时刻。若检查与
//! 写入分离在 sleep 两侧，一批请求会读到同一快照、等同一时长后同时放行，
//! 批内间隔 0，限速退化为每窗口放一整批。`observe` 各分支的
//! `get_mut`/`entry` 守卫作用域内无二次 map 访问。

use std::time::{Duration, Instant};

use dashmap::DashMap;

/// 默认 per-host 最小请求间隔
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_secs(1);
/// 默认 429 冷却时长（Retry-After 缺失或更小时的兜底）
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);
/// 默认裸 403 连击阈值（连续 N 次后置 tls_suspect）
pub const DEFAULT_BARE403_THRESHOLD: u32 = 3;

/// `gate` 单次等待钳制上限：冷却再长也不能把 worker 占住超过 5s
const MAX_GATE_WAIT: Duration = Duration::from_secs(5);

/// 惰性清扫间隔：达到间隔后下一次 `gate`/`observe` 触发一次全表清扫，
/// 回收只被访问过一次的 host 条目（评审 LOW：state 只增不删，内存无界）
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// 单个 host 的纪律状态
#[derive(Debug, Clone)]
struct HostState {
    /// 下一个预约放行时刻（`gate` 在临界区内推进 `+= min_interval`）
    next_allowed: Instant,
    /// 429 冷却截止时刻；`None` 表示不在冷却期
    cooldown_until: Option<Instant>,
    /// 裸 403 连击计数（Success 清零，challenge 403 / Failure 不动）
    bare403_streak: u32,
    /// 裸 403 连击达到阈值后置位；Success 清除
    tls_suspect: bool,
}

/// `observe` 的请求结果分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveOutcome {
    /// 最终 2xx 成功：清除冷却、连击与 tls_suspect
    Success,
    /// 429 限速：按 `max(retry_after, cooldown_default)` 进入冷却（连击不动）
    RateLimited {
        /// 响应 Retry-After 头解析出的秒数；`None` 表示头缺失或不可解析
        retry_after: Option<Duration>,
    },
    /// 403
    Forbidden {
        /// true = 带 challenge 的 WAF 页（不改连击）；
        /// false = 裸 403（连击 +1，达阈值置 tls_suspect）
        challenge: bool,
    },
    /// 一般失败（不改任何状态）
    Failure,
}

/// Per-host 纪律控制器：礼貌限速 + 429 冷却 + 裸 403 连击 → TLS 指纹升级
///
/// 线程安全：内部 `DashMap`（分片锁），可安全跨任务共享。
#[derive(Debug)]
pub struct HostDiscipline {
    /// host → 纪律状态
    state: DashMap<String, HostState>,
    /// 相邻请求最小间隔（不可变）
    min_interval: Duration,
    /// 429 冷却默认时长（不可变）
    cooldown_default: Duration,
    /// 裸 403 连击阈值（不可变）
    bare403_threshold: u32,
    /// 上次全表清扫时刻（惰性触发，间隔 [`SWEEP_INTERVAL`]）
    last_sweep: parking_lot::Mutex<Instant>,
}

impl HostDiscipline {
    /// 创建指定参数的 HostDiscipline
    ///
    /// # Panics
    ///
    /// `bare403_threshold == 0` 时 panic（fail-fast）：0 意味着"无需连击"，
    /// 任何一次裸 403 都会立即置位，属于配置错误。
    pub fn new(min_interval: Duration, cooldown_default: Duration, bare403_threshold: u32) -> Self {
        assert!(
            bare403_threshold >= 1,
            "bare403_threshold must be >= 1, got {bare403_threshold}"
        );
        Self {
            state: DashMap::new(),
            min_interval,
            cooldown_default,
            bare403_threshold,
            last_sweep: parking_lot::Mutex::new(Instant::now()),
        }
    }

    /// 使用默认参数创建（min_interval=1s，cooldown_default=60s，threshold=3）
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_MIN_INTERVAL,
            DEFAULT_COOLDOWN,
            DEFAULT_BARE403_THRESHOLD,
        )
    }

    /// 通过纪律门：必要时等待，然后放行
    ///
    /// 在 per-host 预约时刻表上领取本次放行时刻：
    /// `release_at = max(next_allowed, cooldown_until, now)`，并在同一临界区
    /// 把时刻表推进 `next_allowed = release_at + min_interval`。检查与预约
    /// 必须在同一 `entry` 临界区内完成——并发到达的同 host 请求各自取到
    /// 严格递增的 `release_at`，相邻放行间隔 ≥ `min_interval`。
    ///
    /// 随后 sleep 到 `release_at`，单次等待钳制 ≤ [`MAX_GATE_WAIT`]：
    /// 冷却是礼貌信号而非硬门禁，不能把 worker 无限期占住。钳制只缩短
    /// 实际睡眠时长，不回改时刻表——批内间隔仍由预约序列保证。
    pub async fn gate(&self, host: &str) {
        self.maybe_sweep();
        let now = Instant::now();
        let release_at = {
            let mut st = self
                .state
                .entry(host.to_string())
                .or_insert_with(|| HostState {
                    next_allowed: now,
                    cooldown_until: None,
                    bare403_streak: 0,
                    tls_suspect: false,
                });
            let release_at = st
                .cooldown_until
                .unwrap_or(now)
                .max(st.next_allowed)
                .max(now);
            st.next_allowed = release_at + self.min_interval;
            release_at
        };
        let wait = release_at
            .checked_duration_since(Instant::now())
            .unwrap_or_default()
            .min(MAX_GATE_WAIT);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    /// 记录一次请求结果（语义见 [`ObserveOutcome`]）
    ///
    /// 对未经过 `gate` 的 host 同样生效（`entry` 惰性建档），
    /// 调用方无需保证 observe 先于 gate 发生。
    pub fn observe(&self, host: &str, outcome: ObserveOutcome) {
        self.maybe_sweep();
        match outcome {
            ObserveOutcome::Success => {
                let now = Instant::now();
                if let Some(mut st) = self.state.get_mut(host) {
                    st.cooldown_until = None;
                    // 时刻表一并收敛到当前时刻：成功即恢复健康，
                    // 长冷却期残留的远期预约不应继续钳制后续请求
                    st.next_allowed = st.next_allowed.min(now);
                    st.bare403_streak = 0;
                    st.tls_suspect = false;
                }
            }
            ObserveOutcome::RateLimited { retry_after } => {
                let now = Instant::now();
                let until = now + retry_after.unwrap_or_default().max(self.cooldown_default);
                self.state
                    .entry(host.to_string())
                    .and_modify(|st| st.cooldown_until = Some(until))
                    .or_insert(HostState {
                        next_allowed: now + self.min_interval,
                        cooldown_until: Some(until),
                        bare403_streak: 0,
                        tls_suspect: false,
                    });
            }
            ObserveOutcome::Forbidden { challenge: false } => {
                let threshold = self.bare403_threshold;
                self.state
                    .entry(host.to_string())
                    .and_modify(|st| {
                        st.bare403_streak = st.bare403_streak.saturating_add(1);
                        if st.bare403_streak >= threshold {
                            st.tls_suspect = true;
                        }
                    })
                    .or_insert(HostState {
                        next_allowed: Instant::now() + self.min_interval,
                        cooldown_until: None,
                        bare403_streak: 1,
                        tls_suspect: threshold <= 1,
                    });
            }
            // challenge 403（带 WAF 指纹）与一般失败不改任何状态：
            // 前者已有 needs_js 升级路径处理，后者与 host 纪律无关
            ObserveOutcome::Forbidden { challenge: true } | ObserveOutcome::Failure => {}
        }
    }

    /// host 是否已被标记为 TLS 指纹可疑
    pub fn tls_suspect(&self, host: &str) -> bool {
        self.state
            .get(host)
            .map(|st| st.tls_suspect)
            .unwrap_or(false)
    }

    /// 距上次清扫超过 [`SWEEP_INTERVAL`] 时做一次全表回收。
    ///
    /// 在 `gate`/`observe` 上惰性触发（不引入后台任务）：
    /// `state.retain` 逐 shard 持锁，60s 一次的摊销成本可忽略。
    fn maybe_sweep(&self) {
        let mut last = self.last_sweep.lock();
        if last.elapsed() < SWEEP_INTERVAL {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.state
            .retain(|_, st| !Self::is_stale(st, Instant::now()));
    }

    /// 条目是否已不含任何信息——与"该 host 从未出现过"等价：
    /// 无冷却、无连击、无 TLS 可疑、时刻表也已过期。
    /// 注意 `tls_suspect == true` 的条目永不过期（只能被 Success 清除）。
    fn is_stale(st: &HostState, now: Instant) -> bool {
        st.bare403_streak == 0
            && !st.tls_suspect
            && st.cooldown_until.is_none_or(|until| until <= now)
            && st.next_allowed <= now
    }

    /// 立即执行一次全表清扫（不受 [`SWEEP_INTERVAL`] 节流；测试与诊断用）
    #[cfg(test)]
    fn sweep_now(&self) {
        self.state
            .retain(|_, st| !Self::is_stale(st, Instant::now()));
    }
}

impl Default for HostDiscipline {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// threshold=3、min_interval=100ms 的测试实例
    fn discipline() -> HostDiscipline {
        HostDiscipline::new(Duration::from_millis(100), Duration::from_secs(60), 3)
    }

    // ========== 构造 ==========

    #[test]
    #[should_panic(expected = "bare403_threshold must be >= 1")]
    fn new_zero_threshold_panics() {
        let _ = HostDiscipline::new(Duration::from_secs(1), Duration::from_secs(60), 0);
    }

    #[test]
    fn tls_suspect_false_for_unknown_host() {
        let d = discipline();
        assert!(!d.tls_suspect("never-seen.example.com"));
    }

    // ========== gate：首次不等待 / 有界等待 ==========

    #[tokio::test]
    async fn gate_first_call_does_not_wait() {
        let d = discipline();
        let start = Instant::now();
        d.gate("fresh.example.com").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(50),
            "first gate must not wait, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn gate_waits_min_interval_between_requests() {
        let d = discipline();
        d.gate("hot.example.com").await;
        let start = Instant::now();
        d.gate("hot.example.com").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(90),
            "second gate should wait ~min_interval, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "second gate wait should be bounded by min_interval, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn gate_waits_until_cooldown_expiry() {
        // cooldown_default 必须小于 retry_after，否则 max 语义会把
        // 冷却提升到默认值（max 语义已由 rate_limited_* 系列同步测试覆盖）
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_millis(100), 3);
        d.gate("rl.example.com").await;
        d.observe(
            "rl.example.com",
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_millis(200)),
            },
        );
        let start = Instant::now();
        d.gate("rl.example.com").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(180),
            "gate should wait until cooldown expiry, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "gate wait should be bounded by cooldown, took {elapsed:?}"
        );
    }

    // ========== 冷却 / 时刻表状态（读内部状态，纯同步） ==========

    /// host 当前冷却截止剩余时长；无条目或已过期返回 ZERO
    fn cooldown_remaining(d: &HostDiscipline, host: &str) -> Duration {
        let now = Instant::now();
        d.state
            .get(host)
            .and_then(|st| st.cooldown_until)
            .and_then(|until| until.checked_duration_since(now))
            .unwrap_or_default()
    }

    #[test]
    fn state_absent_for_unknown_host() {
        let d = discipline();
        assert!(
            d.state.get("x.example.com").is_none(),
            "未知 host 不应被预先建档"
        );
    }

    #[test]
    fn next_allowed_scheduled_min_interval_after_forbidden_creation() {
        // Forbidden 路径建档（next_allowed=now+min_interval、无冷却），
        // 时刻表应把下一个放行预约在 min_interval 之后
        let d = HostDiscipline::new(Duration::from_millis(300), Duration::from_secs(60), 3);
        d.observe(
            "i.example.com",
            ObserveOutcome::Forbidden { challenge: false },
        );
        let now = Instant::now();
        let next_allowed = d.state.get("i.example.com").unwrap().next_allowed;
        let remaining = next_allowed.checked_duration_since(now).unwrap_or_default();
        assert!(
            (Duration::from_millis(200)..=Duration::from_millis(300)).contains(&remaining),
            "next_allowed should be ~min_interval ahead, got {remaining:?}"
        );
    }

    #[test]
    fn rate_limited_writes_cooldown_with_retry_after() {
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_secs(60), 3);
        d.observe(
            "rl.example.com",
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        );
        let wait = cooldown_remaining(&d, "rl.example.com");
        assert!(
            wait >= Duration::from_secs(29),
            "RateLimited should write ~30s cooldown, got {wait:?}"
        );
    }

    #[test]
    fn rate_limited_without_retry_after_uses_default() {
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_secs(20), 3);
        d.observe(
            "rl.example.com",
            ObserveOutcome::RateLimited { retry_after: None },
        );
        let wait = cooldown_remaining(&d, "rl.example.com");
        assert!(
            wait >= Duration::from_secs(19),
            "RateLimited without header should fall back to cooldown_default, got {wait:?}"
        );
    }

    #[test]
    fn rate_limited_retry_after_below_default_takes_max() {
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_secs(20), 3);
        d.observe(
            "rl.example.com",
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(2)),
            },
        );
        let wait = cooldown_remaining(&d, "rl.example.com");
        assert!(
            wait >= Duration::from_secs(19),
            "cooldown should be max(retry_after, default)=20s, got {wait:?}"
        );
    }

    #[test]
    fn gate_wait_clamped_to_five_seconds() {
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_secs(60), 3);
        d.observe(
            "clamped.example.com",
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(600)),
            },
        );
        let raw = cooldown_remaining(&d, "clamped.example.com");
        assert!(
            raw > MAX_GATE_WAIT,
            "raw cooldown should exceed the clamp for this scenario"
        );
        assert_eq!(
            raw.min(MAX_GATE_WAIT),
            MAX_GATE_WAIT,
            "gate wait must be clamped to 5s"
        );
    }

    // ========== 裸 403 连击 → tls_suspect → Success 清除 ==========

    #[test]
    fn bare403_streak_triggers_tls_suspect_then_success_clears() {
        let d = discipline();
        d.observe(
            "waf.example.com",
            ObserveOutcome::Forbidden { challenge: false },
        );
        d.observe(
            "waf.example.com",
            ObserveOutcome::Forbidden { challenge: false },
        );
        assert!(
            !d.tls_suspect("waf.example.com"),
            "2 strikes below threshold=3 must not set tls_suspect"
        );
        d.observe(
            "waf.example.com",
            ObserveOutcome::Forbidden { challenge: false },
        );
        assert!(
            d.tls_suspect("waf.example.com"),
            "3 consecutive bare 403s must set tls_suspect"
        );
        d.observe("waf.example.com", ObserveOutcome::Success);
        assert!(
            !d.tls_suspect("waf.example.com"),
            "Success must clear tls_suspect"
        );
    }

    #[test]
    fn challenge403_and_failure_do_not_count_streak() {
        let d = discipline();
        for _ in 0..5 {
            d.observe(
                "c.example.com",
                ObserveOutcome::Forbidden { challenge: true },
            );
            d.observe("c.example.com", ObserveOutcome::Failure);
        }
        assert!(
            !d.tls_suspect("c.example.com"),
            "challenge 403 / Failure must not advance the streak"
        );
    }

    #[test]
    fn success_clears_cooldown() {
        let d = HostDiscipline::new(Duration::from_millis(1), Duration::from_secs(60), 3);
        d.observe(
            "ok.example.com",
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        );
        assert!(
            cooldown_remaining(&d, "ok.example.com") >= Duration::from_secs(29),
            "precondition: host should be cooling down"
        );
        d.observe("ok.example.com", ObserveOutcome::Success);
        let st = d.state.get("ok.example.com").unwrap();
        assert!(st.cooldown_until.is_none(), "Success must clear cooldown");
        assert!(
            st.next_allowed <= Instant::now(),
            "Success must collapse the schedule so later gates are not held back"
        );
    }

    // ========== 并发间隔语义：批内相邻放行间隔 ≥ min_interval ==========

    #[tokio::test]
    async fn concurrent_gate_release_times_spaced_by_min_interval() {
        // gate 的检查与写入若在 sleep 两侧分离，并发同 host 请求会读到同一
        // 快照、等同一时长后同时放行——批内间隔 0，限速退化为每窗口放一批。
        // 预约时刻表语义下 N 个并发 gate 必须串行领取放行时刻：
        // 相邻实际放行时刻差 ≥ min_interval（容差覆盖调度抖动），
        // 完成总耗时 ≥ (N-1)×min_interval。
        let interval = Duration::from_millis(40);
        let d = Arc::new(HostDiscipline::new(interval, Duration::from_secs(60), 3));
        let n = 12u32;

        let mut handles = Vec::new();
        for _ in 0..n {
            let dd = Arc::clone(&d);
            handles.push(tokio::spawn(async move {
                dd.gate("burst.example.com").await;
                Instant::now()
            }));
        }
        let mut released: Vec<Instant> = Vec::new();
        for handle in handles {
            released.push(handle.await.expect("task join failed"));
        }
        released.sort();

        let jitter = Duration::from_millis(10);
        for (i, pair) in released.windows(2).enumerate() {
            let gap = pair[1].duration_since(pair[0]);
            assert!(
                gap + jitter >= interval,
                "release #{i} to #{} gap {gap:?} must be ~min_interval {interval:?}",
                i + 1
            );
        }
        let total = released
            .last()
            .unwrap()
            .duration_since(*released.first().unwrap());
        assert!(
            total + jitter >= interval * (n - 1),
            "{n} concurrent gates must span >= (N-1)×min_interval, got {total:?}"
        );
    }

    // ========== 并发冒烟：gate/observe/tls_suspect 混合并发无死锁 ==========

    #[tokio::test]
    async fn concurrent_gate_and_observe_no_deadlock() {
        let d = Arc::new(HostDiscipline::new(
            Duration::from_millis(1),
            Duration::from_millis(1),
            3,
        ));
        d.gate("hot.example.com").await;
        let mut handles = Vec::new();
        for _ in 0..8 {
            let dd = Arc::clone(&d);
            handles.push(tokio::spawn(async move {
                for _ in 0..25 {
                    dd.gate("hot.example.com").await;
                    dd.observe(
                        "hot.example.com",
                        ObserveOutcome::Forbidden { challenge: false },
                    );
                    let _ = dd.tls_suspect("hot.example.com");
                }
            }));
        }
        // DashMap 同 shard 守卫误持有可能死锁——以超时显性化而非挂死测试
        tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(handles))
            .await
            .expect("concurrent gate/observe/tls_suspect must not deadlock");
    }

    // ==== 评审 LOW：state 惰性清扫 ====

    /// 过期中性条目被清扫回收；携带纪律信息的条目（tls_suspect/连击/冷却）保留
    #[test]
    fn test_sweep_removes_stale_keeps_disciplined_entries() {
        let d = HostDiscipline::with_defaults();
        let now = Instant::now();
        // 过期中性条目：等价于"从未出现过"
        d.state.insert(
            "stale.example".to_string(),
            HostState {
                next_allowed: now - Duration::from_secs(1),
                cooldown_until: None,
                bare403_streak: 0,
                tls_suspect: false,
            },
        );
        // TLS 可疑条目：永不过期（只能被 Success 清除）
        d.state.insert(
            "suspect.example".to_string(),
            HostState {
                next_allowed: now - Duration::from_secs(1),
                cooldown_until: None,
                bare403_streak: 0,
                tls_suspect: true,
            },
        );
        // 冷却中条目：冷却期内不回收
        d.state.insert(
            "cooldown.example".to_string(),
            HostState {
                next_allowed: now - Duration::from_secs(1),
                cooldown_until: Some(now + Duration::from_secs(30)),
                bare403_streak: 0,
                tls_suspect: false,
            },
        );

        d.sweep_now();

        assert!(
            !d.state.contains_key("stale.example"),
            "stale entry must be evicted"
        );
        assert!(
            d.state.contains_key("suspect.example"),
            "tls_suspect entry must persist"
        );
        assert!(
            d.state.contains_key("cooldown.example"),
            "cooling entry must persist until cooldown expires"
        );
    }

    /// Success 后收敛为中性且时刻表已过期的条目会被下一次清扫回收
    #[test]
    fn test_sweep_collects_success_reset_entry() {
        let d = HostDiscipline::with_defaults();
        let now = Instant::now();
        // Success 之后的收敛态：next_allowed 被压回 now、其余全中性
        d.state.insert(
            "recovered.example".to_string(),
            HostState {
                next_allowed: now,
                cooldown_until: None,
                bare403_streak: 0,
                tls_suspect: false,
            },
        );
        // next_allowed == now 满足 is_stale（<= now），但先强制触发清扫
        d.sweep_now();
        assert!(
            !d.state.contains_key("recovered.example"),
            "fully reset entry is equivalent to absent and must be collected"
        );
    }
}
