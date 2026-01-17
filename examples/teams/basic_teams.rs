// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础团队管理示例
//!
//! 演示如何创建和管理团队。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("基础团队管理示例");
    info!("创建和管理团队");
}
