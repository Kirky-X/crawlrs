// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Garrison DAO 工厂与全局态。
//!
//! ## 设计决策（R-key-lifecycle-001）
//!
//! 复用 garrison v0.8.1 内建 [`GarrisonDaoOxcache`]，**不自实现 `CrawlrsGarrisonDao`**。
//! 理由：
//!
//! 1. [`GarrisonDaoOxcache::new()`] 已实现完整 [`GarrisonDao`] trait，自管理 oxcache 实例；
//! 2. 按 proposal「全量重签 + garrison 原生存储」，garrison 用自己的 schema（`garrison:apikey:<ns>:<key>`），
//!    不读 crawlrs 旧 `api_keys`/`scopes` 表，故无需共享 crawlrs 的 `pool`/`cache`；
//! 3. 避免重复造轮子（规则5 简洁优先 + 规则8 惯例优先于新颖）。
//!
//! 若未来需要 L2 持久化（postgres），配合 garrison `init_dbnexus` + `GarrisonMigration` 即可，
//! 仍不需要自实现 `CrawlrsGarrisonDao`。
//!
//! ## 全局 DAO 注入（T027）
//!
//! [`GarrisonManager::init`] 持有 dao 后通过 `GarrisonSession::dao()` 暴露给 garrison
//! 内部模块（`pub(crate)`），但**外部业务代码无法访问**——garrison 0.8.1 未对外暴露
//! `ApiKeyHandler` 单例或 `dao()` 公共 API。
//!
//! 解法（与 `garrison_listener.rs::AUDIT_SERVICE` 一致）：
//! - [`init_garrison_dao`] 创建 dao 后立即 [`set_garrison_dao`] 注入全局态
//! - 业务 handler（`api_key_handler`）通过 [`get_garrison_dao`] 读取
//! - 测试可通过 [`reset_garrison_dao_for_test`] 重置（避免测试间污染）
//!
//! 使用 [`parking_lot::RwLock`]`<Option<…>>` 而非 [`std::sync::OnceLock`] 的理由：
//! - 测试可重置全局态，避免并行测试污染
//! - `read()` 是共享读锁，热路径无竞争（bootstrap 后只读不变）
//! - `set_garrison_dao` 在已有实例时返回 `Err`，避免静默覆盖

use std::sync::Arc;

use garrison::dao::GarrisonDao;
use garrison::dao::GarrisonDaoOxcache;
use parking_lot::RwLock;

/// 全局 [`GarrisonDao`] 引用，由 [`set_garrison_dao`] 注入，由 [`get_garrison_dao`] 读取。
///
/// ## 设计
///
/// 使用 [`parking_lot::RwLock`]`<Option<…>>` 而非 [`std::sync::OnceLock`] 的理由：
/// - 测试可通过 [`reset_garrison_dao_for_test`] 重置全局态，避免测试间污染
/// - `read()` 是共享读锁，热路径无竞争（bootstrap 后只读不变）
/// - `set_garrison_dao` 在已有实例时返回 `Err`，避免静默覆盖
///
/// ## 时序
///
/// `init_garrison_auth` 调用 `init_garrison_dao` 创建 dao 后立即 `set_garrison_dao`，
/// 早于第一个 `POST /v1/admin/api-keys` 请求（远晚于 bootstrap 完成）。
static GARRISON_DAO: RwLock<Option<Arc<dyn GarrisonDao>>> = RwLock::new(None);

/// 测试串行化锁，避免 `GARRISON_DAO` 全局态在并行测试中竞态。
///
/// 涉及 `set_garrison_dao`/`reset_garrison_dao_for_test` 的测试通过此锁串行化。
/// `#[tokio::test]` 默认并行执行，全局 `RwLock` 会污染——串行化是必要折衷。
///
/// 使用 [`tokio::sync::Mutex`]（而非 [`std::sync::Mutex`]）的理由：
/// 测试中需持锁跨 `.await`（如 `init_garrison_dao().await`），`std::sync::Mutex`
/// 在多线程 runtime 持锁跨 await 可能死锁（[`clippy::await_holding_lock`] 警告）。
/// `tokio::sync::Mutex` 是 async-aware，安全持锁跨 await。
///
/// 使用 [`std::sync::OnceLock`] 而非 `const` 初始化的理由：
/// `tokio::sync::Mutex::new` 非 `const fn`，无法在 static 表达式中直接调用。
#[cfg(test)]
pub(crate) static TEST_MUTEX: std::sync::OnceLock<tokio::sync::Mutex<()>> =
    std::sync::OnceLock::new();

