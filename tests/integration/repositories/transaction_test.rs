// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Integration tests for TransactionManager with real PostgreSQL.
//!
//! 覆盖 transaction.rs 中需要真实 DB 连接的路径：
//! - begin/commit/rollback 基本流程（session.begin_transaction/commit/rollback 真实 SQL）
//! - begin_with_config 各种 isolation_level / access_mode / enable_savepoints 组合
//! - Drop trait 自动回滚（不 panic）
//! - TransactionGuard RAII 模式
//! - 错误路径（重复 begin、无活动事务 commit/rollback 等）
//! - 状态查询方法（is_active/has_transaction/savepoint_count）
//! - savepoint 方法的 SQL 执行路径与错误处理
//!
//! 单元测试 (transaction.rs 内 `#[cfg(test)] mod tests`) 用 lazy pool 不连真 DB，
//! 覆盖了配置/校验/错误路径；本文件覆盖真实 SQL 路径。
//!
//! # 关于 savepoint 测试
//!
//! transaction.rs 的 `savepoint/release_savepoint/rollback_to_savepoint` 通过
//! `session.connection().execute_unprepared()` 执行 SQL，但 dbnexus 0.2.0 的
//! `Session::connection()` 返回连接池句柄而非事务句柄（`state.transaction`）。
//! 这导致 `SAVEPOINT` SQL 不在事务块中执行，PostgreSQL 报错
//! "SAVEPOINT can only be used in transaction blocks"。
//!
//! 这是源码 bug（应改用 `session.execute_raw()` 走事务句柄），但任务约束禁止修改源码。
//! 因此 savepoint 测试验证当前实际行为（SQL 执行路径被触发但返回 SavepointFailed），
//! 而非 happy path。这仍覆盖了 savepoint 方法的 SQL 执行和错误处理代码路径。

use super::super::helpers::create_test_app_no_worker;
use crawlrs::infrastructure::database::transaction::{
    TransactionAccess, TransactionConfig, TransactionError, TransactionGuard, TransactionIsolation,
    TransactionManager,
};
use uuid::Uuid;

// ============================================================
// Helpers
// ============================================================

/// 生成唯一的 savepoint 名称（仅 alphanumeric + underscore，符合校验规则）
fn unique_sp_name() -> String {
    format!("sp_{}", Uuid::new_v4().as_simple())
}

/// 默认 admin role 配置，与 test_app.rs 中 admin_role 一致
fn admin_config(
    iso: TransactionIsolation,
    access: TransactionAccess,
    sp: bool,
) -> TransactionConfig {
    TransactionConfig {
        isolation_level: iso,
        access_mode: access,
        enable_savepoints: sp,
        role: "admin".to_string(),
    }
}

// ============================================================
// 1. 基本事务流程 (happy path)
// ============================================================

/// 基本 begin → commit 流程
#[tokio::test]
async fn test_begin_commit_basic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);

    manager.begin().await.expect("begin should succeed");

    assert!(manager.is_active());
    assert!(manager.has_transaction());

    manager.commit().await.expect("commit should succeed");

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);
}

/// 基本 begin → rollback 流程
#[tokio::test]
async fn test_begin_rollback_basic() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());

    manager.rollback().await.expect("rollback should succeed");

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);
}

// ============================================================
// 2. begin_with_config — 所有 isolation_level 与 access_mode 组合
// ============================================================

