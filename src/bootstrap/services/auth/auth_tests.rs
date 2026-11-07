// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `init_garrison_auth` 单元测试（auth feature）。
//!
//! 测试覆盖三类场景：
//! 1. 弱密钥 / 空密钥 → Err(BootstrapError::GarrisonConfig)（fail-fast 类型化）
//! 2. 强密钥 + 真实 DbPool → Ok(())（标记 #[ignore] 由集成测试覆盖）
//!
//! 注意：DbPool 在 dbnexus 0.4.0 无 Default 实现（需真实连接池），故弱密钥测试也
//! 依赖 TEST_DATABASE_URL 提供真实 pool。但弱密钥场景下 pool 不会被使用（config
//! 校验在第 1 步即返回 Err），故 create_test_db_pool 仅用于满足函数签名。

use super::init_garrison_auth;
use crate::bootstrap::error::BootstrapError;
use crate::common::test_helpers::{create_test_db_pool, skip_if_no_test_db};
use crate::config::settings::Settings;

/// 构造测试用 Settings（仅填充 auth.jwt_secret，其他字段走 confers 默认）。
fn make_settings(jwt_secret: &str) -> Settings {
    let mut settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
    settings.auth.jwt_secret = jwt_secret.to_string();
    settings
}

/// 空 jwt_secret 返回 `Err(BootstrapError::GarrisonConfig)`，
/// 错误消息含 "garrison config error" 与 EmptySecret Display（"must not be empty or missing"）。
///
/// 验证“失败必须显性化”——空密钥不藏默认值背后，而是返回类型化错误。
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_init_garrison_auth_rejects_empty_secret() {
    if skip_if_no_test_db() {
        return;
    }
    let settings = make_settings("");
    // pool 不会被触达（config 校验先失败），但需满足函数签名。
    let pool = create_test_db_pool();
    let result = init_garrison_auth(&settings, pool).await;
    assert!(
        result.is_err(),
        "empty jwt_secret must be rejected with Err(BootstrapError::GarrisonConfig)"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, BootstrapError::GarrisonConfig(_)),
        "error should be BootstrapError::GarrisonConfig variant, got: {err}"
    );
    assert!(
        format!("{err}").contains("garrison config error"),
        "error Display should contain 'garrison config error' prefix, got: {err}"
    );
    assert!(
        format!("{err}").contains("must not be empty or missing"),
        "error Display should contain EmptySecret description, got: {err}"
    );
}

/// 弱密钥（<32 字节）返回 `Err(BootstrapError::GarrisonConfig)`，
/// 错误消息含 WeakSecret Display（"weak jwt_secret: length N < 32 bytes"）。
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_init_garrison_auth_rejects_weak_secret() {
    if skip_if_no_test_db() {
        return;
    }
    let weak = "too_short"; // 9 字节
    let settings = make_settings(weak);
    let pool = create_test_db_pool();
    let result = init_garrison_auth(&settings, pool).await;
    assert!(
        result.is_err(),
        "weak jwt_secret (<32 bytes) must be rejected with Err(BootstrapError::GarrisonConfig)"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, BootstrapError::GarrisonConfig(_)),
        "error should be BootstrapError::GarrisonConfig variant, got: {err}"
    );
    // 验证错误消息含长度信息（验证类型化错误传递 len/min 字段到 Display）
    let display = format!("{err}");
    assert!(
        display.contains("length 9"),
        "error Display should contain 'length 9', got: {display}"
    );
    assert!(
        display.contains("< 32 bytes"),
        "error Display should contain '< 32 bytes', got: {display}"
    );
}

/// 强密钥 + 真实 DbPool → `Ok(())`，garrison 单例成功初始化。
///
/// 此测试为集成测试，前置条件：
/// 1. `TEST_DATABASE_URL` 指向已运行 garrison postgres migrations 的数据库
/// 2. `GarrisonManager::init` 不被其他并发测试污染（GARRISON_MANAGER 单例）
///
/// **标记 `#[ignore]`**：完整端到端测试在 (`tests/integration/auth_garrison_test.rs`) 进行。
/// 此处保留测试骨架作为占位，避免伪装成有效测试（测试要验证有意义属性）。
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "集成测试覆盖：需真实 DB + garrison migrations + GARRISON_MANAGER 单例隔离"]
async fn test_init_garrison_auth_strong_secret_returns_ok() {
    let strong = "a-very-strong-secret-key-32-bytes-or-more!!"; // 44 字节
    let settings = make_settings(strong);

    // 跳过条件：无 TEST_DATABASE_URL（与 garrison_interface.rs 测试一致）
    if skip_if_no_test_db() {
        return;
    }
    let pool = create_test_db_pool();

    // init_garrison_auth 会注入进程级 DAO 单例：必须持 garrison 全局锁
    // 并先 reset，否则与并发构建全量 state 的测试（bootstrap::routes
    // tests、workers scrape_worker tests）互相触发
    // "set_garrison_dao failed: global DAO already injected"。
    let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
    let result = init_garrison_auth(&settings, pool).await;
    assert!(
        result.is_ok(),
        "strong jwt_secret + valid DbPool should succeed, got: {:?}",
        result.err()
    );
}
