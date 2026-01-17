// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 指标导出示例
//!
//! 演示如何导出Prometheus指标。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("指标导出示例");
    info!("导出Prometheus指标");
}
