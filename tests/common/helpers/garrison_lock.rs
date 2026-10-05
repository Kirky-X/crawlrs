// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 进程级 garrison 全局单例的跨测试文件互斥锁。

use once_cell::sync::Lazy;
use tokio::sync::Mutex;

/// 串行化所有写入 garrison 全局单例（GARRISON_DAO / audit service）的测试。
///
/// `tests/integration/auth_garrison_test.rs`（E2E：每用例 reset+init 周期）与
/// `tests/unit/bootstrap/routes_test.rs`（OnceCell 进程内只构建一次）写入的是
/// 同一进程全局态；不加锁并行运行时互相触发
/// "set_garrison_dao failed: global DAO already injected"。
pub static GARRISON_GLOBAL_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
