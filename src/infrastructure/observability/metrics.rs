// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 系统指标监控模块
//!
//! 提供 CPU、内存等系统指标的监控功能，支持通过 DI 注入。

use chrono::Utc;
use log::{error, warn};
use metrics::{describe_counter, describe_gauge, describe_histogram, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

/// 构造 sysinfo `System`（0.40 起构造可失败：内核接口不可用/平台不支持）。
///
/// 失败时返回 `None` 并记录错误——三个监控组件进入 **fail-closed 降级**：
/// cpu/memory 读取返回 1.0（最坏情形哨兵，驱动内存准入拒绝与阈值告警；
/// 绝不返回 0.0 这种"假健康"值），不 panic。错误可见：构造期一条 `log::error!`。
fn new_system() -> Option<System> {
    match System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    ) {
        Ok(sys) => {
            // new_with_specifics 已按 RefreshKind 刷新 CPU/内存；不再 refresh_all()
            //（后者会枚举全部进程，纯属启动期浪费）
            Some(sys)
        }
        Err(e) => {
            log::error!("sysinfo System 初始化失败，系统监控 fail-closed 降级（读取返回 100%，内存准入拒绝新任务）: {e}");
            None
        }
    }
}

/// 系统监控 trait（支持 DI）
///
/// 提供系统资源监控的抽象接口，便于测试时注入 mock 实现。
pub trait SystemMonitorTrait: Send + Sync {
    /// 获取当前 CPU 使用率 (0.0 - 1.0)
    fn cpu_usage(&self) -> f64;
    /// 获取当前内存使用率 (0.0 - 1.0)
    fn memory_usage(&self) -> f64;
    /// 检查指标是否过期
    fn is_metrics_stale(&self) -> bool;
}

/// 系统监控组件（DI 实现）
///
/// 通过原子变量提供无锁读取，同时保持热路径高性能。
pub struct SystemMonitorComponent {
    /// 系统信息（构造失败时为 None：监控降级，见 [`new_system`]）
    system: Arc<Mutex<Option<System>>>,
    /// 最后更新时间戳
    last_update: Arc<AtomicU64>,
    /// CPU 使用率 (0-100)
    cpu_usage: Arc<AtomicU64>,
    /// 内存使用率 (0-100)
    memory_usage: Arc<AtomicU64>,
}

impl SystemMonitorComponent {
    /// 创建新的系统监控组件
    pub fn new() -> Self {
        let sys = new_system();

        Self {
            system: Arc::new(Mutex::new(sys)),
            last_update: Arc::new(AtomicU64::new(0)),
            cpu_usage: Arc::new(AtomicU64::new(0)),
            memory_usage: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 测试专用：注入指定 System（None = 模拟构造失败，覆盖 fail-closed 降级路径）
    #[cfg(test)]
    fn with_system(system: Option<System>) -> Self {
        Self {
            system: Arc::new(Mutex::new(system)),
            last_update: Arc::new(AtomicU64::new(0)),
            cpu_usage: Arc::new(AtomicU64::new(0)),
            memory_usage: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 更新系统指标
    fn update(&mut self) {
        // Acquire lock with poisoning handling
        let mut guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, cannot update metrics");
                // On poisoning, we cannot recover the system state
                // Skip this update cycle
                return;
            }
        };
        let Some(sys) = guard.as_mut() else {
            // sysinfo 构造失败：监控已降级并在构造期记录 error
            return;
        };
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    /// 刷新指标（用于后台任务）
    pub fn refresh(&mut self) {
        self.update();

        let cpu_usage = {
            let guard = match self.system.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    log::error!(
                        "Failed to acquire system monitor lock for CPU usage; fail-closed 100%"
                    );
                    // 无法判断时按最坏情形写哨兵（保持 stale：last_update 不更新）
                    self.cpu_usage.store(100, Ordering::Relaxed);
                    self.memory_usage.store(100, Ordering::Relaxed);
                    return;
                }
            };
            match guard.as_ref() {
                Some(sys) => f64::from(sys.global_cpu_usage()),
                // fail-closed 哨兵：100% 让阈值告警必触发，避免显示假健康
                None => 100.0,
            }
        };

        let memory_usage = {
            let guard = match self.system.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    log::error!(
                        "Failed to acquire system monitor lock for memory usage; fail-closed 100%"
                    );
                    self.cpu_usage.store(100, Ordering::Relaxed);
                    self.memory_usage.store(100, Ordering::Relaxed);
                    return;
                }
            };
            match guard.as_ref() {
                Some(sys) => {
                    let total_mem = sys.total_memory();
                    if total_mem > 0 {
                        sys.used_memory() as f64 / total_mem as f64 * 100.0
                    } else {
                        0.0
                    }
                }
                // fail-closed 哨兵：100% 让阈值告警必触发
                None => 100.0,
            }
        };

