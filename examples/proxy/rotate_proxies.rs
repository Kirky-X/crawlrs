// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理轮换示例
//!
//! 演示如何实现代理IP轮换。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("代理轮换示例");
    info!("实现代理IP自动轮换");
}
