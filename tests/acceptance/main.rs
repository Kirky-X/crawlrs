// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! BDD 验收套件入口（bdd-acceptance-hardening）。
//!
//! 运行：`cargo test --test acceptance --features platform`
//! 前置：Docker 运行中（testcontainers 自动起 PostgreSQL 并应用 migrations）。

mod support;

use cucumber::World;
use support::AcceptanceWorld;

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime for acceptance suite");
    rt.block_on(async {
        AcceptanceWorld::cucumber()
            // garrison GarrisonManager 为进程级单例且不可重置，场景必须串行
            .max_concurrent_scenarios(Some(1))
            .run_and_exit("tests/acceptance/features")
            .await;
    });
}