        let now = Utc::now().timestamp() as u64;
        self.last_update.store(now, Ordering::Relaxed);
        self.cpu_usage.store((cpu_usage) as u64, Ordering::Relaxed);
        self.memory_usage
            .store(memory_usage as u64, Ordering::Relaxed);
    }
}

impl Default for SystemMonitorComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMonitorTrait for SystemMonitorComponent {
    fn cpu_usage(&self) -> f64 {
        self.cpu_usage.load(Ordering::Relaxed) as f64 / 100.0
    }

    fn memory_usage(&self) -> f64 {
        self.memory_usage.load(Ordering::Relaxed) as f64 / 100.0
    }

    fn is_metrics_stale(&self) -> bool {
        let last_update = self.last_update.load(Ordering::Relaxed);
        if last_update == 0 {
            return true;
        }
        let now = Utc::now().timestamp() as u64;
        now - last_update > 2
    }
}

/// System monitor for tracking CPU and memory usage
#[derive(Clone)]
pub struct SystemMonitor {
    system: Arc<Mutex<Option<System>>>,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new() -> Self {
        let sys = new_system();

        Self {
            system: Arc::new(Mutex::new(sys)),
        }
    }

    /// 测试专用：注入指定 System（None = 模拟构造失败）
    #[cfg(test)]
    fn with_system(system: Option<System>) -> Self {
        Self {
            system: Arc::new(Mutex::new(system)),
        }
    }

    /// Update system metrics
    fn update(&mut self) {
        let mut guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, cannot update metrics");
                return;
            }
        };
        let Some(sys) = guard.as_mut() else {
            return;
        };
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    /// Get current CPU usage (0.0 - 1.0)
    pub fn cpu_usage(&mut self) -> f64 {
        self.update();
        let guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!(
                    "System monitor mutex poisoned, returning fail-closed 1.0 for CPU usage"
                );
                return 1.0;
            }
        };
        match guard.as_ref() {
            Some(sys) => f64::from(sys.global_cpu_usage()) / 100.0,
            // fail-closed 哨兵：无法采集时按最坏情形返回 100%（不返回假健康 0%）
            None => 1.0,
        }
    }

    /// Get current memory usage (0.0 - 1.0)
    pub fn memory_usage(&mut self) -> f64 {
        self.update();
        let guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!(
                    "System monitor mutex poisoned, returning fail-closed 1.0 for memory usage"
                );
                return 1.0;
            }
        };
        match guard.as_ref() {
            Some(sys) => {
                let total_mem = sys.total_memory();
                if total_mem > 0 {
                    sys.used_memory() as f64 / total_mem as f64
                } else {
                    0.0
                }
            }
            // fail-closed 哨兵：无法采集时按最坏情形返回 100%
            None => 1.0,
        }
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutable system monitor for background updates
///
/// pub 以便 `init_metrics` 与 `MemoryScheduler` 共享同一实例。
/// 通过 `shared_system_monitor()` 获取全局单例，避免重复采集（复用）。
pub struct MutableSystemMonitor {
    system: Arc<Mutex<Option<System>>>,
}

impl MutableSystemMonitor {
    fn new() -> Self {
        let sys = new_system();

        Self {
            system: Arc::new(Mutex::new(sys)),
        }
    }

    /// 测试专用：注入指定 System（None = 模拟构造失败）
    #[cfg(test)]
    fn with_system(system: Option<System>) -> Self {
        Self {
            system: Arc::new(Mutex::new(system)),
        }
    }

