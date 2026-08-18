// Copyright (c) 2026 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 临时诊断测试：打印 sdforge `build()` 实际注册的路由表。
//!
//! 用于排查 `#[forge]` 路由 404（inventory 收集失效/版本前缀错位）。
//! 依赖 presentation（platform 门控），非平台构建时跳过。

use crawlrs::presentation::sdk::build_sdk_router;

#[test]
fn dump_routes() {
    let router = build_sdk_router();
    let dbg = format!("{:?}", router);
    println!("ROUTES: {}", &dbg[..dbg.len().min(3000)]);
}