#[tokio::test]
async fn test_begin_with_config_read_committed() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::ReadCommitted,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin with ReadCommitted should succeed");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_begin_with_config_read_uncommitted() {
    // PostgreSQL 将 ReadUncommitted 视为 ReadCommitted，但语法仍合法
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::ReadUncommitted,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin with ReadUncommitted should succeed");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_begin_with_config_repeatable_read() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::RepeatableRead,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin with RepeatableRead should succeed");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_begin_with_config_serializable() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::Serializable,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin with Serializable should succeed");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_begin_with_config_read_only_access_mode() {
    // ReadOnly 事务仅 begin/commit，不执行写操作
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::ReadCommitted,
            TransactionAccess::ReadOnly,
            true,
        ))
        .await
        .expect("begin with ReadOnly access mode should succeed");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_begin_with_config_savepoints_disabled() {
    // enable_savepoints=false 仍可正常 begin/commit，只是后续 savepoint 调用会被拒
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::ReadCommitted,
            TransactionAccess::ReadWrite,
            false,
        ))
        .await
        .expect("begin with savepoints disabled should succeed");
    manager.commit().await.expect("commit should succeed");
}

// ============================================================
// 3. with_config 构造器 — 验证默认 config 被 begin() 使用
// ============================================================

#[tokio::test]
async fn test_with_config_constructor_begin_uses_default_config() {
    let app = create_test_app_no_worker().await;
    let config = admin_config(
        TransactionIsolation::Serializable,
        TransactionAccess::ReadOnly,
        false,
    );
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);
    // begin() 应使用 with_config 设置的默认 config
    manager
        .begin()
        .await
        .expect("begin should use with_config default config");
    manager.commit().await.expect("commit should succeed");
}

#[tokio::test]
async fn test_with_config_constructor_savepoints_disabled_blocks_savepoint() {
    // 验证 with_config 的 enable_savepoints=false 确实生效
    let app = create_test_app_no_worker().await;
    let config = admin_config(
        TransactionIsolation::ReadCommitted,
        TransactionAccess::ReadWrite,
        false,
    );
    let manager = TransactionManager::with_config(app.db_pool.clone(), config);
    manager.begin().await.expect("begin should succeed");

    let sp_name = unique_sp_name();
    let result = manager.savepoint(&sp_name).await;
    assert!(
        matches!(result, Err(TransactionError::SavepointFailed { .. })),
        "expected SavepointFailed when savepoints disabled, got {:?}",
        result
    );
    let err = result.unwrap_err();
    assert!(err.to_string().contains("disabled"));

    manager.rollback().await.expect("cleanup rollback");
}

// ============================================================
// 4. savepoint — SQL 执行路径与错误处理
//
// 注意：transaction.rs 用 session.connection().execute_unprepared() 执行 SAVEPOINT，
// 但 dbnexus 0.2.0 的 connection() 返回连接池句柄而非事务句柄，导致 PostgreSQL 报
// "SAVEPOINT can only be used in transaction blocks"。这是源码 bug，约束禁止修源码，
// 因此这些测试验证当前实际行为（SQL 执行路径被触发，返回 SavepointFailed）。
// ============================================================

