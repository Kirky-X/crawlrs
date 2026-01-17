// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础Webhook配置示例
//!
//! 演示如何配置Webhook通知。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("基础Webhook配置示例");
    info!("配置Webhook接收任务事件通知");
}
