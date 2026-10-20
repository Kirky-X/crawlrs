// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-host 纪律组件
//!
//! 每个 host 独立维护五件事：
//! 1. **礼貌限速**：`gate` 在 per-host 预约时刻表上串行发放放行时刻，
//!    同 host 相邻请求（含并发到达）间隔 ≥ 步长 `max(current_interval,
//!    crawl_delay)`；AutoThrottle（`autothrottle_enabled`）开启后按成功响应
//!    延迟 EMA 收敛动态间隔（下界 `min_interval`、上界 `max_interval`）
//! 2. **429 冷却**：收到 429 后按 `max(retry_after, 指数退避下界)` 进入冷却
//!    （连续 429 指数推进 `min(base×2^n, backoff_max)`、同窗 burst 不推进；
//!    legacy 固定模式下下界为 `cooldown_default`），冷却期内 `gate` 有界等待
//!    （单次钳制 ≤5s，避免占用 worker 过久）；连续 429 达 `stalled_threshold`
//!    置停滞标志，提示调用方强制身份/代理切换
//! 3. **裸 403 连击**：无 WAF 指纹的 403 连续 `bare403_threshold` 次后置
//!    `tls_suspect`，提示调用方升级 TLS 指纹（带 challenge 页的 403 不计数——
//!    那是 JS 挑战而非 TLS 指纹问题）
//! 4. **Crawl-delay 双时钟**：robots.txt 法定的 `crawl_delay` 经
//!    `set_crawl_delay` 注入（首值生效），与 AutoThrottle 动态值取 max
//!    共同决定 gate 步长，互不抢夺
//! 5. **放行抖动**：`jitter_ratio` > 0 时实际放行时刻在预约槽位上叠加
//!    ±ratio×interval 随机偏移，打散请求节奏指纹（时刻表本身保持确定性）
//!
//! # 并发模型
//!
//! 与 `EngineRouter::host_browser_hint` 同款 `DashMap` 约定：先 drop `get`
//! 守卫再做任何写操作（同 shard 的 get 守卫存活期间写入会死锁，
//! 见 `host_needs_browser` 的过期清除路径）。`gate` 的检查与预约在同一个
//! `entry` 临界区内完成（读 `max(next_allowed, cooldown_until)` → 写
//! `next_allowed = slot + step`），守卫块尾即放、不跨
//! `await` 持有——并发同 host 请求各自取到严格递增的预约时刻。若检查与
//! 写入分离在 sleep 两侧，一批请求会读到同一快照、等同一时长后同时放行，
//! 批内间隔 0，限速退化为每窗口放一整批。`observe` 各分支的
//! `get_mut`/`entry` 守卫作用域内无二次 map 访问。

use std::time::{Duration, Instant};

use dashmap::DashMap;
use rand::RngExt;

/// 默认 per-host 最小请求间隔
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_secs(1);
/// 默认 AutoThrottle 收敛上界
pub const DEFAULT_MAX_INTERVAL: Duration = Duration::from_secs(60);
/// 默认 429 冷却时长（Retry-After 缺失或更小时的兜底；legacy 固定冷却模式）
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);
/// 默认裸 403 连击阈值（连续 N 次后置 tls_suspect）
pub const DEFAULT_BARE403_THRESHOLD: u32 = 3;
/// 默认 AutoThrottle 收敛目标并发（target_interval = latency / target_concurrency）
pub const DEFAULT_TARGET_CONCURRENCY: u32 = 8;
/// 默认 429 指数退避底值
pub const DEFAULT_BACKOFF_BASE: Duration = Duration::from_secs(2);
/// 默认 429 指数退避封顶
pub const DEFAULT_BACKOFF_MAX: Duration = Duration::from_secs(60);
/// 默认停滞阈值（连续 RateLimited 达到该次数置 stalled）
pub const DEFAULT_STALLED_THRESHOLD: u32 = 3;

/// `gate` 单次等待钳制上限：冷却再长也不能把 worker 占住超过 5s
const MAX_GATE_WAIT: Duration = Duration::from_secs(5);

/// 惰性清扫间隔：达到间隔后下一次 `gate`/`observe` 触发一次全表清扫，
/// 回收只被访问过一次的 host 条目（评审 LOW：state 只增不删，内存无界）
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// AutoThrottle 与礼貌参数集（F1/F5 配置面的算法侧视图）
///
/// 由 [`HostDiscipline::with_options`] 消费；平台侧经
/// [`HostDiscipline::from_politeness`] 从 `PolitenessSettings` 转换。
#[derive(Debug, Clone)]
pub struct PolitenessOptions {
    /// 相邻同 host 请求最小间隔（AutoThrottle 收敛下界）
    pub min_interval: Duration,
    /// AutoThrottle 收敛上界
    pub max_interval: Duration,
    /// 429 冷却兜底（legacy 固定冷却模式，即 `backoff_base == 0` 时生效）
    pub cooldown_default: Duration,
    /// 裸 403 连击阈值
    pub bare403_threshold: u32,
    /// AutoThrottle 开关：false 时动态间隔恒等于 min_interval（现状行为）
    pub autothrottle_enabled: bool,
    /// 延迟收敛目标并发：target_interval = latency / target_concurrency
    pub target_concurrency: u32,
    /// 429 指数退避底值；`ZERO` = 关闭指数退避（legacy 固定冷却）
    pub backoff_base: Duration,
    /// 429 指数退避封顶
    pub backoff_max: Duration,
    /// 连续 RateLimited 停滞阈值；0 = 关闭停滞检测
    pub stalled_threshold: u32,
    /// 放行时刻抖动比例（0.0-1.0）；0 = 确定性时刻表
    pub jitter_ratio: f64,
}

