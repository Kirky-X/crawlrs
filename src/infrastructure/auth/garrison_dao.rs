// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Garrison DAO 工厂。
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

use std::sync::Arc;

use garrison::dao::GarrisonDao;
use garrison::dao::GarrisonDaoOxcache;

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
}
