// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 自定义引擎集成示例
//!
//! 演示如何集成自定义爬取引擎。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("自定义引擎集成示例");
    info!("扩展自定义爬取引擎");
}
