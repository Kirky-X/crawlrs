// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::Json;
use serde_json::json;
use tracing::info;

pub async fn metrics() -> Json<serde_json::Value> {
    info!("Metrics handler called");
    
    let response = json!({
        "status": "ok",
        "metrics": "# HELP app_info Information about the application\n# TYPE app_info gauge\napp_info{version=\"1.0.0\"} 1\n# HELP app_uptime_seconds Application uptime in seconds\n# TYPE app_uptime_seconds counter\napp_uptime_seconds 3600"
    });
    
    info!("Metrics response: {:?}", response);
    Json(response)
}