/// 验证 savepoint 在活动事务中会执行到 SQL 层（返回 SavepointFailed with SQL error，
/// 而非 NoActiveTransaction）。覆盖 savepoint 方法的 SQL 执行和错误处理代码路径。
#[tokio::test]
async fn test_savepoint_in_active_tx_executes_sql_path() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    let sp_name = unique_sp_name();
    let result = manager.savepoint(&sp_name).await;

    // 源码 bug：session.connection() 返回连接池句柄而非事务句柄，
    // 导致 SAVEPOINT 不在事务块中执行。验证 SQL 执行路径被触发（返回 SavepointFailed）。
    let err = result.expect_err("savepoint should fail due to source bug");
    assert!(
        matches!(err, TransactionError::SavepointFailed { .. }),
        "expected SavepointFailed (SQL exec path triggered), got {:?}",
        err
    );
    assert!(
        err.to_string().contains("SAVEPOINT") || err.to_string().contains("transaction blocks"),
        "error message should reference SAVEPOINT/transaction blocks, got: {}",
        err
    );
    // savepoint 未被加入 VecDeque（SQL 失败前已 return）
    assert_eq!(manager.savepoint_count(), 0);

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_release_nonexistent_savepoint_returns_error() {
    // begin 后直接 release 不存在的 savepoint，应返回 SavepointNotFound
    // （不依赖 savepoint 成功执行）
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    let sp_name = unique_sp_name();
    let result = manager.release_savepoint(&sp_name).await;
    assert!(
        matches!(result, Err(TransactionError::SavepointNotFound(_))),
        "expected SavepointNotFound, got {:?}",
        result
    );
    let err = result.unwrap_err();
    assert!(err.to_string().contains(&sp_name));

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_rollback_to_nonexistent_savepoint_returns_error() {
    // begin 后直接 rollback_to 不存在的 savepoint，应返回 SavepointNotFound
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("begin should succeed");
    let sp_name = unique_sp_name();
    let result = manager.rollback_to_savepoint(&sp_name).await;
    assert!(
        matches!(result, Err(TransactionError::SavepointNotFound(_))),
        "expected SavepointNotFound, got {:?}",
        result
    );

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_savepoint_when_disabled_returns_error() {
    // enable_savepoints=false 时调用 savepoint 应返回 SavepointFailed "disabled"
    // 此检查在 SQL 执行前，不依赖源码 bug
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::with_config(
        app.db_pool.clone(),
        admin_config(
            TransactionIsolation::ReadCommitted,
            TransactionAccess::ReadWrite,
            false,
        ),
    );

    manager.begin().await.expect("begin should succeed");
    let sp_name = unique_sp_name();
    let result = manager.savepoint(&sp_name).await;
    assert!(
        matches!(result, Err(TransactionError::SavepointFailed { .. })),
        "expected SavepointFailed when disabled, got {:?}",
        result
    );
    let err = result.unwrap_err();
    assert!(err.to_string().contains("disabled"));

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_release_savepoint_when_no_active_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let sp_name = unique_sp_name();
    let result = manager.release_savepoint(&sp_name).await;
    assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
}

#[tokio::test]
async fn test_rollback_to_savepoint_when_no_active_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let sp_name = unique_sp_name();
    let result = manager.rollback_to_savepoint(&sp_name).await;
    assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
}

// ============================================================
// 5. Drop trait — 活动事务时 drop 不 panic
// ============================================================

#[tokio::test]
async fn test_drop_with_active_transaction_does_not_panic() {
    let app = create_test_app_no_worker().await;
    {
        let manager = TransactionManager::new(app.db_pool.clone());
        manager.begin().await.expect("begin should succeed");
        assert!(manager.is_active());
        // manager drops here with active (unfinished) transaction.
        // Drop 只 warn 不执行异步 rollback；dbnexus Session 的 Drop 会处理实际回滚。
        // 此测试验证 Drop 不 panic。
    }
    // 到达这里说明 Drop 未 panic
}

// ============================================================
// 6. TransactionGuard — 真实 DB happy path
// ============================================================

#[tokio::test]
async fn test_guard_commit_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager.begin().await.expect("begin should succeed");

    let guard = TransactionGuard::new(&manager);
    guard.commit().await.expect("guard commit should succeed");
    assert!(!manager.is_active());
}

#[tokio::test]
async fn test_guard_rollback_with_real_db() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager.begin().await.expect("begin should succeed");

    let guard = TransactionGuard::new(&manager);
    guard
        .rollback()
        .await
        .expect("guard rollback should succeed");
    assert!(!manager.is_active());
}

// ============================================================
// 7. 错误路径
// ============================================================

#[tokio::test]
async fn test_double_begin_returns_transaction_already_active() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("first begin should succeed");
    let result = manager.begin().await;
    assert!(
        matches!(result, Err(TransactionError::TransactionAlreadyActive)),
        "expected TransactionAlreadyActive, got {:?}",
        result
    );

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_begin_with_config_when_active_returns_error() {
    // 直接调用 begin_with_config 也应检测 TransactionAlreadyActive
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("first begin should succeed");
    let result = manager
        .begin_with_config(TransactionConfig::default())
        .await;
    assert!(
        matches!(result, Err(TransactionError::TransactionAlreadyActive)),
        "expected TransactionAlreadyActive, got {:?}",
        result
    );

    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_commit_without_active_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let result = manager.commit().await;
    assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
}