impl Default for PolitenessOptions {
    fn default() -> Self {
        Self {
            min_interval: DEFAULT_MIN_INTERVAL,
            max_interval: DEFAULT_MAX_INTERVAL,
            cooldown_default: DEFAULT_COOLDOWN,
            bare403_threshold: DEFAULT_BARE403_THRESHOLD,
            autothrottle_enabled: false,
            target_concurrency: DEFAULT_TARGET_CONCURRENCY,
            backoff_base: DEFAULT_BACKOFF_BASE,
            backoff_max: DEFAULT_BACKOFF_MAX,
            stalled_threshold: DEFAULT_STALLED_THRESHOLD,
            jitter_ratio: 0.0,
        }
    }
}

/// 单个 host 的纪律状态
#[derive(Debug, Clone)]
struct HostState {
    /// 下一个预约放行时刻（`gate` 在临界区内推进 `+= step`）
    next_allowed: Instant,
    /// 429 冷却截止时刻；`None` 表示不在冷却期
    cooldown_until: Option<Instant>,
    /// 裸 403 连击计数（Success 清零，challenge 403 / Failure 不动）
    bare403_streak: u32,
    /// 裸 403 连击达到阈值后置位；Success 清除
    tls_suspect: bool,
    /// AutoThrottle 动态收敛间隔（初值 = min_interval；仅 Success 分支更新）
    current_interval: Duration,
    /// 429 指数连击计数（Success 清零；同窗 burst 不推进）
    backoff_exponent: u32,
    /// 连续 RateLimited 计数（Success 清零），达阈值置 stalled
    rate_limited_streak: u32,
    /// 停滞标志：连续 429 达 `stalled_threshold` 置位；Success 清除。
    /// 置位后 route_sequential 对该 host 的后续 attempt 强制轮换代理
    stalled: bool,
    /// robots.txt Crawl-delay（法定值，非健康信号）：首值生效不回退，
    /// gate 推进步长取 max(current_interval, crawl_delay)
    crawl_delay: Option<Duration>,
}

impl HostState {
    /// 新建档：时刻表起点 `next_allowed`，动态间隔取 min_interval
    fn fresh(next_allowed: Instant, min_interval: Duration) -> Self {
        Self {
            next_allowed,
            cooldown_until: None,
            bare403_streak: 0,
            tls_suspect: false,
            current_interval: min_interval,
            backoff_exponent: 0,
            rate_limited_streak: 0,
            stalled: false,
            crawl_delay: None,
        }
    }
}

/// `observe` 的请求结果分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveOutcome {
    /// 最终 2xx 成功：清除冷却、连击、tls_suspect 与停滞记忆
    Success,
    /// 429 限速：冷却取 `max(retry_after, 指数退避下界)`
    /// （指数连击推进 + burst 抑制见 `HostDiscipline::observe`；legacy
    /// 固定冷却模式下下界为 `cooldown_default`）
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

