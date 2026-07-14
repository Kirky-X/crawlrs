// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! oxcache 缓存示例
//!
//! 演示如何使用 oxcache（moka 内存后端）进行缓存。

use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);
    info!("oxcache 缓存示例");
    info!("配置和使用 oxcache（moka memory backend）缓存");
}
