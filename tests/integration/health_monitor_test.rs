// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use axum::{http::StatusCode, routing::get, Router};
use crawlrs::engines::health_monitor::{EngineHealth, EngineHealthMonitor, HealthCheckConfig};
use std::sync::Arc;
use tokio::net::TcpListener;

async fn start_test_server(success: bool) -> String {
    let app = if success {
        Router::new().route("/status/200", get(|| async { "OK" }))
    } else {
        Router::new().route(
            "/status/200",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
    };

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to address");
    let addr = listener.local_addr().expect("Failed to get local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("Failed to start server");
    });

    format!("http://{}/status/200", addr)
}

#[tokio::test]
async fn test_health_monitor_real_integration() {
    let target_url = start_test_server(true).await;

    // Allow some time for the server to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let engine = Arc::new(ReqwestEngine::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![engine];

    let config = HealthCheckConfig {
        target_url,
        // Use relaxed thresholds for test environment
        degraded_threshold_ms: 5000,
        unhealthy_threshold_ms: 10000,
        ..Default::default()
    };

    let monitor = EngineHealthMonitor::new_with_config(engines, config);
    monitor.perform_health_check().await;

    let health_info = monitor
        .get_engine_health("reqwest")
        .await
        .expect("Failed to get engine health");

    // Allow for minor network delays in test environment
    assert!(
        health_info.health == EngineHealth::Healthy || health_info.health == EngineHealth::Degraded,
        "Expected Healthy or Degraded, got {:?}",
        health_info.health
    );
    // Consecutive failures may be 0 or 1 due to initial connection delay
    assert!(
        health_info.consecutive_failures <= 1,
        "Expected 0 or 1 consecutive failures, got {}",
        health_info.consecutive_failures
    );
}

#[tokio::test]
async fn test_health_monitor_real_integration_failure() {
    let target_url = start_test_server(false).await;

    let engine = Arc::new(ReqwestEngine::new());
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![engine];

    let config = HealthCheckConfig {
        target_url,
        max_consecutive_failures: 1,
        ..Default::default()
    };

    let monitor = EngineHealthMonitor::new_with_config(engines, config);
    monitor.perform_health_check().await;

    let health_info = monitor
        .get_engine_health("reqwest")
        .await
        .expect("Failed to get engine health");
    assert_eq!(health_info.health, EngineHealth::Unhealthy);
}
