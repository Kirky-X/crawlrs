//! 内存水位调度器：sysinfo 周期采样系统可用内存 → 三档离散水位（Critical/Tight/Abundant）。
//! AtomicU8 镜像读路径无锁（与 circuit_breaker 同风格）；采样与失败计数仅由单一采样任务写入。

#[cfg(all(feature = "engine-moli", feature = "metrics"))]
use metrics::gauge;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 三档水位。数值越大越宽裕；u8 判别值同时是 AtomicU8 镜像的存储值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum MemoryLevel {
    /// 可用内存 < critical 阈值：浏览器系引擎冻结，仅 moli 可承担 JS
    Critical = 0,
    /// 可用内存 < tight 阈值：moli 优先、Playwright 池不扩容
    Tight = 1,
    /// 内存充足：原调度策略不变
    Abundant = 2,
}

impl MemoryLevel {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => MemoryLevel::Critical,
            1 => MemoryLevel::Tight,
            _ => MemoryLevel::Abundant,
        }
    }
}

/// 纯函数三档判定（无 I/O、无时间依赖，可单测）。
/// 语义：available < critical → Critical；available < tight → Tight；否则 Abundant。
pub fn classify(available_mib: u64, critical_mib: u64, tight_mib: u64) -> MemoryLevel {
    if available_mib < critical_mib {
        MemoryLevel::Critical
    } else if available_mib < tight_mib {
        MemoryLevel::Tight
    } else {
        MemoryLevel::Abundant
    }
}

/// 采样失败连续多少次后保守判 Critical。
const FAILURE_THRESHOLD: u8 = 3;

/// 水位调度的引擎加成（Tight 档 moli 优先的偏置量，封顶 255）。
const TIGHT_MOLI_BOOST: u16 = 30;

/// 全局水位镜像单例（DI 装配，router 注入与 BrowserPool 构造时自取共享同一 Arc；
/// 模式与 playwright_pool 的 GLOBAL_BROWSER_POOL 先例一致）。
/// 覆盖式写入而非 first-wins：装配顺序重跑（测试/嵌入场景）不产生静默失效——
/// 后装配者胜，与 `BrowserPool::set_memory_watermark` 显式 setter 语义一致。
static GLOBAL_MEMORY_WATERMARK: parking_lot::RwLock<Option<Arc<MemoryWatermark>>> =
    parking_lot::RwLock::new(None);

/// 装配全局水位镜像（覆盖式：后写者生效）
pub fn set_global(watermark: Arc<MemoryWatermark>) {
    *GLOBAL_MEMORY_WATERMARK.write() = Some(watermark);
}

/// 读取全局水位镜像（未装配 = None）
pub fn get_global() -> Option<Arc<MemoryWatermark>> {
    GLOBAL_MEMORY_WATERMARK.read().clone()
}

/// 浏览器系引擎名（Critical 档对 needs_js 请求归 0 的作用面；moli 除外——Critical 时仅 moli 承担 JS）。
/// wreq/reqwest 为静态 HTTP 引擎不在此列；flaresolverr 三模式均为远程浏览器服务。
/// 维护义务：新增引擎或改引擎 name() 时必须同步此清单（暂以字符串清单为最小实现，
/// trait 级 EngineKind 元数据属后续演进，见收敛叙事）。
const BROWSER_ENGINES: [&str; 5] = [
    "playwright",
    "flaresolverr",
    "flaresolverr_cdp",
    "flaresolverr_tls",
    "mllm",
];

/// 水位前置调度纯函数（无 I/O 与时间依赖）：
///
/// - 非 `needs_js` 请求不受任何水位影响（静态内容与内存压力无关）；
/// - Abundant：不改分（原策略不变）；
/// - Tight：moli +[`TIGHT_MOLI_BOOST`]（封顶 255），其余引擎不改分（Playwright 不加分）；
/// - Critical：浏览器系引擎（除 moli）归 0——仅 moli 保留分数承担 JS；静态引擎不改分
///   （Critical 的静态回退由 router 层 D4 语义处理，不经此函数）。
pub fn apply_memory_policy(score: u8, engine_name: &str, level: MemoryLevel, needs_js: bool) -> u8 {
    if !needs_js || level == MemoryLevel::Abundant {
        return score;
    }
    match level {
        MemoryLevel::Abundant => score,
        MemoryLevel::Tight => {
            if engine_name == "moli" {
                (u16::from(score) + TIGHT_MOLI_BOOST).min(255) as u8
            } else {
                score
            }
        }
        MemoryLevel::Critical => {
            if engine_name == "moli" || !BROWSER_ENGINES.contains(&engine_name) {
                score
            } else {
                0
            }
        }
    }
}

