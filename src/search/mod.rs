// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 搜索模块
//!
//! 提供统一的搜索引擎客户端和多种搜索引擎实现
//!
//! # 示例
//!
//! ```ignore
//! use crawlrs::search::SearchClient;
//!
//! let results = SearchClient::global()
//!     .search("Rust programming")
//!     .google()
//!     .limit(10)
//!     .execute()
//!     .await?;
//! ```

pub mod ab_test;
#[cfg(feature = "oxcache-cache")]
pub mod aggregator;
#[cfg(feature = "engine-reqwest")]
pub mod client;
pub mod engine_trait;
pub mod error;
#[cfg(feature = "engine-reqwest")]
pub mod factory;
pub mod response;
pub mod router;
#[cfg(feature = "engine-reqwest")]
pub mod smart;
pub mod types;

pub use ab_test::SearchABTestEngine;
#[cfg(feature = "oxcache-cache")]
pub use aggregator::deduplicator::ResultDeduplicator as Deduplicator;
#[cfg(feature = "oxcache-cache")]
pub use aggregator::SearchAggregator;
pub use engine_trait::{SearchEngine, SearchRequest};
pub use error::SearchError;
pub use response::{Response, ResponseItem};
pub use types::{EngineHealth, SearchEngineType};