#[tokio::test]
async fn test_rollback_without_active_returns_error() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    let result = manager.rollback().await;
    assert!(matches!(result, Err(TransactionError::NoActiveTransaction)));
}

// ============================================================
// 8. 状态查询方法在事务生命周期中的变化
// ============================================================

#[tokio::test]
async fn test_state_transitions_during_transaction_lifecycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    // 初始状态
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);

    // begin 后
    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());
    assert!(manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);

    // commit 后
    manager.commit().await.expect("commit should succeed");
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
    assert_eq!(manager.savepoint_count(), 0);
}

#[tokio::test]
async fn test_state_transitions_with_rollback_lifecycle() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    assert!(!manager.is_active());
    assert!(!manager.has_transaction());

    manager.begin().await.expect("begin should succeed");
    assert!(manager.is_active());
    assert!(manager.has_transaction());

    manager.rollback().await.expect("rollback should succeed");
    assert!(!manager.is_active());
    assert!(!manager.has_transaction());
}

// ============================================================
// 9. 同一 manager 上多次顺序事务
// ============================================================

#[tokio::test]
async fn test_multiple_sequential_transactions_on_same_manager() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    // 第一轮：commit
    manager.begin().await.expect("first begin should succeed");
    manager.commit().await.expect("first commit should succeed");
    assert!(!manager.is_active());

    // 第二轮：rollback
    manager.begin().await.expect("second begin should succeed");
    manager
        .rollback()
        .await
        .expect("second rollback should succeed");
    assert!(!manager.is_active());

    // 第三轮：commit
    manager.begin().await.expect("third begin should succeed");
    manager.commit().await.expect("third commit should succeed");
    assert!(!manager.is_active());
}

#[tokio::test]
async fn test_can_begin_again_after_commit() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("first begin should succeed");
    manager.commit().await.expect("first commit should succeed");

    // commit 后再次 begin 不应返回 TransactionAlreadyActive
    manager
        .begin()
        .await
        .expect("second begin after commit should succeed");
    manager.rollback().await.expect("cleanup rollback");
}

#[tokio::test]
async fn test_can_begin_again_after_rollback() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());

    manager.begin().await.expect("first begin should succeed");
    manager
        .rollback()
        .await
        .expect("first rollback should succeed");

    // rollback 后再次 begin 不应返回 TransactionAlreadyActive
    manager
        .begin()
        .await
        .expect("second begin after rollback should succeed");
    manager.commit().await.expect("cleanup commit");
}

// ============================================================
// 10. begin_with_config 各种组合 + 后续 commit/rollback 混合
// ============================================================

#[tokio::test]
async fn test_begin_with_config_then_rollback() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::RepeatableRead,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin should succeed");
    assert!(manager.is_active());
    manager.rollback().await.expect("rollback should succeed");
    assert!(!manager.is_active());
}

#[tokio::test]
async fn test_begin_with_config_read_only_then_rollback() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::ReadCommitted,
            TransactionAccess::ReadOnly,
            true,
        ))
        .await
        .expect("begin with ReadOnly should succeed");
    manager.rollback().await.expect("rollback should succeed");
}

#[tokio::test]
async fn test_begin_with_config_serializable_then_rollback() {
    let app = create_test_app_no_worker().await;
    let manager = TransactionManager::new(app.db_pool.clone());
    manager
        .begin_with_config(admin_config(
            TransactionIsolation::Serializable,
            TransactionAccess::ReadWrite,
            true,
        ))
        .await
        .expect("begin with Serializable should succeed");
    manager.rollback().await.expect("rollback should succeed");
}