/// 内存水位镜像：采样任务写入，读方（router/池）无锁读取。
pub struct MemoryWatermark {
    level: AtomicU8,
    critical_mib: u64,
    tight_mib: u64,
    consecutive_failures: AtomicU8,
}

impl MemoryWatermark {
    pub fn new(critical_mib: u64, tight_mib: u64) -> Arc<Self> {
        Arc::new(Self {
            level: AtomicU8::new(MemoryLevel::Abundant as u8),
            critical_mib,
            tight_mib,
            consecutive_failures: AtomicU8::new(0),
        })
    }

    pub fn level(&self) -> MemoryLevel {
        MemoryLevel::from_u8(self.level.load(Ordering::Relaxed))
    }

    /// 应用一次采样结果（None = 采样失败）并返回最新档位。
    ///
    /// 失败语义：保持上一档位并 `log::warn!`；连续 [`FAILURE_THRESHOLD`] 次失败
    /// 保守判 Critical（错误不吞——sysinfo 无错误返回值，available == 0 视为采样失败）。
    /// 成功采样重置失败计数。
    pub fn apply_sample(&self, available_mib: Option<u64>) -> MemoryLevel {
        match available_mib {
            Some(available) => {
                self.consecutive_failures.store(0, Ordering::Relaxed);
                let next = classify(available, self.critical_mib, self.tight_mib);
                let prev = self.level.swap(next as u8, Ordering::Relaxed);
                #[cfg(all(feature = "engine-moli", feature = "metrics"))]
                gauge!("crawlrs_memory_level").set(next as u8 as f64);
                if prev != next as u8 {
                    log::info!(
                        target: "crawlrs::memory",
                        "memory level transition: {:?} -> {:?} (available={} MiB)",
                        MemoryLevel::from_u8(prev),
                        next,
                        available
                    );
                }
                next
            }
            None => {
                let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= FAILURE_THRESHOLD {
                    let prev = self
                        .level
                        .swap(MemoryLevel::Critical as u8, Ordering::Relaxed);
                    #[cfg(all(feature = "engine-moli", feature = "metrics"))]
                    gauge!("crawlrs_memory_level").set(MemoryLevel::Critical as u8 as f64);
                    log::warn!(
                        target: "crawlrs::memory",
                        "memory sampling failed {} times consecutively, forcing Critical (was {:?})",
                        failures,
                        MemoryLevel::from_u8(prev)
                    );
                    MemoryLevel::Critical
                } else {
                    log::warn!(
                        target: "crawlrs::memory",
                        "memory sampling failed ({}), keeping previous level {:?}",
                        failures,
                        self.level()
                    );
                    self.level()
                }
            }
        }
    }

