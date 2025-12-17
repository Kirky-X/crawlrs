// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{http::StatusCode, routing::get, Router};
use crawlrs::engines::health_monitor::{EngineHealth, EngineHealthMonitor, HealthCheckConfig};
use crawlrs::engines::reqwest_engine::ReqwestEngine;
use crawlrs::engines::traits::ScraperEngine;
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

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}/status/200", addr)
}

#[tokio::test]
async fn test_health_monitor_real_integration() {
    let target_url = start_test_server(true).await;

    let engine = Arc::new(ReqwestEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![engine];

    let config = HealthCheckConfig {
        target_url,
        ..Default::default()
    };

    let monitor = EngineHealthMonitor::new_with_config(engines, config);
    monitor.perform_health_check().await;

    let health_info = monitor
        .get_engine_health("reqwest")
        .await
        .expect("Engine should be found");
    assert_eq!(health_info.health, EngineHealth::Healthy);
    assert_eq!(health_info.consecutive_failures, 0);
}

#[tokio::test]
async fn test_health_monitor_real_integration_failure() {
    let target_url = start_test_server(false).await;

    let engine = Arc::new(ReqwestEngine);
    let engines: Vec<Arc<dyn ScraperEngine>> = vec![engine];

    let config = HealthCheckConfig {
        target_url,
        max_consecutive_failures: 1,
        ..Default::default()
    };

    let monitor = EngineHealthMonitor::new_with_config(engines, config);
    monitor.perform_health_check().await;

    let health_info = monitor.get_engine_health("reqwest").await.unwrap();
    assert_eq!(health_info.health, EngineHealth::Unhealthy);
}
