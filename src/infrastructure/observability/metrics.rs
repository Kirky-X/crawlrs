// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use metrics::{
    describe_counter, describe_gauge, describe_histogram, gauge,
};
use metrics_exporter_prometheus::PrometheusBuilder;
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tracing::{error, warn};

static SYSTEM: Lazy<Arc<Mutex<System>>> = Lazy::new(|| {
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );
    sys.refresh_all();
    Arc::new(Mutex::new(sys))
});

/// 初始化指标系统
///
/// 配置并注册应用所需的各类监控指标
pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    builder
        .install()
        .expect("failed to install Prometheus recorder");

    // Start background task to update system metrics
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            update_system_metrics();
        }
    });

    // Register metrics
    describe_gauge!("system_cpu_usage_ratio", "Current CPU usage ratio (0.0 to 1.0)");
    describe_gauge!("system_memory_usage_ratio", "Current memory usage ratio (0.0 to 1.0)");
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

fn update_system_metrics() {
    if let Ok(mut sys) = SYSTEM.lock() {
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cpu_usage = sys.global_cpu_usage() / 100.0;
        gauge!("system_cpu_usage_ratio").set(cpu_usage as f64);

        // Alerting logic
        if cpu_usage > 0.9 {
            error!("CRITICAL: System CPU usage is extremely high: {:.2}%", cpu_usage * 100.0);
        } else if cpu_usage > 0.8 {
            warn!("ALARM: System CPU usage is high: {:.2}%", cpu_usage * 100.0);
        }

        let total_mem = sys.total_memory();
        if total_mem > 0 {
            let used_mem = sys.used_memory();
            let mem_usage = used_mem as f64 / total_mem as f64;
            gauge!("system_memory_usage_ratio").set(mem_usage);

            if mem_usage > 0.9 {
                error!("CRITICAL: System memory usage is extremely high: {:.2}%", mem_usage * 100.0);
            } else if mem_usage > 0.8 {
                warn!("ALARM: System memory usage is high: {:.2}%", mem_usage * 100.0);
            }
        }
    }
}

/// 获取当前系统 CPU 使用率 (0.0 - 1.0)
pub fn get_cpu_usage() -> f64 {
    if let Ok(sys) = SYSTEM.lock() {
        (sys.global_cpu_usage() / 100.0) as f64
    } else {
        0.0
    }
}

/// 获取当前系统内存使用率 (0.0 - 1.0)
pub fn get_memory_usage() -> f64 {
    if let Ok(sys) = SYSTEM.lock() {
        let total_mem = sys.total_memory();
        if total_mem > 0 {
            sys.used_memory() as f64 / total_mem as f64
        } else {
            0.0
        }
    } else {
        0.0
    }
}
