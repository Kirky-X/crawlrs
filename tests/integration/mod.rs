// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// garrison 认证端到端集成测试（R-auth-engine-003 / T031）。
///
/// 所有测试用例均 `#[ignore]` 标记——需真实 DB + garrison 单例，
/// 手动运行：`cargo test --test main -- --ignored auth_garrison_test`
pub mod auth_garrison_test;
pub mod helpers;
pub mod repositories;

/// WreqEngine JA3/JA4 指纹端到端测试（T024）。
///
/// 需 `engine-tls-fingerprint` feature（含 BoringSSL 构建）。用例 `#[ignore]`，
/// 手动运行：`cargo test --test main --features full,engine-tls-fingerprint -- --ignored wreq_fingerprint_test`
#[cfg(feature = "engine-tls-fingerprint")]
pub mod wreq_fingerprint_test;
