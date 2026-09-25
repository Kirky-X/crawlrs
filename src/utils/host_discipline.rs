// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-host 纪律组件
//!
//! 每个 host 独立维护三件事：
//! 1. **礼貌限速**：`gate` 保证同 host 相邻请求间隔 ≥ `min_interval`
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
//! 见 `host_needs_browser` 的过期清除路径）。`gate` 内部先在独立作用域
//! 计算 `pending_wait`（守卫随即释放），再 sleep，最后 `entry` 写入；
//! `observe` 各分支的 `get_mut`/`entry` 守卫作用域内无二次 map 访问。

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

/// 单个 host 的纪律状态
#[derive(Debug, Clone)]
struct HostState {
    /// 上次放行时刻（`gate` 尾部刷新）
    last_request: Instant,
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

    /// 通过纪律门：必要时等待，然后记录本次放行
    ///
    /// 距 `last_request` 不足 `min_interval` 或处于 429 冷却期时 sleep 到
    /// 两者的较晚到期点，单次等待钳制 ≤ [`MAX_GATE_WAIT`]，随后刷新
    /// `last_request`。等待是有界的——冷却期长于 5s 时也只等 5s：
    /// 冷却是礼貌信号而非硬门禁，不能把 worker 无限期占住。
    pub async fn gate(&self, host: &str) {
        let wait = self.pending_wait(host, Instant::now()).min(MAX_GATE_WAIT);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        // get 守卫已在 pending_wait 内释放，此处 entry 写入无死锁面
        self.state
            .entry(host.to_string())
            .and_modify(|st| st.last_request = Instant::now())
            .or_insert(HostState {
                last_request: Instant::now(),
                cooldown_until: None,
                bare403_streak: 0,
                tls_suspect: false,
            });
    }

    /// 记录一次请求结果（语义见 [`ObserveOutcome`]）
    ///
    /// 对未经过 `gate` 的 host 同样生效（`entry` 惰性建档），
    /// 调用方无需保证 observe 先于 gate 发生。
    pub fn observe(&self, host: &str, outcome: ObserveOutcome) {
        match outcome {
            ObserveOutcome::Success => {
                if let Some(mut st) = self.state.get_mut(host) {
                    st.cooldown_until = None;
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
                        last_request: now,
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
                        last_request: Instant::now(),
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

    /// 计算当前需要等待的时长（未钳制）
    ///
    /// 取冷却剩余与 min_interval 剩余的较大者；get 守卫在返回前释放，
    /// 调用方随后可安全写 map。
    fn pending_wait(&self, host: &str, now: Instant) -> Duration {
        match self.state.get(host) {
            Some(st) => {
                let cooldown_wait = st
                    .cooldown_until
                    .and_then(|until| until.checked_duration_since(now))
                    .unwrap_or_default();
                let interval_wait = now
                    .checked_duration_since(st.last_request)
                    .and_then(|since| self.min_interval.checked_sub(since))
                    .unwrap_or_default();
                cooldown_wait.max(interval_wait)
            }
            None => Duration::ZERO,
        }
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

    // ========== pending_wait / 钳制（纯同步，不 sleep） ==========

    #[test]
    fn pending_wait_zero_for_unknown_host() {
        let d = discipline();
        assert_eq!(
            d.pending_wait("x.example.com", Instant::now()),
            Duration::ZERO
        );
    }

    #[test]
    fn pending_wait_covers_min_interval_after_first_gate_state() {
        // Forbidden 路径建档（last_request=now、无冷却），
        // pending_wait 应由 min_interval 剩余量主导
        let d = HostDiscipline::new(Duration::from_millis(300), Duration::from_secs(60), 3);
        d.observe(
            "i.example.com",
            ObserveOutcome::Forbidden { challenge: false },
        );
        let wait = d.pending_wait("i.example.com", Instant::now());
        assert!(
            (Duration::from_millis(200)..=Duration::from_millis(300)).contains(&wait),
            "pending_wait should be ~min_interval remainder, got {wait:?}"
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
        let wait = d.pending_wait("rl.example.com", Instant::now());
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
        let wait = d.pending_wait("rl.example.com", Instant::now());
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
        let wait = d.pending_wait("rl.example.com", Instant::now());
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
        assert!(
            d.pending_wait("clamped.example.com", Instant::now()) > MAX_GATE_WAIT,
            "raw pending_wait should exceed the clamp for this scenario"
        );
        let wait = d
            .pending_wait("clamped.example.com", Instant::now())
            .min(MAX_GATE_WAIT);
        assert_eq!(wait, MAX_GATE_WAIT, "gate wait must be clamped to 5s");
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
            d.pending_wait("ok.example.com", Instant::now()) >= Duration::from_secs(29),
            "precondition: host should be cooling down"
        );
        d.observe("ok.example.com", ObserveOutcome::Success);
        let wait = d.pending_wait("ok.example.com", Instant::now());
        assert!(
            wait < Duration::from_millis(100),
            "Success must clear cooldown, got {wait:?}"
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
}
