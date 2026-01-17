// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 积分管理示例
//!
//! 演示如何管理团队积分配额。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("积分管理示例");
    info!("管理团队积分配额和使用");
}