    /// 刷新系统指标（改为 `&self`，内部 Arc\<Mutex\> 保证线程安全）
    pub fn refresh(&self) {
        let mut guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("MutableSystemMonitor mutex poisoned, cannot refresh");
                return;
            }
        };
        let Some(sys) = guard.as_mut() else {
            return;
        };
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    pub fn cpu_usage(&self) -> f64 {
        let guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!(
                    "MutableSystemMonitor mutex poisoned, returning fail-closed 1.0 for CPU usage"
                );
                return 1.0;
            }
        };
        match guard.as_ref() {
            Some(sys) => f64::from(sys.global_cpu_usage()) / 100.0,
            // fail-closed 哨兵：无法采集时按最坏情形返回 100%（内存准入拒新任务、告警触发）
            None => 1.0,
        }
    }

    pub fn memory_usage(&self) -> f64 {
        let guard = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("MutableSystemMonitor mutex poisoned, returning fail-closed 1.0 for memory usage");
                return 1.0;
            }
        };
        match guard.as_ref() {
            Some(sys) => {
                let total_mem = sys.total_memory();
                if total_mem > 0 {
                    sys.used_memory() as f64 / total_mem as f64
                } else {
                    0.0
                }
            }
            // fail-closed 哨兵：无法采集时按最坏情形返回 100%
            None => 1.0,
        }
    }
}

impl MutableSystemMonitor {
    /// 是否处于构造失败降级态（无法采集；读取路径返回 fail-closed 100%）。
    /// 供指标导出把「无法采集」与「真实满负载」区分开。
    pub fn is_degraded(&self) -> bool {
        match self.system.lock() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        }
    }
}

/// 为 MutableSystemMonitor 实现 SystemMonitorTrait
///
/// 供 `MemoryScheduler` 以 `Arc<dyn SystemMonitorTrait>` 注入。
impl SystemMonitorTrait for MutableSystemMonitor {
    fn cpu_usage(&self) -> f64 {
        MutableSystemMonitor::cpu_usage(self)
    }
    fn memory_usage(&self) -> f64 {
        MutableSystemMonitor::memory_usage(self)
    }
    fn is_metrics_stale(&self) -> bool {
        // 后台任务持续刷新，不判定过期；但 sysinfo 构造失败（降级）必须报告 stale，
        // 让调用方能把"无法采集"与"健康"区分开（fail-closed）
        match self.system.lock() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        }
    }
}

/// 全局共享系统监控单例
///
/// `init_metrics()` 首次调用时初始化并启动后台刷新任务；
/// `WorkerManager` 后续调用获取同一 `Arc` 实例供 `MemoryScheduler` 读取，
/// 避免重复 sysinfo 采集（复用现有 SystemMonitorTrait）。
static SHARED_MONITOR: OnceLock<Arc<MutableSystemMonitor>> = OnceLock::new();

/// 获取全局共享 `MutableSystemMonitor` 单例
///
/// 首次调用时创建实例并启动 5 秒间隔的后台刷新任务（需在 tokio runtime 内调用）。
/// 后续调用直接返回缓存的 `Arc`。
pub fn shared_system_monitor() -> Arc<MutableSystemMonitor> {
    SHARED_MONITOR
        .get_or_init(|| {
            let monitor = Arc::new(MutableSystemMonitor::new());
            let monitor_clone = Arc::clone(&monitor);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    monitor_clone.refresh();
                }
            });
            monitor
        })
        .clone()
}

/// 安装 Prometheus recorder；失败时记录错误并返回 false（指标禁用）。
/// 全局 Prometheus 渲染句柄（D8 修复：/metrics 端点此前返回硬编码 stub）。
static PROMETHEUS_HANDLE: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();

/// 返回 Prometheus 渲染句柄；未初始化（或 recorder 安装失败）时为 `None`。
pub fn prometheus_handle() -> Option<&'static metrics_exporter_prometheus::PrometheusHandle> {
    PROMETHEUS_HANDLE.get()
}

