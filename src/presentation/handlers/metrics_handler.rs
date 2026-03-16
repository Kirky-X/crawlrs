// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::info;

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
