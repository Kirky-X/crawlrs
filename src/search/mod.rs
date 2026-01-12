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
pub mod adapter;
pub mod aggregator;
pub mod client;
pub mod engine_trait;
pub mod error;
pub mod factory;
pub mod response;
pub mod router;
pub mod smart;
pub mod types;

pub use ab_test::SearchABTestEngine;
pub use adapter::{GenericSearchEngineAdapter, SearchEngineAdapter};
pub use aggregator::deduplicator::ResultDeduplicator as Deduplicator;
pub use aggregator::SearchAggregator;
pub use client::{
    BaiduSearchEngine, BingSearchEngine, GoogleSearchEngine, SearchClient, SogouSearchEngine,
    SHARED_HTTP_CLIENT,
};
pub use engine_trait::{SearchEngine, SearchRequest};
pub use error::SearchError;
pub use factory::{create_default_router, SearchEngineFactory, SearchEngineFactoryConfig};
pub use response::{Response, ResponseItem};
pub use router::{
    EngineMetrics, RouterStats, SearchEngineRouter, SearchEngineRouterConfig,
    SmartSearchEngineWrapper,
};
pub use smart::{
    create_baidu_smart_search, create_bing_smart_search, create_google_smart_search,
    create_sogou_smart_search, SmartSearchEngine, SmartSearchEngineConfig,
};
pub use types::{EngineHealth, SearchEngineType};
