// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 代理认证示例
//!
//! 演示如何配置代理认证。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("代理认证示例");
    info!("配置代理服务器认证");
}
