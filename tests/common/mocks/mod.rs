// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Shared mock implementations for tests.
//!
//! - 统一 mock 定义，消除 25+ 重复定义。
//!
//! SDK-layer mocks (`MockSearchService`, `MockTaskQueue`, `MockCrawlRepository`,
//! etc.) live in `src/presentation/sdk/mocks.rs` — import via
//! `crawlrs::presentation::sdk::mocks::*`.
//!
//! Repository/engine/service mocks live here:

pub mod mock_engines;
pub mod mock_repositories;
pub mod mock_services;

// Re-exports for convenience
// 多个测试目标（main / sdk_api_test）共享本模块，各目标用到的 re-export 子集不同，
// 未用到的子集允许告警静默，避免按目标维护条件导出。
#[allow(unused_imports)]
pub use mock_engines::MockScraperEngine;
#[allow(unused_imports)]
pub use mock_repositories::{
    MockCrawlRepository, MockCreditsRepository, MockScrapeResultRepository, MockTaskRepository,
};
#[allow(unused_imports)]
pub use mock_services::{MockCacheService, MockWebhookService};