fn install_prometheus_recorder() -> bool {
    // D8 修复：改用 `install_recorder` 持有渲染句柄，`GET /metrics` 由此输出
    // 真实指标族。旧实现 `with_http_listener((0.0.0.0, 9100)).install()` 不返回
    // 句柄，端点恒返硬编码 stub（E2E SJ-03/S-H 实证 version/uptime 恒定）。
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            let _ = PROMETHEUS_HANDLE.set(handle);
            true
        }
        Err(e) => {
            log::error!(
                "Failed to install Prometheus recorder: {}. Metrics will be disabled.",
                e
            );
            false
        }
    }
}

/// 启动系统指标后台刷新任务（Prometheus 导出）。
fn spawn_system_metrics_updater(monitor: Arc<MutableSystemMonitor>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            update_system_metrics(&monitor);
        }
    });
}

/// 初始化指标系统
///
/// 配置并注册应用所需的各类监控指标
///
/// 通过 `shared_system_monitor()` 获取全局单例，避免重复采集。
/// 后台刷新任务由 `shared_system_monitor()` 首次调用时启动，此处仅负责
/// Prometheus 注册与指标导出。
pub fn init_metrics() {
    if !install_prometheus_recorder() {
        return;
    }

    // 复用全局共享 monitor（首次调用会启动后台刷新任务）
    let monitor = shared_system_monitor();

    // Start background task to update system metrics（Prometheus 导出）
    spawn_system_metrics_updater(monitor);

    describe_gauge!(
        "system_cpu_usage_ratio",
        "Current CPU usage ratio (0.0 to 1.0)"
    );
    describe_gauge!(
        "system_memory_usage_ratio",
        "Current memory usage ratio (0.0 to 1.0)"
    );
    describe_counter!("crawl_tasks_total", "Total number of crawl tasks submitted");
    describe_counter!(
        "crawl_tasks_completed_total",
        "Total number of crawl tasks completed"
    );
    describe_counter!(
        "crawl_tasks_failed_total",
        "Total number of crawl tasks failed"
    );
    describe_histogram!(
        "crawl_duration_seconds",
        "Duration of crawl tasks in seconds"
    );

    // Circuit Breaker Metrics
    describe_counter!(
        "circuit_breaker_requests_total",
        "Total number of requests processed by circuit breaker"
    );
    describe_counter!(
        "circuit_breaker_failures_total",
        "Total number of failed requests recorded by circuit breaker"
    );
    describe_counter!(
        "circuit_breaker_successes_total",
        "Total number of successful requests recorded by circuit breaker"
    );
    describe_counter!(
        "circuit_breaker_rejected_total",
        "Total number of requests rejected by open circuit breaker"
    );
    describe_gauge!(
        "circuit_breaker_status",
        "Current status of circuit breaker (0=Closed, 0.5=HalfOpen, 1=Open)"
    );

    // 增强指标
    describe_gauge!(
        "crawlrs_queue_depth",
        "Current number of items in the task queue"
    );
    describe_counter!(
        "crawlrs_engine_success_total",
        "Total engine routing results by engine and outcome"
    );
    describe_histogram!(
        "crawlrs_engine_duration_seconds",
        "Engine routing duration in seconds by engine"
    );
    describe_counter!(
        "crawlrs_cache_hit_total",
        "Total cache lookups by cache_type and result (hit/miss)"
    );
    describe_counter!(
        "crawlrs_webhook_delivery_total",
        "Total webhook delivery attempts by result and status_code"
    );
    describe_gauge!(
        "crawlrs_system_monitor_degraded",
        "Whether system monitoring is degraded (1 = sysinfo unavailable, readings are fail-closed sentinels)"
    );
}

