// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
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

pub mod adapter;
pub mod client;
pub mod engine_trait;
pub mod error;
pub mod response;
pub mod types;

pub use adapter::create_domain_adapter;
pub use client::{
    BaiduSearchEngine, BingSearchEngine, GoogleSearchEngine, SearchClient, SogouSearchEngine,
};
pub use engine_trait::{SearchEngine, SearchRequest};
pub use error::SearchError;
pub use response::{Response, ResponseItem};
pub use types::{EngineHealth, SearchEngineType};
