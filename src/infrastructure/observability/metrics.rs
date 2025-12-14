// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use metrics::{
    describe_counter, describe_gauge, describe_histogram, register_counter, register_gauge,
    register_histogram,
};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;

/// 初始化指标系统
///
/// 配置并注册应用所需的各类监控指标
pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    builder
        .install()
        .expect("failed to install Prometheus recorder");

    // Register metrics
    describe_counter!("crawl_tasks_total", "Total number of crawl tasks submitted");
    register_counter!("crawl_tasks_total");

    describe_counter!(
        "crawl_tasks_completed_total",
        "Total number of crawl tasks completed"
    );
    register_counter!("crawl_tasks_completed_total");

    describe_counter!(
        "crawl_tasks_failed_total",
        "Total number of crawl tasks failed"
    );
    register_counter!("crawl_tasks_failed_total");

    describe_histogram!(
        "crawl_duration_seconds",
        "Duration of crawl tasks in seconds"
    );
    register_histogram!("crawl_duration_seconds");

    // Circuit Breaker Metrics
    describe_counter!(
        "circuit_breaker_requests_total",
        "Total number of requests processed by circuit breaker"
    );
    register_counter!("circuit_breaker_requests_total");

    describe_counter!(
        "circuit_breaker_failures_total",
        "Total number of failed requests recorded by circuit breaker"
    );
    register_counter!("circuit_breaker_failures_total");

    describe_counter!(
        "circuit_breaker_successes_total",
        "Total number of successful requests recorded by circuit breaker"
    );
    register_counter!("circuit_breaker_successes_total");

    describe_counter!(
        "circuit_breaker_rejected_total",
        "Total number of requests rejected by open circuit breaker"
    );
    register_counter!("circuit_breaker_rejected_total");

    describe_gauge!(
        "circuit_breaker_status",
        "Current status of circuit breaker (0=Closed, 0.5=HalfOpen, 1=Open)"
    );
    register_gauge!("circuit_breaker_status");
}