fn update_system_metrics(monitor: &MutableSystemMonitor) {
    monitor.refresh();

    // 降级信号独立导出：区分「无法采集（fail-closed 100%）」与「真实满负载」
    gauge!("crawlrs_system_monitor_degraded").set(if monitor.is_degraded() { 1.0 } else { 0.0 });

    let cpu_usage = monitor.cpu_usage();

    gauge!("system_cpu_usage_ratio").set(cpu_usage);

    // Alerting logic
    if cpu_usage > 0.9 {
        error!(
            "CRITICAL: System CPU usage is extremely high: {:.2}%",
            cpu_usage * 100.0
        );
    } else if cpu_usage > 0.8 {
        warn!("ALARM: System CPU usage is high: {:.2}%", cpu_usage * 100.0);
    }

    let mem_usage = monitor.memory_usage();

    gauge!("system_memory_usage_ratio").set(mem_usage);

    if mem_usage > 0.9 {
        error!(
            "CRITICAL: System memory usage is extremely high: {:.2}%",
            mem_usage * 100.0
        );
    } else if mem_usage > 0.8 {
        warn!(
            "ALARM: System memory usage is high: {:.2}%",
            mem_usage * 100.0
        );
    }
}

/// Check metrics and trigger alerts based on defined rules (P0, P1, P2, P3)
pub fn check_alerts() {
    // Note: This function would typically be called periodically by the metrics system
    // Access metrics from the global registry if possible, or use shared state.
    // For this implementation, we will simulate the check based on logged metrics.

    // P1-High: Success Rate < 95%
    // This requires calculating success rate over a window, which is best done in Prometheus.
    // However, for application-level alerting:
    // We could maintain a sliding window of success/failure counts.

    // P2-Medium: Queue Backlog > 10000
    // We can't access DB here directly. We should rely on the metrics collected by workers.

    // P3-Low: Single Engine Failure
    // Checked in HealthMonitor
}

/// 使用注入的 SystemMonitor 获取 CPU 使用率 (推荐方式)
/// This is the recommended method for DI-based usage.
pub fn get_cpu_usage_with_monitor(monitor: &dyn SystemMonitorTrait) -> f64 {
    monitor.cpu_usage()
}

/// 使用注入的 SystemMonitor 获取内存使用率 (推荐方式)
/// This is the recommended method for DI-based usage.
pub fn get_memory_usage_with_monitor(monitor: &dyn SystemMonitorTrait) -> f64 {
    monitor.memory_usage()
}

