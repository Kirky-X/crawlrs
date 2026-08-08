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
//!     .bing()
//!     .limit(10)
//!     .execute()
//!     .await?;
//! ```

pub mod client;
pub mod dedup;
pub mod engine_trait;
pub mod error;
pub mod response;
pub mod rrf;
pub mod smart;
pub mod types;

pub use dedup::ResultDeduplicator;
pub use engine_trait::{SearchEngine, SearchRequest};
pub use error::SearchError;
pub use response::{Response, ResponseItem};
pub use rrf::RRFFuser;
pub use types::{EngineHealth, SearchEngineType};