    /// 采样循环：以 `refresh_interval` 周期刷新系统内存并写入镜像。通常由 DI 装配
    /// spawn 一次、随进程生命周期运行；任务取消即停止（无内部死循环外的状态）。
    pub async fn run(self: Arc<Self>, refresh_interval: Duration) {
        let mut sys = sysinfo::System::new();
        let mut ticker = tokio::time::interval(refresh_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            sys.refresh_memory();
            // available == 0 视为采样失败（sysinfo 在部分平台读不到时给 0，无错误 API）
            let available_mib = sys.available_memory() / 1024 / 1024;
            let sample = if available_mib == 0 {
                None
            } else {
                Some(available_mib)
            };
            self.apply_sample(sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_boundary_511_512() {
        // 511 MiB 可用 → Critical；512 MiB（恰好等于 critical 阈值）→ 非 Critical
        assert_eq!(classify(511, 512, 2048), MemoryLevel::Critical);
        assert_eq!(classify(512, 512, 2048), MemoryLevel::Tight);
    }

    #[test]
    fn classify_boundary_2047_2048() {
        // 2047 MiB → Tight；2048 MiB（恰好等于 tight 阈值）→ Abundant
        assert_eq!(classify(2047, 512, 2048), MemoryLevel::Tight);
        assert_eq!(classify(2048, 512, 2048), MemoryLevel::Abundant);
    }

    #[test]
    fn apply_sample_failure_keeps_previous_then_critical() {
        let wm = MemoryWatermark::new(512, 2048);
        // 先落一个明确的 Tight
        assert_eq!(wm.apply_sample(Some(1500)), MemoryLevel::Tight);
        // 采样失败 ×2：保持上一档位
        assert_eq!(wm.apply_sample(None), MemoryLevel::Tight);
        assert_eq!(wm.apply_sample(None), MemoryLevel::Tight);
        // 连续第 3 次失败：保守侧判 Critical
        assert_eq!(wm.apply_sample(None), MemoryLevel::Critical);
    }

    #[test]
    fn apply_sample_success_resets_failure_counter() {
        let wm = MemoryWatermark::new(512, 2048);
        assert_eq!(wm.apply_sample(None), MemoryLevel::Abundant); // 失败 1 次，保持初始档
        assert_eq!(wm.apply_sample(None), MemoryLevel::Abundant); // 失败 2 次
        assert_eq!(wm.apply_sample(Some(4096)), MemoryLevel::Abundant); // 成功采样重置计数
        assert_eq!(wm.apply_sample(None), MemoryLevel::Abundant); // 重新计数 1 次
        assert_eq!(wm.apply_sample(None), MemoryLevel::Abundant); // 2 次
        assert_eq!(wm.apply_sample(None), MemoryLevel::Critical); // 3 次 → Critical
    }

    #[test]
    fn sample_below_critical_reflected_in_mirror() {
        let wm = MemoryWatermark::new(512, 2048);
        assert_eq!(wm.apply_sample(Some(100)), MemoryLevel::Critical);
        assert_eq!(wm.level(), MemoryLevel::Critical);
    }

    // ---- apply_memory_policy 语义表：3 档 × {moli, playwright, reqwest} × {needs_js} ----

    #[test]
    fn policy_abundant_never_changes_score() {
        for (engine, score) in [("moli", 100u8), ("playwright", 200), ("reqwest", 150)] {
            for needs_js in [true, false] {
                assert_eq!(
                    apply_memory_policy(score, engine, MemoryLevel::Abundant, needs_js),
                    score,
                    "Abundant 不改分: {engine}/{needs_js}"
                );
            }
        }
    }

    #[test]
    fn policy_non_needs_js_unaffected_by_any_level() {
        for level in [
            MemoryLevel::Abundant,
            MemoryLevel::Tight,
            MemoryLevel::Critical,
        ] {
            for (engine, score) in [("moli", 100u8), ("playwright", 200), ("reqwest", 150)] {
                assert_eq!(
                    apply_memory_policy(score, engine, level, false),
                    score,
                    "非 needs_js 不受水位影响: {engine}/{level:?}"
                );
            }
        }
    }

    #[test]
    fn policy_tight_boosts_only_moli() {
        assert_eq!(
            apply_memory_policy(100, "moli", MemoryLevel::Tight, true),
            130,
            "Tight 对 moli +30"
        );
        assert_eq!(
            apply_memory_policy(250, "moli", MemoryLevel::Tight, true),
            255,
            "Tight 加成封顶 255"
        );
        assert_eq!(
            apply_memory_policy(200, "playwright", MemoryLevel::Tight, true),
            200,
            "Tight 时 Playwright 不得加分"
        );
        assert_eq!(
            apply_memory_policy(150, "reqwest", MemoryLevel::Tight, true),
            150,
            "Tight 时静态引擎不改分"
        );
    }

    #[test]
    fn policy_critical_zeroes_browser_engines_except_moli() {
        assert_eq!(
            apply_memory_policy(200, "playwright", MemoryLevel::Critical, true),
            0,
            "Critical 时 Playwright 归 0"
        );
        for browser in [
            "flaresolverr",
            "flaresolverr_cdp",
            "flaresolverr_tls",
            "mllm",
        ] {
            assert_eq!(
                apply_memory_policy(180, browser, MemoryLevel::Critical, true),
                0,
                "Critical 时浏览器系 {browser} 归 0"
            );
        }
        assert_eq!(
            apply_memory_policy(120, "moli", MemoryLevel::Critical, true),
            120,
            "Critical 时仅 moli 保留分数"
        );
        assert_eq!(
            apply_memory_policy(150, "reqwest", MemoryLevel::Critical, true),
            150,
            "Critical 时静态引擎不改分（回退语义由 router 层处理）"
        );
    }
}
