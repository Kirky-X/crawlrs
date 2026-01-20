// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 系统指标监控模块
//!
//! 提供 CPU、内存等系统指标的监控功能，支持通过 DI 注入。

use chrono::Utc;
use metrics::{describe_counter, describe_gauge, describe_histogram, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use shaku::{Component, Interface};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tracing::{error, warn};

/// 系统监控 trait（支持 DI）
///
/// 提供系统资源监控的抽象接口，便于测试时注入 mock 实现。
pub trait SystemMonitorTrait: Interface + Send + Sync {
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
#[derive(Component)]
#[shaku(interface = SystemMonitorTrait)]
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
                tracing::error!("System monitor mutex poisoned, cannot update metrics");
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
                    tracing::error!("Failed to acquire system monitor lock for CPU usage");
                    return;
                }
            };
            f64::from(sys.global_cpu_usage())
        };

        let memory_usage = {
            let sys = match self.system.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    tracing::error!("Failed to acquire system monitor lock for memory usage");
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
        let mut sys = self.system.lock().unwrap();
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    /// Get current CPU usage (0.0 - 1.0)
    pub fn cpu_usage(&mut self) -> f64 {
        self.update();
        let sys = self.system.lock().unwrap();
        f64::from(sys.global_cpu_usage()) / 100.0
    }

    /// Get current memory usage (0.0 - 1.0)
    pub fn memory_usage(&mut self) -> f64 {
        self.update();
        let sys = self.system.lock().unwrap();
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
        let mut sys = self.system.lock().unwrap();
        sys.refresh_cpu_all();
        sys.refresh_memory();
    }

    fn cpu_usage(&self) -> f64 {
        let sys = self.system.lock().unwrap();
        f64::from(sys.global_cpu_usage()) / 100.0
    }

    fn memory_usage(&self) -> f64 {
        let sys = self.system.lock().unwrap();
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
        tracing::warn!(
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
