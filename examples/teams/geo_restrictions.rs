// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 地理限制管理示例
//!
//! 演示如何配置团队地理访问限制。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("地理限制管理示例");
    info!("配置IP地理访问限制");
}
