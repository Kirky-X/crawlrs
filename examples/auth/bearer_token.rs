// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bearer Token认证示例
//!
//! 演示如何使用Bearer Token进行身份验证。

use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("Bearer Token认证示例");
    info!("使用OAuth2 Bearer Token进行身份验证");
}
