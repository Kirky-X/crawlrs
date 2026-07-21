// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 系统指标监控模块
//!
//! 提供 CPU、内存等系统指标的监控功能，支持通过 DI 注入。

use chrono::Utc;
use log::{error, warn};
use metrics::{describe_counter, describe_gauge, describe_histogram, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

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
    /// 系统信息
    system: Arc<Mutex<System>>,
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
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();

        Self {
            system: Arc::new(Mutex::new(sys)),
            last_update: Arc::new(AtomicU64::new(0)),
            cpu_usage: Arc::new(AtomicU64::new(0)),
            memory_usage: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 更新系统指标
    fn update(&mut self) {
        // Acquire lock with poisoning handling
        let sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, cannot update metrics");
                // On poisoning, we cannot recover the system state
                // Skip this update cycle
                return;
            }
        };
        let mut sys = sys; // Make mutable for refresh operations
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    /// 刷新指标（用于后台任务）
    pub fn refresh(&mut self) {
        self.update();

        let cpu_usage = {
            let sys = match self.system.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    log::error!("Failed to acquire system monitor lock for CPU usage");
                    return;
                }
            };
            f64::from(sys.global_cpu_usage())
        };

        let memory_usage = {
            let sys = match self.system.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    log::error!("Failed to acquire system monitor lock for memory usage");
                    return;
                }
            };
            let total_mem = sys.total_memory();
            if total_mem > 0 {
                sys.used_memory() as f64 / total_mem as f64 * 100.0
            } else {
                0.0
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
    system: Arc<Mutex<System>>,
}

impl SystemMonitor {
    /// Create a new system monitor
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();

        Self {
            system: Arc::new(Mutex::new(sys)),
        }
    }

    /// Update system metrics
    fn update(&mut self) {
        let mut sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, cannot update metrics");
                return;
            }
        };
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    /// Get current CPU usage (0.0 - 1.0)
    pub fn cpu_usage(&mut self) -> f64 {
        self.update();
        let sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, returning 0.0 for CPU usage");
                return 0.0;
            }
        };
        f64::from(sys.global_cpu_usage()) / 100.0
    }

    /// Get current memory usage (0.0 - 1.0)
    pub fn memory_usage(&mut self) -> f64 {
        self.update();
        let sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("System monitor mutex poisoned, returning 0.0 for memory usage");
                return 0.0;
            }
        };
        let total_mem = sys.total_memory();
        if total_mem > 0 {
            sys.used_memory() as f64 / total_mem as f64
        } else {
            0.0
        }
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutable system monitor for background updates
struct MutableSystemMonitor {
    system: Arc<Mutex<System>>,
}

impl MutableSystemMonitor {
    fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();

        Self {
            system: Arc::new(Mutex::new(sys)),
        }
    }

    fn refresh(&mut self) {
        let mut sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("MutableSystemMonitor mutex poisoned, cannot refresh");
                return;
            }
        };
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    fn cpu_usage(&self) -> f64 {
        let sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("MutableSystemMonitor mutex poisoned, returning 0.0 for CPU usage");
                return 0.0;
            }
        };
        f64::from(sys.global_cpu_usage()) / 100.0
    }

    fn memory_usage(&self) -> f64 {
        let sys = match self.system.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("MutableSystemMonitor mutex poisoned, returning 0.0 for memory usage");
                return 0.0;
            }
        };
        let total_mem = sys.total_memory();
        if total_mem > 0 {
            sys.used_memory() as f64 / total_mem as f64
        } else {
            0.0
        }
    }
}

/// 初始化指标系统
///
/// 配置并注册应用所需的各类监控指标
pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    if let Err(e) = builder.with_http_listener(([0, 0, 0, 0], 9100)).install() {
        log::warn!(
            "Failed to install Prometheus recorder: {}. Metrics will be disabled.",
            e
        );
        return;
    }

    // Create mutable system monitor for background task
    let mut monitor = MutableSystemMonitor::new();

    // Start background task to update system metrics
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            update_system_metrics(&mut monitor);
        }
    });

    // Register metrics
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
}

