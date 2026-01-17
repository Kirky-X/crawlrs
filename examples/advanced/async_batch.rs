// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 异步批量处理示例
//!
//! 演示如何进行异步批量数据处理。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("异步批量处理示例");
    info!("并发处理多个任务");
}
