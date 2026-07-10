// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::Json;
use log::info;
use serde::{Deserialize, Serialize};

/// 指标响应数据传输对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsResponseDto {
    /// 状态
    pub status: String,
    /// 指标数据
    pub metrics: String,
}

pub async fn metrics() -> Json<MetricsResponseDto> {
    info!("Metrics handler called");

    let response = MetricsResponseDto {
        status: "ok".to_string(),
        metrics: "# HELP app_info Information about the application\n# TYPE app_info gauge\napp_info{version=\"1.0.0\"} 1\n# HELP app_uptime_seconds Application uptime in seconds\n# TYPE app_uptime_seconds counter\napp_uptime_seconds 3600".to_string(),
    };

    info!("Metrics response: status={}", response.status);
    Json(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn test_metrics_returns_ok_status() {
        let response = metrics().await;

        assert_eq!(response.status, "ok");
        assert!(!response.metrics.is_empty());
    }

    #[tokio::test]
    async fn test_metrics_response_contains_help_text() {
        let response = metrics().await;

        assert!(response.metrics.contains("# HELP"));
        assert!(response.metrics.contains("# TYPE"));
    }

    #[tokio::test]
    async fn test_metrics_response_contains_app_info() {
        let response = metrics().await;

        assert!(response.metrics.contains("app_info"));
        assert!(response.metrics.contains("app_uptime_seconds"));
    }

    #[tokio::test]
    async fn test_metrics_response_dto_serialization() {
        let dto = MetricsResponseDto {
            status: "ok".to_string(),
            metrics: "test_metric 42".to_string(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["metrics"], "test_metric 42");
    }

    #[tokio::test]
    async fn test_metrics_response_dto_deserialization() {
        let json = r##"{"status":"ok","metrics":"# HELP test test"}"##;
        let dto: MetricsResponseDto = serde_json::from_str(json).unwrap();

        assert_eq!(dto.status, "ok");
        assert_eq!(dto.metrics, "# HELP test test");
    }

    #[tokio::test]
    async fn test_metrics_json_body_structure() {
        let response = metrics().await.into_response();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed["status"], "ok");
        assert!(parsed["metrics"].is_string());
    }
}
