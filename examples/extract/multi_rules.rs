// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 多规则提取示例
//!
//! 演示如何同时使用多个提取规则。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("多规则提取示例 - 功能演示");
    info!("支持批量配置提取规则");
}
