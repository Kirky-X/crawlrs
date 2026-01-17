// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 输出格式示例
//!
//! 演示不同输出格式的配置和使用。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("输出格式示例 - 功能演示");
    info!("支持JSON、CSV等格式");
}
