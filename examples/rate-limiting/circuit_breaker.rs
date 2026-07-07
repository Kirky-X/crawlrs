// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 熔断器示例
//!
//! 演示如何使用熔断器保护服务。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("熔断器示例");
    info!("配置熔断保护机制");
}
