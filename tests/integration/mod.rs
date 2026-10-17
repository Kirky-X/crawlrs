// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

pub mod auth_garrison_test;
/// garrison 认证端到端集成测试。
///
/// 所有测试用例均 `#[ignore]` 标记——需真实 DB + garrison 单例，
/// 手动运行：`cargo test --test main -- --ignored auth_garrison_test`
///
/// `garrison_lock` 供 `auth_garrison_test` 引用共享的 garrison 全局锁
/// （与 `main` 目标经 tests/common 挂载的是同一份源文件）；本目标根是
/// tests/integration/，只挂所需单文件，避免整棵 common 的宏被双重装入。
#[path = "../common/helpers/garrison_lock.rs"]
pub mod garrison_lock;
pub mod helpers;
pub mod repositories;
/// SearXNG 生命周期端到端集成测试（真实 PostgreSQL）
pub mod searxng_lifecycle_test;
/// SearXNG 实例登记簿仓储集成测试（真实 PostgreSQL）
pub mod searxng_repo_test;

/// WreqEngine JA3/JA4 指纹端到端测试。
///
/// 需 `engine-tls-fingerprint` feature（含 BoringSSL 构建）。用例 `#[ignore]`，
/// 手动运行：`cargo test --test main --features full,engine-tls-fingerprint -- --ignored wreq_fingerprint_test`
#[cfg(feature = "engine-tls-fingerprint")]
pub mod wreq_fingerprint_test;
