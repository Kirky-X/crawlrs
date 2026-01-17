// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 重试逻辑示例
//!
//! 演示如何配置Webhook重试策略。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("重试逻辑示例");
    info!("配置Webhook递送重试策略");
}
