// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "engine-reqwest")]
pub mod crawl_text_integration;
#[cfg(feature = "dbnexus-postgres")]
pub mod error_helpers;
/// 工具模块
///
/// 提供通用的工具函数和辅助功能
/// 包括文本处理、URL工具、错误处理等功能
pub mod http_client;
pub mod port_sniffer;
#[cfg(feature = "oxcache-cache")]
pub mod regex_cache;
pub mod retry_policy;
#[cfg(all(feature = "engine-reqwest", feature = "oxcache-cache"))]
pub mod robots;
pub mod search_test;
pub mod secret;
#[cfg(feature = "logging")]
pub mod telemetry;
pub mod text_processing;
pub mod url;

// 向后兼容的重新导出 - 已清理，只保留结构体
#[cfg(feature = "engine-reqwest")]
pub use crate::utils::text_processing::{
    CrawlProcessingError, CrawlTextProcessor, ProcessedCrawlContent, ProcessedWebContent,
    TextEncodingError, WebContentError, WebContentProcessor,
};

pub use crate::utils::url::{resolve_url, SafeUrl, UrlError};

pub use crate::utils::secret::{Clearable, SecretString};
