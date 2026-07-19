// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Shared mock implementations for integration tests.
//!
//! SDK-layer mocks (`MockSearchService`, `MockTaskQueue`, `MockCrawlRepository`,
//! etc.) have moved to `src/presentation/sdk/mocks.rs` so they can be shared
//! between the lib's own `#[cfg(test)]` unit tests and the integration tests
//! here. Import them via `crawlrs::presentation::sdk::mocks::*`.
