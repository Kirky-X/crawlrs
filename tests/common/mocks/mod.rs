// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Shared mock implementations for tests.
//!
//! T042-T043: 统一 mock 定义，消除 25+ 重复定义。
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
pub use mock_engines::MockScraperEngine;
pub use mock_repositories::{
    MockCrawlRepository, MockCreditsRepository, MockScrapeResultRepository, MockTaskRepository,
};
pub use mock_services::{MockCacheService, MockWebhookService};