fn update_system_metrics(monitor: &mut MutableSystemMonitor) {
    monitor.refresh();

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

// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Mock SystemMonitorTrait implementation ==========

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

    // ========== get_cpu_usage_with_monitor tests ==========

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

    // ========== get_memory_usage_with_monitor tests ==========

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

    // ========== is_metrics_stale_with_monitor tests ==========

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

    // ========== SystemMonitorComponent tests ==========

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
        // cpu_usage() returns stored_value / 100.0
        // After refresh, the stored value is the raw CPU percentage (0-100)
        // So cpu_usage() should return a value in [0, 1]
        let mut component = SystemMonitorComponent::new();
        component.refresh();
        let cpu = component.cpu_usage();
        assert!(cpu >= 0.0, "cpu should be non-negative");
        assert!(cpu <= 1.0, "cpu should be at most 1.0, got {}", cpu);
    }

    // ========== SystemMonitor tests ==========

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

    // ========== check_alerts tests ==========

    #[test]
    fn test_check_alerts_does_not_panic() {
        // check_alerts is currently a no-op but should not panic
        check_alerts();
    }

    // ========== SystemMonitorTrait mock integration tests ==========

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

    // ========== MutableSystemMonitor tests ==========

    #[test]
    fn test_mutable_system_monitor_new_does_not_panic() {
        let _monitor = MutableSystemMonitor::new();
    }

    #[test]
    fn test_mutable_system_monitor_refresh_does_not_panic() {
        let mut monitor = MutableSystemMonitor::new();
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
        let mut monitor = MutableSystemMonitor::new();
        monitor.refresh();
        let cpu = monitor.cpu_usage();
        assert!((0.0..=1.0).contains(&cpu));
    }

    #[test]
    fn test_mutable_system_monitor_memory_usage_after_refresh() {
        let mut monitor = MutableSystemMonitor::new();
        monitor.refresh();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&mem));
    }

    #[test]
    fn test_mutable_system_monitor_multiple_refresh_cycles() {
        let mut monitor = MutableSystemMonitor::new();
        for _ in 0..3 {
            monitor.refresh();
            let cpu = monitor.cpu_usage();
            let mem = monitor.memory_usage();
            assert!((0.0..=1.0).contains(&cpu));
            assert!((0.0..=1.0).contains(&mem));
        }
    }

    // ========== update_system_metrics tests ==========

    #[test]
    fn test_update_system_metrics_does_not_panic() {
        let mut monitor = MutableSystemMonitor::new();
        update_system_metrics(&mut monitor);
    }

    #[test]
    fn test_update_system_metrics_after_refresh() {
        let mut monitor = MutableSystemMonitor::new();
        monitor.refresh();
        update_system_metrics(&mut monitor);
        // Verify monitor is still functional after update
        let cpu = monitor.cpu_usage();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&cpu));
        assert!((0.0..=1.0).contains(&mem));
    }

    #[test]
    fn test_update_system_metrics_multiple_calls() {
        let mut monitor = MutableSystemMonitor::new();
        // Calling update_system_metrics multiple times should not panic
        for _ in 0..5 {
            update_system_metrics(&mut monitor);
        }
        let cpu = monitor.cpu_usage();
        let mem = monitor.memory_usage();
        assert!((0.0..=1.0).contains(&cpu));
        assert!((0.0..=1.0).contains(&mem));
    }

    // =========================================================================
    // Mutex poisoning 测试：覆盖所有 mutex poisoned 错误分支
    // 通过在另一个线程中 panic 来 poison mutex，然后验证方法不 panic
    // =========================================================================

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

        // refresh() should return early without panicking
        component.refresh();
        // last_update was never updated (refresh failed), so still stale
        assert!(
            component.is_metrics_stale(),
            "last_update should remain 0 after poisoned refresh"
        );
    }

    #[test]
    fn test_system_monitor_mutex_poisoned_cpu_returns_zero() {
        let mut monitor = SystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let cpu = monitor.cpu_usage();
        assert_eq!(cpu, 0.0, "poisoned mutex should return 0.0 for CPU usage");
    }

    #[test]
    fn test_system_monitor_mutex_poisoned_memory_returns_zero() {
        let mut monitor = SystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let mem = monitor.memory_usage();
        assert_eq!(
            mem, 0.0,
            "poisoned mutex should return 0.0 for memory usage"
        );
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_refresh_no_panic() {
        let mut monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        // refresh() should return early without panicking
        monitor.refresh();
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_cpu_returns_zero() {
        let monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let cpu = monitor.cpu_usage();
        assert_eq!(cpu, 0.0);
    }

    #[test]
    fn test_mutable_system_monitor_mutex_poisoned_memory_returns_zero() {
        let monitor = MutableSystemMonitor::new();
        let system_arc = monitor.system.clone();
        let handle = std::thread::spawn(move || {
            let _guard = system_arc.lock().unwrap();
            panic!("poisoning mutex for test");
        });
        let _ = handle.join();

        let mem = monitor.memory_usage();
        assert_eq!(mem, 0.0);
    }

    // ---- init_metrics: 覆盖 tokio::spawn + interval 注册路径 ----

    #[tokio::test]
    async fn test_init_metrics_does_not_panic() {
        // init_metrics 会尝试绑定 9100 端口；若端口被占用则 warn 并 return，
        // 若端口可用则 spawn 后台任务并注册指标。两种情况都不应 panic。
        init_metrics();
    }
}