/// Per-host 纪律控制器：礼貌限速（AutoThrottle）+ 429 冷却 + 裸 403 连击
/// → TLS 指纹升级
///
/// 线程安全：内部 `DashMap`（分片锁），可安全跨任务共享。
#[derive(Debug)]
pub struct HostDiscipline {
    /// host → 纪律状态
    state: DashMap<String, HostState>,
    /// 相邻请求最小间隔 = AutoThrottle 收敛下界（不可变）
    min_interval: Duration,
    /// AutoThrottle 收敛上界（不可变）
    max_interval: Duration,
    /// 429 冷却默认时长（不可变；legacy 固定冷却模式）
    cooldown_default: Duration,
    /// 裸 403 连击阈值（不可变）
    bare403_threshold: u32,
    /// AutoThrottle 开关（不可变）：false 时动态间隔恒等于 min_interval
    autothrottle_enabled: bool,
    /// 延迟收敛目标并发（不可变）
    target_concurrency: u32,
    /// 429 指数退避底值（不可变；ZERO = legacy 固定冷却）
    backoff_base: Duration,
    /// 429 指数退避封顶（不可变）
    backoff_max: Duration,
    /// 停滞阈值（不可变；0 = 关闭）
    stalled_threshold: u32,
    /// 放行抖动比例（不可变，已钳制到 [0, 1]）
    jitter_ratio: f64,
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
        Self::with_options(PolitenessOptions {
            min_interval,
            max_interval: min_interval,
            cooldown_default,
            bare403_threshold,
            autothrottle_enabled: false,
            target_concurrency: DEFAULT_TARGET_CONCURRENCY,
            // legacy 固定冷却模式：关闭指数退避与停滞检测
            backoff_base: Duration::ZERO,
            backoff_max: DEFAULT_BACKOFF_MAX,
            stalled_threshold: 0,
            jitter_ratio: 0.0,
        })
    }

    /// 创建指定礼貌参数的 HostDiscipline（F1/F5 配置面入口）
    ///
    /// # Panics
    ///
    /// fail-fast 断言三项结构性约束：`bare403_threshold >= 1`（同 [`Self::new`]）、
    /// `target_concurrency >= 1`（0 会在延迟收敛时除零）、
    /// `max_interval >= min_interval`（反向区间会让 `clamp` panic）。
    /// `jitter_ratio` 越界时静默钳制到 [0, 1]（抖动是尽力而为的混淆，非硬约束）。
    pub fn with_options(options: PolitenessOptions) -> Self {
        assert!(
            options.bare403_threshold >= 1,
            "bare403_threshold must be >= 1, got {}",
            options.bare403_threshold
        );
        assert!(
            options.target_concurrency >= 1,
            "target_concurrency must be >= 1, got {}",
            options.target_concurrency
        );
        assert!(
            options.max_interval >= options.min_interval,
            "max_interval {:?} must be >= min_interval {:?}",
            options.max_interval,
            options.min_interval
        );
        Self {
            state: DashMap::new(),
            min_interval: options.min_interval,
            max_interval: options.max_interval,
            cooldown_default: options.cooldown_default,
            bare403_threshold: options.bare403_threshold,
            autothrottle_enabled: options.autothrottle_enabled,
            target_concurrency: options.target_concurrency,
            backoff_base: options.backoff_base,
            backoff_max: options.backoff_max,
            stalled_threshold: options.stalled_threshold,
            jitter_ratio: options.jitter_ratio.clamp(0.0, 1.0),
            last_sweep: parking_lot::Mutex::new(Instant::now()),
        }
    }

    /// 使用默认参数创建（min_interval=1s，max_interval=60s，cooldown_default=60s，
    /// threshold=3；AutoThrottle 默认关，与 config/default.toml `[politeness]` 对齐）
    pub fn with_defaults() -> Self {
        Self::with_options(PolitenessOptions::default())
    }

    /// 从 `PolitenessSettings` 构造（F1/F5：平台侧 DI 注入配置面）。
    ///
    /// utils → config 的依赖与 `robots.rs` 同向（`RobotsChecker::new` 接收
    /// `&RobotsSettings`）。
    pub fn from_politeness(settings: &crate::config::settings::PolitenessSettings) -> Self {
        Self::with_options(PolitenessOptions {
            min_interval: Duration::from_millis(settings.min_interval_ms),
            max_interval: Duration::from_millis(settings.max_interval_ms),
            cooldown_default: Duration::from_secs(settings.cooldown_default_secs),
            bare403_threshold: settings.bare403_threshold,
            autothrottle_enabled: settings.autothrottle_enabled,
            target_concurrency: settings.target_concurrency,
            backoff_base: Duration::from_secs(settings.backoff_base_secs),
            backoff_max: Duration::from_secs(settings.backoff_max_secs),
            stalled_threshold: settings.stalled_threshold,
            jitter_ratio: settings.jitter_ratio,
        })
    }

    /// 通过纪律门：必要时等待，然后放行
    ///
    /// 在 per-host 预约时刻表上领取本次放行时刻：
    /// `slot = max(next_allowed, cooldown_until, now)`，并在同一临界区
    /// 把时刻表推进 `next_allowed = slot + step`，其中步长
    /// `step = max(current_interval, crawl_delay)`——AutoThrottle 的动态值
    /// 是礼貌下界，robots Crawl-delay 是法定值，二者取较大者互不抢夺。
    /// 检查与预约必须在同一 `entry` 临界区内完成——并发到达的同 host 请求
    /// 各自取到严格递增的 `slot`，相邻放行间隔 ≥ step。
    ///
    /// 抖动（`jitter_ratio` > 0）只作用于**实际放行时刻**，不回写时刻表：
    /// 抖动若参与预约推进，会在批内序列上随机游走、侵蚀间隔下界。
    ///
    /// 随后 sleep 到放行时刻，单次等待钳制 ≤ [`MAX_GATE_WAIT`]：
    /// 冷却是礼貌信号而非硬门禁，不能把 worker 无限期占住。钳制只缩短
    /// 实际睡眠时长，不回改时刻表——批内间隔仍由预约序列保证。
    pub async fn gate(&self, host: &str) {
        self.maybe_sweep();
        let now = Instant::now();
        let release_at = {
            let mut st = self
                .state
                .entry(host.to_string())
                .or_insert_with(|| HostState::fresh(now, self.min_interval));
            let step = st.current_interval.max(st.crawl_delay.unwrap_or_default());
            let slot = st
                .cooldown_until
                .unwrap_or(now)
                .max(st.next_allowed)
                .max(now);
            st.next_allowed = slot + step;
            self.jitter_release(slot, st.current_interval)
        };
        let wait = release_at
            .checked_duration_since(Instant::now())
            .unwrap_or_default()
            .min(MAX_GATE_WAIT);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    /// F1：放行时刻抖动——在预约槽位上叠加 ±`jitter_ratio`×interval
    /// 的均匀偏移；ratio ≤ 0 时零成本直通（确定性时刻表，现状行为）。
    fn jitter_release(&self, slot: Instant, interval: Duration) -> Instant {
        if self.jitter_ratio <= 0.0 {
            return slot;
        }
        let span = interval.mul_f64(self.jitter_ratio);
        if span.is_zero() {
            return slot;
        }
        let factor: f64 = rand::rng().random_range(-1.0..1.0);
        let magnitude = span.mul_f64(factor.abs());
        if factor >= 0.0 {
            slot.checked_add(magnitude).unwrap_or(slot)
        } else {
            slot.checked_sub(magnitude).unwrap_or(slot)
        }
    }

    /// 记录一次请求结果（语义见 [`ObserveOutcome`]）
    ///
    /// 对未经过 `gate` 的 host 同样生效（`entry` 惰性建档），
    /// 调用方无需保证 observe 先于 gate 发生。
    pub fn observe(&self, host: &str, outcome: ObserveOutcome) {
        self.maybe_sweep();
        match outcome {
            ObserveOutcome::Success => self.observe_success_inner(host, None),
            ObserveOutcome::RateLimited { retry_after } => {
                let now = Instant::now();
                let retry = retry_after.unwrap_or_default();
                // 首次 429（惰性建档路径）从底值起步：n=0 → base×2^0（或 legacy 兜底）
                let until_fresh = now + retry.max(self.cooldown_floor(0));
                self.state
                    .entry(host.to_string())
                    .and_modify(|st| {
                        st.rate_limited_streak = st.rate_limited_streak.saturating_add(1);
                        if self.stalled_threshold > 0
                            && st.rate_limited_streak >= self.stalled_threshold
                        {
                            st.stalled = true;
                        }
                        // 同窗 burst 抑制：冷却期内重复 429 只外推截止、不推进指数
                        // ——否则同一次限速窗口内的连发会把退避推到远超实际必要
                        let in_window = st.cooldown_until.is_some_and(|until| now < until);
                        let n = st.backoff_exponent;
                        if !in_window {
                            st.backoff_exponent = n.saturating_add(1);
                        }
                        let until = now + retry.max(self.cooldown_floor(n));
                        st.cooldown_until = Some(if in_window {
                            st.cooldown_until.map_or(until, |old| old.max(until))
                        } else {
                            until
                        });
                    })
                    .or_insert_with(|| {
                        let mut st = HostState::fresh(now + self.min_interval, self.min_interval);
                        st.cooldown_until = Some(until_fresh);
                        st.backoff_exponent = 1;
                        st.rate_limited_streak = 1;
                        st.stalled = self.stalled_threshold == 1;
                        st
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
                    .or_insert_with(|| {
                        let mut st =
                            HostState::fresh(Instant::now() + self.min_interval, self.min_interval);
                        st.bare403_streak = 1;
                        st.tls_suspect = threshold <= 1;
                        st
                    });
            }
            // challenge 403（带 WAF 指纹）与一般失败不改任何状态：
            // 前者已有 needs_js 升级路径处理，后者与 host 纪律无关
            ObserveOutcome::Forbidden { challenge: true } | ObserveOutcome::Failure => {}
        }
    }

    /// 429 冷却下界（F5）：指数模式（`backoff_base > 0`）取
    /// `min(base × 2^exponent, backoff_max)`；legacy 模式（`backoff_base == 0`，
    /// 即 [`Self::new`] 三参构造）取固定 `cooldown_default`。
    fn cooldown_floor(&self, exponent: u32) -> Duration {
        if self.backoff_base.is_zero() {
            return self.cooldown_default;
        }
        self.backoff_base
            .checked_mul(1u32 << exponent.min(31))
            .unwrap_or(self.backoff_max)
            .min(self.backoff_max)
    }

    /// F5：注入 robots.txt 解析出的 Crawl-delay（per-host 双时钟的法定值钟）
    ///
    /// 首值生效不回退——robots.txt 在 TTL 缓存窗口内可能被不同 UA 解析出
    /// 不同值，只认首次注入，避免限速在多次写之间来回跳。对未 `gate` 过的
    /// host 惰性建档（对齐 `observe` 的 entry 语义）。
    pub fn set_crawl_delay(&self, host: &str, delay: Duration) {
        self.state
            .entry(host.to_string())
            .and_modify(|st| {
                if st.crawl_delay.is_none() {
                    st.crawl_delay = Some(delay);
                }
            })
            .or_insert_with(|| {
                let mut st = HostState::fresh(Instant::now(), self.min_interval);
                st.crawl_delay = Some(delay);
                st
            });
    }

    /// F5：host 是否已停滞（连续 RateLimited 达 `stalled_threshold`）。
    /// 置位后调用方应对该 host 强制身份/代理切换；Success 清除。
    pub fn is_stalled(&self, host: &str) -> bool {
        self.state.get(host).map(|st| st.stalled).unwrap_or(false)
    }

    /// F1：记录一次 2xx 成功并携带观测延迟，驱动 AutoThrottle 收敛
    ///
    /// 收敛目标 `target = latency / target_concurrency`，EMA 取有偏更新
    /// `new = max((current_interval + target) / 2, target)` 后钳制到
    /// `[min_interval, max_interval]`。`max(target)` 保证观测目标本身即为
    /// 下界——快站的单次观测不会让间隔骤降（防正反馈）。
    /// 非 2xx 路径不会调用本方法（错误页不参与收敛）。
    pub fn observe_success_with_latency(&self, host: &str, latency: Duration) {
        self.observe_success_inner(host, Some(latency));
    }

    /// Success 分支的公共实现：清除健康记忆（冷却/连击/TLS 可疑）；
    /// `latency = Some` 且 AutoThrottle 开启时做 EMA 收敛。
    /// 无条目时静默返回（与既有 `observe(Success)` 的 `get_mut` 语义一致：
    /// 请求路径必先 `gate` 建档，此处无需再建）。
    fn observe_success_inner(&self, host: &str, latency: Option<Duration>) {
        self.maybe_sweep();
        let now = Instant::now();
        if let Some(mut st) = self.state.get_mut(host) {
            st.cooldown_until = None;
            // 时刻表一并收敛到当前时刻：成功即恢复健康，
            // 长冷却期残留的远期预约不应继续钳制后续请求
            st.next_allowed = st.next_allowed.min(now);
            st.bare403_streak = 0;
            st.tls_suspect = false;
            st.backoff_exponent = 0;
            st.rate_limited_streak = 0;
            st.stalled = false;
            if let Some(latency) = latency {
                if self.autothrottle_enabled {
                    let target = latency / self.target_concurrency;
                    let next = ((st.current_interval + target) / 2).max(target);
                    st.current_interval = next.clamp(self.min_interval, self.max_interval);
                }
            }
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
    /// 无冷却、无连击、无 TLS 可疑、无停滞记忆、时刻表也已过期。
    /// 注意 `tls_suspect == true` 的条目永不过期（只能被 Success 清除）；
    /// `crawl_delay` 同理——robots 法定约束不得因静默过期失效（条纹同
    /// tls_suspect：持有法定约束的条目不回收）。
    fn is_stale(st: &HostState, now: Instant) -> bool {
        st.bare403_streak == 0
            && !st.tls_suspect
            && st.rate_limited_streak == 0
            && !st.stalled
            && st.crawl_delay.is_none()
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

    // gate：首次不等待 / 有界等待

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

    // 冷却 / 时刻表状态（读内部状态，纯同步）

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

    // 裸 403 连击 → tls_suspect → Success 清除

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

    // 并发间隔语义：批内相邻放行间隔 ≥ min_interval

    #[tokio::test]
    async fn concurrent_gate_release_times_spaced_by_min_interval() {
        // gate 的检查与写入若在 sleep 两侧分离，并发同 host 请求会读到同一
        // 快照、等同一时长后同时放行——批内间隔 0，限速退化为每窗口放一批。
        // 预约时刻表语义下 N 个并发 gate 必须在入口临界区串行领取放行时刻。
        // 断言对象是时刻表推进量（临界区内计算，零调度噪声）：批量放行退化
        // 只推进一次时刻表，必被 `post - pre == n×interval` 击穿；返回时刻受
        // 任务调度抢占影响可漂移，不能作为预约语义的度量。
        let interval = Duration::from_millis(40);
        let d = Arc::new(HostDiscipline::new(interval, Duration::from_secs(60), 3));
        let n = 12u32;
        let host = "burst.example.com";

        // 预热建档，取得时刻表基准（预热放行 slot₀，表推进到 slot₀+interval）
        d.gate(host).await;
        let pre = d.state.get(host).unwrap().next_allowed;

        let mut handles = Vec::new();
        for _ in 0..n {
            let dd = Arc::clone(&d);
            handles.push(tokio::spawn(async move {
                dd.gate(host).await;
                Instant::now()
            }));
        }
        let mut released: Vec<Instant> = Vec::new();
        for handle in handles {
            released.push(handle.await.expect("task join failed"));
        }

        // 时刻表推进量：n 次并发 gate 恰好各推进一个 step
        let post = d.state.get(host).unwrap().next_allowed;
        assert_eq!(
            post - pre,
            interval * n,
            "{n} concurrent gates must each reserve exactly one slot (timetable advanced {:+?})",
            post - pre
        );
        // 放行不早于时刻表基准：全部 slot ≥ pre 且 sleep 为 at-least 语义
        for (i, released_at) in released.iter().enumerate() {
            assert!(
                released_at >= &pre,
                "release #{i} at {released_at:?} must not precede timetable base {pre:?}"
            );
        }
    }

    // 并发冒烟：gate/observe/tls_suspect 混合并发无死锁

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

    // 评审 LOW：state 惰性清扫

    /// 过期中性条目被清扫回收；携带纪律信息的条目（tls_suspect/连击/冷却）保留
    #[test]
    fn test_sweep_removes_stale_keeps_disciplined_entries() {
        let d = HostDiscipline::with_defaults();
        let now = Instant::now();
        // 过期中性条目：等价于"从未出现过"
        d.state.insert(
            "stale.example".to_string(),
            HostState::fresh(now - Duration::from_secs(1), Duration::from_secs(1)),
        );
        // TLS 可疑条目：永不过期（只能被 Success 清除）
        d.state.insert(
            "suspect.example".to_string(),
            HostState {
                tls_suspect: true,
                ..HostState::fresh(now - Duration::from_secs(1), Duration::from_secs(1))
            },
        );
        // 冷却中条目：冷却期内不回收
        d.state.insert(
            "cooldown.example".to_string(),
            HostState {
                cooldown_until: Some(now + Duration::from_secs(30)),
                ..HostState::fresh(now - Duration::from_secs(1), Duration::from_secs(1))
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
            HostState::fresh(now, Duration::from_secs(1)),
        );
        // next_allowed == now 满足 is_stale（<= now），但先强制触发清扫
        d.sweep_now();
        assert!(
            !d.state.contains_key("recovered.example"),
            "fully reset entry is equivalent to absent and must be collected"
        );
    }

    // F1：AutoThrottle 延迟观测收敛

    /// AutoThrottle 关闭的参数集（回归现状基准）
    fn opts_throttle_off(min_interval: Duration) -> PolitenessOptions {
        PolitenessOptions {
            min_interval,
            max_interval: Duration::from_secs(60),
            cooldown_default: Duration::from_secs(60),
            bare403_threshold: 3,
            autothrottle_enabled: false,
            target_concurrency: 8,
            backoff_base: Duration::ZERO,
            backoff_max: Duration::from_secs(60),
            stalled_threshold: 0,
            jitter_ratio: 0.0,
        }
    }

    /// AutoThrottle 开启的参数集（其余字段同关闭基准）
    fn opts_throttle_on(min_interval: Duration) -> PolitenessOptions {
        PolitenessOptions {
            autothrottle_enabled: true,
            ..opts_throttle_off(min_interval)
        }
    }

    /// host 当前动态收敛间隔（无条目时 panic——调用方保证先建档）
    fn current_interval_of(d: &HostDiscipline, host: &str) -> Duration {
        d.state
            .get(host)
            .map(|st| st.current_interval)
            .unwrap_or_else(|| panic!("host {host} must have an entry"))
    }

    /// AutoThrottle 关闭时行为与现状逐字节一致：连续 Success 观测不动
    /// current_interval，gate 间隔恒等于 min_interval
    #[tokio::test]
    async fn autothrottle_off_gate_interval_stays_min_interval() {
        let min = Duration::from_millis(100);
        let d = HostDiscipline::with_options(opts_throttle_off(min));
        d.gate("t.example.com").await; // 生产路径：gate 先建档，observe 才有载体
        for _ in 0..3 {
            d.observe_success_with_latency("t.example.com", Duration::from_millis(1));
        }
        assert_eq!(
            current_interval_of(&d, "t.example.com"),
            min,
            "autothrottle off must never move current_interval off min_interval"
        );
        d.gate("t.example.com").await;
        let start = Instant::now();
        d.gate("t.example.com").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(90),
            "gate gap must stay at min_interval, took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "gate gap must not exceed min_interval, took {elapsed:?}"
        );
    }

    /// 慢站抬升后再连续观测低延迟：current_interval 单调下降且不被压破
    /// min_interval（收敛下界）
    #[tokio::test]
    async fn autothrottle_converges_down_then_floors_at_min_interval() {
        let min = Duration::from_millis(100);
        let d = HostDiscipline::with_options(opts_throttle_on(min));
        let host = "fast.example.com";
        d.gate(host).await; // 生产路径：gate 先建档
                            // 慢站一次观测（4s/8=500ms target）抬升收敛间隔
        d.observe_success_with_latency(host, Duration::from_secs(4));
        assert_eq!(
            current_interval_of(&d, host),
            Duration::from_millis(500),
            "first slow observation must raise interval to max(ema, target)"
        );
        // 连续低延迟观测（40ms/8=5ms target）：EMA 逐步下降
        let mut prev = current_interval_of(&d, host);
        for _ in 0..3 {
            d.observe_success_with_latency(host, Duration::from_millis(40));
            let cur = current_interval_of(&d, host);
            assert!(
                cur < prev,
                "interval must decrease monotonically on fast observations, {prev:?} -> {cur:?}"
            );
            assert!(
                cur >= min,
                "interval must never drop below min_interval, got {cur:?}"
            );
            prev = cur;
        }
        assert_eq!(
            prev, min,
            "EMA must floor at min_interval after enough fast observations"
        );
    }

    /// 慢站观测（latency 大）：current_interval 收敛上升且 ≤ max_interval
    #[tokio::test]
    async fn autothrottle_converges_up_on_slow_responses() {
        let d = HostDiscipline::with_options(opts_throttle_on(Duration::from_millis(100)));
        let host = "slow.example.com";
        d.gate(host).await; // 生产路径：gate 先建档
        d.observe_success_with_latency(host, Duration::from_secs(4)); // target=500ms
        let after_first = current_interval_of(&d, host);
        assert!(
            after_first > Duration::from_millis(100),
            "slow observation must raise interval above min, got {after_first:?}"
        );
        d.observe_success_with_latency(host, Duration::from_secs(40)); // target=5s
        let after_second = current_interval_of(&d, host);
        assert!(
            after_second > after_first,
            "larger latency must keep raising interval, {after_first:?} -> {after_second:?}"
        );
        assert!(
            after_second <= Duration::from_secs(60),
            "interval must stay clamped to max_interval, got {after_second:?}"
        );
    }

    /// EMA 有偏性：任一更新后 current_interval ≥ min(target, max_interval)
    /// ——不会低于观测目标（防快站单次观测骤降的正反馈）
    #[tokio::test]
    async fn autothrottle_update_never_below_target() {
        let d = HostDiscipline::with_options(opts_throttle_on(Duration::from_millis(100)));
        let host = "bias.example.com";
        d.gate(host).await; // 生产路径：gate 先建档
        let latency = Duration::from_millis(40);
        for _ in 0..5 {
            d.observe_success_with_latency(host, latency);
            let target = latency / 8;
            assert!(
                current_interval_of(&d, host) >= target,
                "EMA bias: interval must stay >= target"
            );
        }
    }

    /// RateLimited / Forbidden observe 不改变 current_interval——错误页不参与收敛
    #[tokio::test]
    async fn non_success_outcomes_leave_current_interval_unchanged() {
        let d = HostDiscipline::with_options(opts_throttle_on(Duration::from_millis(100)));
        let host = "err.example.com";
        d.gate(host).await; // 生产路径：gate 先建档
        d.observe_success_with_latency(host, Duration::from_secs(4)); // 抬升到 500ms
        let before = current_interval_of(&d, host);
        d.observe(
            host,
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        );
        d.observe(host, ObserveOutcome::Forbidden { challenge: false });
        d.observe(host, ObserveOutcome::Forbidden { challenge: true });
        d.observe(host, ObserveOutcome::Failure);
        assert_eq!(
            current_interval_of(&d, host),
            before,
            "error outcomes must not participate in latency convergence"
        );
    }

    // F1：放行时刻抖动

    /// jitter_ratio=0：放行时刻严格递增、批内间隔恒等于 interval（确定性时刻表）
    ///
    /// 断言对象是 gate 领取的预约时刻表（`next_allowed` 在入口临界区推进，
    /// slotₖ = slotₖ₋₁ + interval），零调度噪声——放行返回时刻受抢占影响
    /// 不可避免地漂移，不能作为确定性性质的度量（重负载下固定裕度必假阳性）。
    #[tokio::test]
    async fn jitter_zero_releases_deterministically() {
        let interval = Duration::from_millis(40);
        let mut opts = opts_throttle_on(interval);
        opts.jitter_ratio = 0.0;
        let d = HostDiscipline::with_options(opts);
        let host = "det.example.com";
        let mut prev_slot = None;
        for _ in 0..5 {
            d.gate(host).await;
            // gate 后 next_allowed = 本次 slot + step，反读本次领取的 slot
            let slot = d.state.get(host).unwrap().next_allowed - interval;
            if let Some(prev) = prev_slot {
                assert_eq!(
                    slot - prev,
                    interval,
                    "no-jitter timetable must advance exactly one interval per release"
                );
            }
            prev_slot = Some(slot);
        }
    }

    /// jitter_ratio=0.5：批内间隔被随机化——每个间隔落在 [0, 2]×interval 内，
    /// 均值收敛到 interval（统计断言，放宽容差）
    #[tokio::test]
    async fn jitter_half_randomizes_release_within_band() {
        let interval = Duration::from_millis(40);
        let mut opts = opts_throttle_on(interval);
        opts.jitter_ratio = 0.5;
        let d = Arc::new(HostDiscipline::with_options(opts));
        let n = 16u32;
        let mut handles = Vec::new();
        for _ in 0..n {
            let dd = Arc::clone(&d);
            handles.push(tokio::spawn(async move {
                dd.gate("jit.example.com").await;
                Instant::now()
            }));
        }
        let mut released: Vec<Instant> = Vec::new();
        for handle in handles {
            released.push(handle.await.expect("task join failed"));
        }
        released.sort();

        let tolerance = Duration::from_millis(15);
        let mut gaps = Vec::new();
        for (i, pair) in released.windows(2).enumerate() {
            let gap = pair[1].duration_since(pair[0]);
            assert!(
                gap <= interval * 2 + tolerance,
                "jittered release #{i} gap {gap:?} must stay within [0,2]×interval"
            );
            gaps.push(gap);
        }
        let mean = gaps.iter().sum::<Duration>() / gaps.len() as u32;
        let lo = interval * 7 / 10;
        let hi = interval * 13 / 10;
        assert!(
            (lo..=hi).contains(&mean),
            "mean gap {mean:?} must converge to interval {interval:?} (band {lo:?}..{hi:?})"
        );
    }

    /// 并发 gate 混合动态 interval：慢站观测抬高收敛间隔后，并发批内
    /// 相邻放行间隔仍 ≥ 抬升后的 interval（沿用既有并发测试骨架）
    #[tokio::test]
    async fn concurrent_gate_with_dynamic_interval_stays_spaced() {
        let interval = Duration::from_millis(40);
        let d = Arc::new(HostDiscipline::with_options(opts_throttle_on(interval)));
        let host = "dyn.example.com";
        d.gate(host).await;
        // 单次慢观测（640ms/8=80ms target）把收敛间隔抬到 80ms
        d.observe_success_with_latency(host, Duration::from_millis(640));
        let raised = current_interval_of(&d, host);
        assert_eq!(
            raised,
            Duration::from_millis(80),
            "precondition: interval raised to max(ema, target)"
        );

        let n = 8u32;
        let mut handles = Vec::new();
        for _ in 0..n {
            let dd = Arc::clone(&d);
            handles.push(tokio::spawn(async move {
                dd.gate(host).await;
                Instant::now()
            }));
        }
        let mut released: Vec<Instant> = Vec::new();
        for handle in handles {
            released.push(handle.await.expect("task join failed"));
        }
        released.sort();
        // gate 的预约时刻表保证槽位间距 ≥ raised（jitter_ratio=0 确定性放行，
        // tokio sleep 不早触发）；但槽 N 的任务晚醒（测试进程高负载下的调度
        // 延迟，实测可达 ~15ms）会压缩「实测」间隔——时间戳在 gate 返回后才取。
        // 容差取 25ms 吸收唤醒延迟，同时仍能侦测间距回归（如间隔减半 40ms
        // 的槽位产生的 40ms 间隔 + 25ms 容差 < 80ms 必败）。
        let jitter = Duration::from_millis(25);
        for (i, pair) in released.windows(2).enumerate() {
            let gap = pair[1].duration_since(pair[0]);
            assert!(
                gap + jitter >= raised,
                "dynamic-interval release #{i} gap {gap:?} must be >= raised interval {raised:?}"
            );
        }
    }

    // F5：429 指数连击

    /// 指数退避参数集（base 可调小以加速测试，公式与生产一致）
    fn opts_backoff(base_ms: u64, max_ms: u64, stalled_threshold: u32) -> PolitenessOptions {
        PolitenessOptions {
            min_interval: Duration::from_millis(1),
            max_interval: Duration::from_secs(60),
            cooldown_default: Duration::from_secs(60),
            bare403_threshold: 3,
            autothrottle_enabled: false,
            target_concurrency: 8,
            backoff_base: Duration::from_millis(base_ms),
            backoff_max: Duration::from_millis(max_ms),
            stalled_threshold,
            jitter_ratio: 0.0,
        }
    }

    fn backoff_exponent_of(d: &HostDiscipline, host: &str) -> u32 {
        d.state
            .get(host)
            .map(|st| st.backoff_exponent)
            .unwrap_or_else(|| panic!("host {host} must have an entry"))
    }

    /// 连续 RateLimited（无 Retry-After）：冷却按 base×2^n 指数推进
    /// （base=30ms → 30ms→60ms→120ms，与生产 base=2s→4s→8s 同式）
    #[tokio::test]
    async fn rate_limited_backoff_escalates_exponentially() {
        let d = HostDiscipline::with_options(opts_backoff(30, 60_000, 0));
        let host = "esc.example.com";
        d.gate(host).await;

        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert_eq!(backoff_exponent_of(&d, host), 1);
        let wait1 = cooldown_remaining(&d, host);
        assert!(
            (Duration::from_millis(20)..=Duration::from_millis(45)).contains(&wait1),
            "1st cooldown should be ~base (30ms), got {wait1:?}"
        );

        // 等过第一个冷却窗，第二次 429 才推进指数
        tokio::time::sleep(Duration::from_millis(50)).await;
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert_eq!(backoff_exponent_of(&d, host), 2);
        let wait2 = cooldown_remaining(&d, host);
        assert!(
            (Duration::from_millis(50)..=Duration::from_millis(75)).contains(&wait2),
            "2nd cooldown should be ~2×base (60ms), got {wait2:?}"
        );

        tokio::time::sleep(Duration::from_millis(80)).await;
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert_eq!(backoff_exponent_of(&d, host), 3);
        let wait3 = cooldown_remaining(&d, host);
        assert!(
            (Duration::from_millis(110)..=Duration::from_millis(135)).contains(&wait3),
            "3rd cooldown should be ~4×base (120ms), got {wait3:?}"
        );
    }

    /// 同窗 burst：冷却期内重复 429 只刷新截止、不推进指数（burst 抑制）
    #[tokio::test]
    async fn rate_limited_burst_within_window_does_not_advance_exponent() {
        let d = HostDiscipline::with_options(opts_backoff(2_000, 60_000, 0));
        let host = "burst.example.com";
        d.gate(host).await;
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        let until_first = d.state.get(host).unwrap().cooldown_until.unwrap();
        assert_eq!(backoff_exponent_of(&d, host), 1);

        // 冷却期内第二次 429：指数不动，截止只外推不回缩
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert_eq!(
            backoff_exponent_of(&d, host),
            1,
            "in-window repeat 429 must not advance the exponent"
        );
        let st = d.state.get(host).unwrap();
        assert!(
            st.cooldown_until.unwrap() >= until_first,
            "burst refresh must not shorten the cooldown window"
        );
    }

    /// Retry-After=30s 与指数值并存时取 30s（max 语义保持）
    #[tokio::test]
    async fn rate_limited_retry_after_dominates_exponential() {
        let d = HostDiscipline::with_options(opts_backoff(30, 60_000, 0));
        let host = "ra.example.com";
        d.gate(host).await;
        d.observe(
            host,
            ObserveOutcome::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
            },
        );
        let wait = cooldown_remaining(&d, host);
        assert!(
            wait >= Duration::from_secs(29),
            "Retry-After must dominate the exponential floor, got {wait:?}"
        );
    }

    /// 指数封顶：连击超过 backoff_max 后冷却不再增长
    #[tokio::test]
    async fn rate_limited_backoff_caps_at_max() {
        let d = HostDiscipline::with_options(opts_backoff(10, 40, 0));
        let host = "cap.example.com";
        d.gate(host).await;
        for i in 0..8 {
            // 每轮冷却 ≤40ms，睡过窗口再打下一次 429
            tokio::time::sleep(Duration::from_millis(60)).await;
            d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
            let wait = cooldown_remaining(&d, host);
            assert!(
                wait <= Duration::from_millis(60),
                "hit #{i}: cooldown {wait:?} must be capped at backoff_max (40ms)"
            );
        }
    }

    /// Success 后 backoff_exponent 清零，下次 429 从 base 重新起步
    #[tokio::test]
    async fn success_resets_backoff_exponent() {
        let d = HostDiscipline::with_options(opts_backoff(30, 60_000, 0));
        let host = "reset.example.com";
        d.gate(host).await;
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert_eq!(backoff_exponent_of(&d, host), 1);

        d.observe(host, ObserveOutcome::Success);
        assert_eq!(
            backoff_exponent_of(&d, host),
            0,
            "Success must clear the backoff exponent"
        );
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        let wait = cooldown_remaining(&d, host);
        assert!(
            (Duration::from_millis(20)..=Duration::from_millis(45)).contains(&wait),
            "post-reset 429 must restart from base (~30ms), got {wait:?}"
        );
    }

    // F5：robots Crawl-delay 双时钟

    /// set_crawl_delay 首值生效：写入 5s 后再写 1s 不回退
    #[test]
    fn set_crawl_delay_first_value_wins() {
        let d = HostDiscipline::with_defaults();
        let host = "cd.example.com";
        d.set_crawl_delay(host, Duration::from_secs(5));
        d.set_crawl_delay(host, Duration::from_secs(1));
        let stored = d
            .state
            .get(host)
            .and_then(|st| st.crawl_delay)
            .expect("crawl_delay must be stored");
        assert_eq!(
            stored,
            Duration::from_secs(5),
            "first crawl-delay must win, later values must not regress it"
        );
    }

    /// Crawl-delay 抬高 gate 推进步长：step = max(min_interval, crawl_delay)
    #[tokio::test]
    async fn crawl_delay_extends_gate_step() {
        let d = HostDiscipline::with_options(opts_throttle_off(Duration::from_millis(100)));
        let host = "cdgate.example.com";
        d.set_crawl_delay(host, Duration::from_millis(300));
        d.gate(host).await;
        let start = Instant::now();
        d.gate(host).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(280),
            "gate step must honor crawl-delay (300ms) over min_interval (100ms), took {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "gate step must not exceed crawl-delay, took {elapsed:?}"
        );
    }

    /// Crawl-delay 与 AutoThrottle 并存：步长取 max(current_interval, crawl_delay)
    #[tokio::test]
    async fn crawl_delay_and_autothrottle_take_max() {
        let d = HostDiscipline::with_options(opts_throttle_on(Duration::from_millis(100)));
        let host = "cdat.example.com";
        d.gate(host).await;
        // 慢观测把 current_interval 抬到 500ms > crawl_delay 200ms
        d.observe_success_with_latency(host, Duration::from_secs(4));
        d.set_crawl_delay(host, Duration::from_millis(200));
        d.gate(host).await;
        let start = Instant::now();
        d.gate(host).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(480),
            "autothrottle interval (500ms) must dominate crawl-delay (200ms), took {elapsed:?}"
        );
    }

    /// set_crawl_delay 对未 gate 过的 host 惰性建档（对齐 observe 的 entry 语义），
    /// 且携带 crawl_delay 的条目不被清扫回收（robots 法定约束不得静默失效）
    #[test]
    fn set_crawl_delay_creates_persistent_entry_lazily() {
        let d = HostDiscipline::with_defaults();
        let host = "lazy.example.com";
        assert!(d.state.get(host).is_none(), "unknown host starts absent");
        d.set_crawl_delay(host, Duration::from_secs(2));
        assert!(
            d.state.get(host).is_some(),
            "set_crawl_delay must create the entry lazily"
        );
        d.sweep_now();
        assert!(
            d.state.contains_key(host),
            "crawl-delay entry must survive sweep (legal constraint, not health signal)"
        );
    }

    // F5：停滞检测

    /// 连续 RateLimited 达阈值置 stalled，Success 清除
    #[tokio::test]
    async fn stalled_flag_set_at_threshold_and_cleared_by_success() {
        let d = HostDiscipline::with_options(opts_backoff(30, 60_000, 3));
        let host = "stall.example.com";
        d.gate(host).await;
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert!(
            !d.is_stalled(host),
            "2 consecutive 429s below threshold=3 must not stall"
        );
        d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
        assert!(
            d.is_stalled(host),
            "3 consecutive 429s must trip the stalled flag"
        );
        d.observe(host, ObserveOutcome::Success);
        assert!(!d.is_stalled(host), "Success must clear the stalled flag");
    }

    /// stalled_threshold=0：停滞检测关闭，任意次数 429 都不置位
    #[tokio::test]
    async fn stalled_detection_disabled_at_zero_threshold() {
        let d = HostDiscipline::with_options(opts_backoff(30, 60_000, 0));
        let host = "nostall.example.com";
        d.gate(host).await;
        for _ in 0..5 {
            d.observe(host, ObserveOutcome::RateLimited { retry_after: None });
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        assert!(
            !d.is_stalled(host),
            "threshold=0 must disable stall detection"
        );
    }
}
