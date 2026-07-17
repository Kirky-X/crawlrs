// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeResult module -统一导出 ScrapeResult 相关的所有类型
//!
//! 这个模块聚合了 scrape_result_entity 的导出，
//! 以保持向后兼容性。

// Re-export from entity (Model as ScrapeResult, Entity as ScrapeResultEntity)
#[cfg(feature = "dbnexus-postgres")]
pub use super::scrape_result_entity::{Entity as ScrapeResultEntity, Model as ScrapeResult};
