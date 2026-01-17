// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! TTL管理示例
//!
//! 演示如何管理缓存的生存时间。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("TTL管理示例");
    info!("配置缓存生存时间");
}
