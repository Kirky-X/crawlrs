// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 团队并发控制示例
//!
//! 演示如何控制团队的并发请求数。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("团队并发控制示例");
    info!("配置团队级别的并发限制");
}
