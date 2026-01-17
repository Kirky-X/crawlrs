// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 任务事件订阅示例
//!
//! 演示如何订阅特定的任务事件。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("任务事件订阅示例");
    info!("订阅爬取任务的生命周期事件");
}