/// 测试专用：获取 [`TEST_MUTEX`] 的引用（首次调用时构造实例）。
///
/// 与 `test_helpers::acquire_next_test_mutex` 同一模式（[`std::sync::OnceLock`] +
/// [`tokio::sync::Mutex`]），消除 `tokio::sync::Mutex::new` 非 const 的限制。
#[cfg(test)]
pub(crate) fn test_mutex() -> &'static tokio::sync::Mutex<()> {
    TEST_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 初始化 garrison DAO（复用内建 [`GarrisonDaoOxcache`]）。
///
/// # 生命周期
///
/// 应在应用启动时调用**一次**，将返回的 `Arc<dyn GarrisonDao>` 传入
/// `garrison::manager::GarrisonManager::init`，由 GarrisonManager 管理其生命周期。
/// 每次调用都创建独立的 oxcache 实例（非单例），故不应在请求路径中重复调用。
///
/// # 返回
///
/// - `Ok(Arc<dyn GarrisonDao>)` — garrison 内建 oxcache DAO 实例
/// - `Err(garrison::GarrisonError)` — 内建 DAO 初始化失败（如 oxcache 创建失败）
///
/// # Spec
///
/// - R-key-lifecycle-001
///
/// # 使用示例
///
/// ```no_run
/// # use crawlrs::infrastructure::auth::garrison_dao::init_garrison_dao;
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let dao = init_garrison_dao().await?;
/// # Ok(()) }
/// ```
pub async fn init_garrison_dao() -> garrison::prelude::GarrisonResult<Arc<dyn GarrisonDao>> {
    let dao = GarrisonDaoOxcache::new().await?;
    Ok(Arc::new(dao))
}

/// 注入 garrison [`GarrisonDao`] 实例，供业务 handler 通过 [`get_garrison_dao`] 读取。
///
/// # 调用时序
///
/// 在 [`crate::bootstrap::services::init_garrison_auth`] 中 `init_garrison_dao` 之后、
/// `GarrisonManager::init` 之前调用。早于第一个 `POST /v1/admin/api-keys` 请求
/// （远晚于 bootstrap 完成）。
///
/// # 参数
///
/// - `dao`: `Arc<dyn GarrisonDao>` 实例（来自 `init_garrison_dao`）
///
/// # 返回
///
/// - `Ok(())` — 注入成功（先前为 None）
/// - `Err(dao)` — 已有实例被注入（返回传入的 dao 让调用方处理）
///
/// # Spec
///
/// - R-key-lifecycle-001：业务 handler 需 dao 来构造 `ApiKeyHandler::generate_with_namespace`
pub fn set_garrison_dao(dao: Arc<dyn GarrisonDao>) -> Result<(), Arc<dyn GarrisonDao>> {
    let mut guard = GARRISON_DAO.write();
    if guard.is_some() {
        return Err(dao);
    }
    *guard = Some(dao);
    Ok(())
}

/// 读取全局 [`GarrisonDao`] 引用（clone `Arc`）。
///
/// 供 `api_key_handler` 在请求路径调用——`read()` 是共享读锁，不阻塞其他读者。
/// 返回 `Option<Arc<…>>`，未注入时为 `None`（bootstrap 完成前）。
///
/// # Spec
///
/// - R-key-lifecycle-001
pub fn get_garrison_dao() -> Option<Arc<dyn GarrisonDao>> {
    GARRISON_DAO.read().clone()
}

