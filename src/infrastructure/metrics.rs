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

use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use tracing::info;

pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    let addr: SocketAddr = "0.0.0.0:9000".parse().expect("Invalid metrics address");

    // Start the exporter
    // Ignore error if address is already in use (for development/testing)
    if let Err(e) = builder.with_http_listener(addr).install() {
        tracing::warn!("Failed to install Prometheus recorder: {}. This might happen if the port is already in use.", e);
    }

    info!("Metrics exporter listening on {}", addr);
}