/// 使用注入的 SystemMonitor 检查指标是否过期 (推荐方式)
/// This is the recommended method for DI-based usage.
pub fn is_metrics_stale_with_monitor(monitor: &dyn SystemMonitorTrait) -> bool {
    monitor.is_metrics_stale()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock SystemMonitorTrait implementation

    struct MockSystemMonitor {
        cpu: f64,
        memory: f64,
        stale: bool,
    }

    impl SystemMonitorTrait for MockSystemMonitor {
        fn cpu_usage(&self) -> f64 {
            self.cpu
        }
        fn memory_usage(&self) -> f64 {
            self.memory
        }
        fn is_metrics_stale(&self) -> bool {
            self.stale
        }
    }

    // sysinfo 构造失败（降级）路径：fail-closed 哨兵与 stale 语义

    #[test]
    fn degraded_component_reports_fail_closed_sentinel() {
        let mut c = SystemMonitorComponent::with_system(None);
        c.refresh();
        // 无法采集时按最坏情形 100%，绝不返回假健康 0%
        assert!((SystemMonitorTrait::cpu_usage(&c) - 1.0).abs() < f64::EPSILON);
        assert!((SystemMonitorTrait::memory_usage(&c) - 1.0).abs() < f64::EPSILON);
        // refresh 写入哨兵值后视为"已刷新"
        assert!(!SystemMonitorTrait::is_metrics_stale(&c));
    }

    #[test]
    fn degraded_monitor_reports_fail_closed_sentinel() {
        let mut m = SystemMonitor::with_system(None);
        assert!((m.cpu_usage() - 1.0).abs() < f64::EPSILON);
        assert!((m.memory_usage() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn degraded_mutable_monitor_reports_fail_closed_and_stale() {
        let m = MutableSystemMonitor::with_system(None);
        assert!((m.memory_usage() - 1.0).abs() < f64::EPSILON);
        assert!((m.cpu_usage() - 1.0).abs() < f64::EPSILON);
        // 降级必须被报告为 stale，调用方可区分"无法采集"与"空闲"
        assert!(SystemMonitorTrait::is_metrics_stale(&m));
    }

    // get_cpu_usage_with_monitor tests

    #[test]
    fn test_get_cpu_usage_with_monitor_returns_value() {
        let monitor = MockSystemMonitor {
            cpu: 0.75,
            memory: 0.5,
            stale: false,
        };
        let result = get_cpu_usage_with_monitor(&monitor);
        assert!((result - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_cpu_usage_with_monitor_zero() {
        let monitor = MockSystemMonitor {
            cpu: 0.0,
            memory: 0.0,
            stale: false,
        };
        assert!((get_cpu_usage_with_monitor(&monitor) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_cpu_usage_with_monitor_full() {
        let monitor = MockSystemMonitor {
            cpu: 1.0,
            memory: 0.0,
            stale: false,
        };
        assert!((get_cpu_usage_with_monitor(&monitor) - 1.0).abs() < f64::EPSILON);
    }

    // get_memory_usage_with_monitor tests

    #[test]
    fn test_get_memory_usage_with_monitor_returns_value() {
        let monitor = MockSystemMonitor {
            cpu: 0.3,
            memory: 0.65,
            stale: false,
        };
        let result = get_memory_usage_with_monitor(&monitor);
        assert!((result - 0.65).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_memory_usage_with_monitor_zero() {
        let monitor = MockSystemMonitor {
            cpu: 0.0,
            memory: 0.0,
            stale: false,
        };
        assert!((get_memory_usage_with_monitor(&monitor) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_memory_usage_with_monitor_full() {
        let monitor = MockSystemMonitor {
            cpu: 0.0,
            memory: 1.0,
            stale: false,
        };
        assert!((get_memory_usage_with_monitor(&monitor) - 1.0).abs() < f64::EPSILON);
    }

    // is_metrics_stale_with_monitor tests

    #[test]
    fn test_is_metrics_stale_with_monitor_true() {
        let monitor = MockSystemMonitor {
            cpu: 0.0,
            memory: 0.0,
            stale: true,
        };
        assert!(is_metrics_stale_with_monitor(&monitor));
    }

    #[test]
    fn test_is_metrics_stale_with_monitor_false() {
        let monitor = MockSystemMonitor {
            cpu: 0.0,
            memory: 0.0,
            stale: false,
        };
        assert!(!is_metrics_stale_with_monitor(&monitor));
    }

    // SystemMonitorComponent tests

    #[test]
    fn test_system_monitor_component_new_initializes_zero() {
        let component = SystemMonitorComponent::new();
        // Before refresh, cpu and memory should be 0
        assert!((component.cpu_usage() - 0.0).abs() < f64::EPSILON);
        assert!((component.memory_usage() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_system_monitor_component_default_equals_new() {
        let component = SystemMonitorComponent::default();
        assert!((component.cpu_usage() - 0.0).abs() < f64::EPSILON);
        assert!((component.memory_usage() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_system_monitor_component_is_stale_before_refresh() {
        let component = SystemMonitorComponent::new();
        // last_update is 0 before refresh, so metrics should be stale
        assert!(
            component.is_metrics_stale(),
            "metrics should be stale before refresh"
        );
    }

    #[test]
    fn test_system_monitor_component_refresh_updates_timestamp() {
        let mut component = SystemMonitorComponent::new();
        assert!(component.is_metrics_stale());
        component.refresh();
        // After refresh, last_update is set to current time, so should not be stale
        assert!(
            !component.is_metrics_stale(),
            "metrics should not be stale immediately after refresh"
        );
    }

    #[test]
    fn test_system_monitor_component_refresh_updates_cpu_and_memory() {
        let mut component = SystemMonitorComponent::new();
        // Before refresh, values are 0
        assert!((component.cpu_usage() - 0.0).abs() < f64::EPSILON);
        assert!((component.memory_usage() - 0.0).abs() < f64::EPSILON);

        component.refresh();

        // After refresh, values should be in valid range [0.0, 1.0]
        let cpu = component.cpu_usage();
        let mem = component.memory_usage();
        assert!(
            (0.0..=1.0).contains(&cpu),
            "cpu usage should be in [0, 1], got {}",
            cpu
        );
        assert!(
            (0.0..=1.0).contains(&mem),
            "memory usage should be in [0, 1], got {}",
            mem
        );
    }

    #[test]
    fn test_system_monitor_component_cpu_usage_divides_by_100() {
        // cpu_usage returns stored_value / 100.0
        // After refresh, the stored value is the raw CPU percentage (0-100)
        // So cpu_usage should return a value in [0, 1]
        let mut component = SystemMonitorComponent::new();
        component.refresh();
        let cpu = component.cpu_usage();
        assert!(cpu >= 0.0, "cpu should be non-negative");
        assert!(cpu <= 1.0, "cpu should be at most 1.0, got {}", cpu);
    }

    // SystemMonitor tests

    #[test]
    fn test_system_monitor_new_does_not_panic() {
        let _monitor = SystemMonitor::new();
    }

    #[test]
    fn test_system_monitor_default_equals_new() {
        let _monitor = SystemMonitor::default();
    }

    #[test]
    fn test_system_monitor_cpu_usage_in_valid_range() {
        let mut monitor = SystemMonitor::new();
        let cpu = monitor.cpu_usage();
        assert!(cpu >= 0.0, "cpu should be non-negative");
        assert!(cpu <= 1.0, "cpu should be at most 1.0, got {}", cpu);
    }

    #[test]
    fn test_system_monitor_memory_usage_in_valid_range() {
        let mut monitor = SystemMonitor::new();
        let mem = monitor.memory_usage();
        assert!(mem >= 0.0, "memory should be non-negative");
        assert!(mem <= 1.0, "memory should be at most 1.0, got {}", mem);
    }

    #[test]
    fn test_system_monitor_clone() {
        let monitor = SystemMonitor::new();
        let _cloned = monitor.clone();
    }

    #[test]
    fn test_system_monitor_multiple_cpu_reads() {
        let mut monitor = SystemMonitor::new();
        let cpu1 = monitor.cpu_usage();
        let cpu2 = monitor.cpu_usage();
        // Both should be in valid range
        assert!((0.0..=1.0).contains(&cpu1));
        assert!((0.0..=1.0).contains(&cpu2));
    }

    // check_alerts tests

    #[test]
    fn test_check_alerts_does_not_panic() {
        // check_alerts is currently a no-op but should not panic
        check_alerts();
    }

    // SystemMonitorTrait mock integration tests

    #[test]
    fn test_mock_monitor_implements_trait() {
        let monitor = MockSystemMonitor {
            cpu: 0.42,
            memory: 0.58,
            stale: false,
        };
        // Verify trait methods work through the trait interface
        let trait_ref: &dyn SystemMonitorTrait = &monitor;
        assert!((trait_ref.cpu_usage() - 0.42).abs() < f64::EPSILON);
        assert!((trait_ref.memory_usage() - 0.58).abs() < f64::EPSILON);
        assert!(!trait_ref.is_metrics_stale());
    }

    #[test]
    fn test_helper_functions_with_mock_via_trait() {
        let monitor = MockSystemMonitor {
            cpu: 0.80,
            memory: 0.90,
            stale: true,
        };
        let trait_ref: &dyn SystemMonitorTrait = &monitor;
        assert!((get_cpu_usage_with_monitor(trait_ref) - 0.80).abs() < f64::EPSILON);
        assert!((get_memory_usage_with_monitor(trait_ref) - 0.90).abs() < f64::EPSILON);
        assert!(is_metrics_stale_with_monitor(trait_ref));
    }

    // MutableSystemMonitor tests

    #[test]
    fn test_mutable_system_monitor_new_does_not_panic() {
        let _monitor = MutableSystemMonitor::new();
    }

    #[test]
    fn test_mutable_system_monitor_refresh_does_not_panic() {
        let monitor = MutableSystemMonitor::new();
        monitor.refresh();
    }

    #[test]
    fn test_mutable_system_monitor_cpu_usage_in_valid_range() {
        let monitor = MutableSystemMonitor::new();
        let cpu = monitor.cpu_usage();
        assert!(cpu >= 0.0, "cpu should be non-negative");
        assert!(cpu <= 1.0, "cpu should be at most 1.0, got {}", cpu);
    }

    #[test]
    fn test_mutable_system_monitor_memory_usage_in_valid_range() {
        let monitor = MutableSystemMonitor::new();
        let mem = monitor.memory_usage();
        assert!(mem >= 0.0, "memory should be non-negative");
        assert!(mem <= 1.0, "memory should be at most 1.0, got {}", mem);
    }

    #[test]
    fn test_mutable_system_monitor_cpu_usage_after_refresh() {
        let monitor = MutableSystemMonitor::new();
        monitor.refresh();
        let cpu = monitor.cpu_usage();
        assert!((0.0..=1.0).contains(&cpu));
    }

    #[test]
    fn test_mutable_system_monitor_memory_usage_after_refresh() {
        let monitor = MutableSystemMonitor::new();
        monitor.refresh();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&mem));
    }

    #[test]
    fn test_mutable_system_monitor_multiple_refresh_cycles() {
        let monitor = MutableSystemMonitor::new();
        for _ in 0..3 {
            monitor.refresh();
            let cpu = monitor.cpu_usage();
            let mem = monitor.memory_usage();
            assert!((0.0..=1.0).contains(&cpu));
            assert!((0.0..=1.0).contains(&mem));
        }
    }

    // update_system_metrics tests

    #[test]
    fn test_update_system_metrics_does_not_panic() {
        let monitor = MutableSystemMonitor::new();
        update_system_metrics(&monitor);
    }

    #[test]
    fn test_update_system_metrics_after_refresh() {
        let monitor = MutableSystemMonitor::new();
        monitor.refresh();
        update_system_metrics(&monitor);
        // Verify monitor is still functional after update
        let cpu = monitor.cpu_usage();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&cpu));
        assert!((0.0..=1.0).contains(&mem));
    }

    #[test]
    fn test_update_system_metrics_multiple_calls() {
        let monitor = MutableSystemMonitor::new();
        // Calling update_system_metrics multiple times should not panic
        for _ in 0..5 {
            update_system_metrics(&monitor);
        }
        let cpu = monitor.cpu_usage();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&cpu));
        assert!((0.0..=1.0).contains(&mem));
    }

    // Mutex poisoning 测试：覆盖所有 mutex poisoned 错误分支
    // 通过在另一个线程中 panic 来 poison mutex，然后验证方法不 panic

    #[test]
    fn test_system_monitor_component_mutex_poisoned_refresh_no_panic() {
        let mut component = SystemMonitorComponent::new();
        assert!(component.is_metrics_stale()); // last_update == 0

        // Poison the mutex by panicking while holding the lock
        let system_arc = component.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        // refresh should return early without panicking
        component.refresh();
        // 锁中毒 = 无法判断 → fail-closed 100%，且仍报告 stale
        assert_eq!(SystemMonitorTrait::cpu_usage(&component), 1.0);
        assert_eq!(SystemMonitorTrait::memory_usage(&component), 1.0);
        assert!(
            component.is_metrics_stale(),
            "last_update should remain 0 after poisoned refresh"
        );
    }

    #[test]
    fn test_system_monitor_mutex_poisoned_cpu_fails_closed() {
        let mut monitor = SystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let cpu = monitor.cpu_usage();
        assert_eq!(cpu, 1.0, "poisoned mutex must fail closed (100%)");
    }

    #[test]
    fn test_system_monitor_mutex_poisoned_memory_fails_closed() {
        let mut monitor = SystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let mem = monitor.memory_usage();
        assert_eq!(mem, 1.0, "poisoned mutex must fail closed (100%)");
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_refresh_no_panic() {
        let monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        // refresh should return early without panicking
        monitor.refresh();
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_cpu_fails_closed() {
        let monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let cpu = monitor.cpu_usage();
        assert_eq!(cpu, 1.0);
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_memory_fails_closed() {
        let monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let mem = monitor.memory_usage();
        assert_eq!(mem, 1.0);
    }

    // init_metrics: 覆盖 tokio::spawn + interval 注册路径

    #[tokio::test]
    async fn test_init_metrics_does_not_panic() {
        // init_metrics 不再绑定 9100（由专用监听/网络层采集）；
        // 若端口可用则 spawn 后台任务并注册指标。两种情况都不应 panic。
        init_metrics();
    }
}
