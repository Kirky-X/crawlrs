// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 指标导出示例
//!
//! 演示如何从 `/metrics` 端点获取 Prometheus 格式的监控指标，
//! 以及如何通过 `EngineClient` 的内置指标了解引擎运行状态。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example metrics_export
//! ```

use crawlrs::engines::engine_client::{EngineClient, ScrapeRequest};
use log::info;
use std::time::Duration;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 指标导出示例");
    info!("=====================================\n");

    // 1. 通过 HTTP 获取 Prometheus 指标
    info!("1️⃣  通过 /metrics 端点获取指标");
    info!("-----------------------------");

    let client = EngineClient::new();
    let metrics_request = ScrapeRequest::new("http://localhost:8899/metrics")
        .timeout(Duration::from_secs(5));

    match client.scrape(&metrics_request).await {
        Ok(response) => {
            info!("  ✅ 指标获取成功");
            info!("  状态码: {}", response.status_code);
            // 显示指标内容（Prometheus 文本格式）
            let preview = &response.content[..response.content.len().min(500)];
            info!("  指标预览:\n{}", preview);
        }
        Err(e) => {
            info!("  ⚠️  无法连接本地服务 (需要先启动 crawlrs): {:?}", e);
            info!("  💡 启动服务: cargo run --bin crawlrs");
        }
    }

    info!("");

    // 2. 爬取后查看引擎内部指标
    info!("2️⃣  引擎操作后查看指标变化");
    info!("-----------------------------");

    // 执行几次爬取操作产生指标
    let urls = vec![
        "https://example.com",
        "https://httpbin.org/html",
    ];

    for url in &urls {
        let request = ScrapeRequest::new(*url).timeout(Duration::from_secs(10));
        match client.scrape(&request).await {
            Ok(response) => {
                info!("  ✅ {} — HTTP {}, {} 字节", url, response.status_code, response.content.len());
            }
            Err(e) => {
                info!("  ❌ {} — {:?}", url, e);
            }
        }
    }

    info!("");
    info!("  💡 引擎内部通过 RouterMetrics 收集以下指标:");
    info!("     - total_requests: 总请求数");
    info!("     - successful_requests: 成功请求数");
    info!("     - failed_requests: 失败请求数");
    info!("     - engine_latencies: 按引擎名称的延迟统计");
    info!("     - engine_selection_total: 引擎选择次数");
    info!("     - failure_classification: 按错误类型的失败统计");

    info!("");

    // 3. Prometheus 集成说明
    info!("3️⃣  Prometheus 集成");
    info!("-----------------------------");
    info!("  crawlrs 在 /metrics 端点暴露 Prometheus 格式指标:");
    info!("");
    info!("  # 基础指标");
    info!("  app_info{{version=\"x.y.z\"}} 1");
    info!("  app_uptime_seconds 3600");
    info!("");
    info!("  # 引擎路由指标（需启用 metrics feature）");
    info!("  engine_request_total{{engine=\"reqwest\", status=\"success\"}} 42");
    info!("  engine_request_duration_seconds{{engine=\"reqwest\"}} 0.5");
    info!("  circuit_breaker_state{{engine=\"playwright\", state=\"closed\"}} 1");
    info!("");
    info!("  💡 在 prometheus.yml 中添加 scrape 配置:");
    info!("     scrape_configs:");
    info!("       - job_name: 'crawlrs'");
    info!("         static_configs:");
    info!("           - targets: ['localhost:8899']");

    info!("\n=====================================");
    info!("✨ 指标导出示例完成");
}
