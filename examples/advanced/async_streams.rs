// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 异步流处理示例
//!
//! 演示如何使用异步流处理数据。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("异步流处理示例");
    info!("使用async stream处理数据");
}
