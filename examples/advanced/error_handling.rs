// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 错误处理模式示例
//!
//! 演示最佳错误处理实践。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("错误处理模式示例");
    info!("优雅的错误处理机制");
}
