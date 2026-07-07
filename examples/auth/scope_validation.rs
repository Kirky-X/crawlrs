// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 作用域验证示例
//!
//! 演示如何使用API作用域控制访问权限。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("作用域验证示例");
    info!("配置API作用域控制访问权限");
}
