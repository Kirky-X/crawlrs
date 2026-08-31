// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE in the project root for full license information.

//! BDD 验收套件入口（bdd-acceptance-hardening）。
//!
//! 运行：`cargo test --test acceptance --features platform`
//! 前置：Docker 运行中（testcontainers 自动起 PostgreSQL 并应用 migrations）。
//!
//! 离线模式：`CRAWLRS_ACCEPTANCE_OFFLINE=1` 时跳过 `@requires-internet` 标记的
//! 场景（真实搜索引擎结果依赖外网，CI 不可控）。CI job 固定启用离线模式。

mod support;

use cucumber::World;
use support::AcceptanceWorld;

/// 需外网连通性的场景标记。
const EXTERNAL_NETWORK_TAG: &str = "requires-internet";

fn main() {
    let offline = std::env::var("CRAWLRS_ACCEPTANCE_OFFLINE")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime for acceptance suite");
    rt.block_on(async {
        AcceptanceWorld::cucumber()
            // garrison GarrisonManager 为进程级单例且不可重置，场景必须串行
            .max_concurrent_scenarios(Some(1))
            .filter_run_and_exit("tests/acceptance/features", move |_, _, sc| {
                !(offline && sc.tags.iter().any(|t| t == EXTERNAL_NETWORK_TAG))
            })
            .await;
    });
}
