// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 缓存配置示例
//!
//! 演示如何配置缓存行为。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("缓存配置示例");
    info!("配置缓存策略和参数");
}