/// 重置全局 [`GARRISON_DAO`]（仅测试用）。
///
/// 单测在 setup/teardown 中调用以避免测试间全局态污染。
/// 生产代码禁止调用——会导致 dao 丢失，API Key 签发请求返回 500。
#[cfg(test)]
pub(crate) fn reset_garrison_dao_for_test() {
    *GARRISON_DAO.write() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：以下测试必须使用 `multi_thread` flavor。
    //
    // `GarrisonDaoOxcache::new()` 启用 `sync_mode(true)`，后续 `_sync` API
    // （`get_sync`/`set_with_ttl_sync`/...）内部经 `MokaMemoryBackend::sync_block_on`
    // 调用 `tokio::task::block_in_place` 驱动 async moka future。
    // `block_in_place` 在 `current_thread` runtime 中会 panic：
    //   "Cannot start a runtime from within a runtime"
    // （oxcache-0.3.0/src/cache/builder/cache_builder.rs:356-361 注释明确要求
    //  multi_thread flavor，参考其 `test_builder_sync_mode_true_enables_backend_sync`）
    // `worker_threads = 1` 保持单线程语义，仅启用 `block_in_place` 支持。

    /// R-key-lifecycle-001：init_garrison_dao 返回可用 Arc<dyn GarrisonDao>
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_init_garrison_dao_returns_usable_dao() {
        let dao = init_garrison_dao().await;
        assert!(dao.is_ok(), "init_garrison_dao must succeed in memory mode");
        let dao = dao.unwrap();
        // 验证 DAO 基本可用：set/get/delete
        let key = "crawlrs:test:garrison_dao:usable";
        let value = "test_value";
        dao.set(key, value, 60).await.expect("set must succeed");
        let got = dao
            .get(key)
            .await
            .expect("get must succeed")
            .expect("value must exist");
        assert_eq!(got, value);
        dao.delete(key).await.expect("delete must succeed");
        let after_delete = dao.get(key).await.expect("get after delete must succeed");
        assert!(after_delete.is_none(), "value must be deleted");
    }

    /// R-key-lifecycle-001：连续两次调用返回独立 DAO 实例（不共享内部状态）
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_init_garrison_dao_returns_independent_instances() {
        let dao1 = init_garrison_dao().await.unwrap();
        let dao2 = init_garrison_dao().await.unwrap();

        // 在 dao1 写入，dao2 不应看到（独立 oxcache 实例）
        let key = "crawlrs:test:garrison_dao:independence";
        dao1.set(key, "from_dao1", 60).await.unwrap();
        let from_dao2 = dao2.get(key).await.unwrap();
        assert!(
            from_dao2.is_none(),
            "dao2 must not see dao1's writes (independent instances)"
        );
        dao1.delete(key).await.unwrap();
    }

    // ========== T027: 全局 dao set/get/reset_for_test 测试 ==========

    /// T027：set_garrison_dao 首次注入返回 Ok，二次注入返回 Err(dao)
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_set_garrison_dao_first_call_ok_second_err() {
        let _guard = test_mutex().lock().await;
        reset_garrison_dao_for_test();

        let dao1 = init_garrison_dao().await.unwrap();
        let dao2 = init_garrison_dao().await.unwrap();

        // 首次注入成功
        let result = set_garrison_dao(dao1.clone());
        assert!(result.is_ok(), "first set_garrison_dao must succeed");

        // 二次注入返回 Err，包含传入的 dao2
        let result2 = set_garrison_dao(dao2.clone());
        assert!(result2.is_err(), "second set_garrison_dao must return Err");
        let returned = result2.unwrap_err();
        assert!(
            Arc::ptr_eq(&returned, &dao2),
            "returned dao must be the same Arc as dao2"
        );

        reset_garrison_dao_for_test();
    }

    /// T027：get_garrison_dao 在注入前返回 None，注入后返回 Some(clone)
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_garrison_dao_before_and_after_set() {
        let _guard = test_mutex().lock().await;
        reset_garrison_dao_for_test();

        // 注入前：None
        assert!(
            get_garrison_dao().is_none(),
            "get_garrison_dao must return None before set"
        );

        // 注入后：Some(Arc<...>) 指向同一实例
        let dao = init_garrison_dao().await.unwrap();
        match set_garrison_dao(dao.clone()) {
            Ok(()) => {}
            Err(_) => panic!("set_garrison_dao must succeed on first call"),
        }

        let got = get_garrison_dao().expect("must be Some after set");
        assert!(
            Arc::ptr_eq(&got, &dao),
            "get_garrison_dao must return Arc pointing to the same instance"
        );

        reset_garrison_dao_for_test();
    }

    /// T027：reset_garrison_dao_for_test 清空全局态
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_reset_garrison_dao_for_test_clears_global() {
        let _guard = test_mutex().lock().await;
        reset_garrison_dao_for_test();

        let dao = init_garrison_dao().await.unwrap();
        match set_garrison_dao(dao) {
            Ok(()) => {}
            Err(_) => panic!("set_garrison_dao must succeed on first call"),
        }
        assert!(get_garrison_dao().is_some(), "must be Some after set");

        reset_garrison_dao_for_test();
        assert!(
            get_garrison_dao().is_none(),
            "must be None after reset_garrison_dao_for_test"
        );
    }

    /// T027：get_garrison_dao 返回的 Arc 可独立持有（clone 后 reset 不影响）
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_garrison_dao_returned_arc_survives_reset() {
        let _guard = test_mutex().lock().await;
        reset_garrison_dao_for_test();

        let dao = init_garrison_dao().await.unwrap();
        match set_garrison_dao(dao.clone()) {
            Ok(()) => {}
            Err(_) => panic!("set_garrison_dao must succeed on first call"),
        }

        let cloned = get_garrison_dao().expect("must be Some");
        reset_garrison_dao_for_test();

        // 重置后 get 返回 None，但已 clone 出来的 Arc 仍可用
        assert!(get_garrison_dao().is_none(), "must be None after reset");
        assert!(
            Arc::strong_count(&cloned) >= 1,
            "cloned Arc must remain valid after reset"
        );
        // 用 cloned 跑一次 set 验证仍可用
        cloned
            .set("crawlrs:test:after_reset", "v", 60)
            .await
            .expect("cloned Arc must still be usable");
    }
}
