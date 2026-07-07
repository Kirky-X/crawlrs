// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 团队隔离示例
//!
//! 演示如何在多租户环境中实现资源隔离。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("团队隔离示例");
    info!("多租户环境下的资源隔离机制");
}
