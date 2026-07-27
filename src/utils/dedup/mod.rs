// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 分层去重器（design.md §9，T050-T053/R-frontier-001）
//!
//! 统一 UrlNormalizer + UrlInterner（Bloom + HashSet）的对外接口，
//! 供 `scrape_worker::extract_and_queue_links` 接入：
//!
//! ```text
//! candidate URL
//!      │
//!      ▼
//! normalizer.normalize(url)
//!      │
//!      ▼
//! normalizer.permutations(normalized)        ← 生成等价变体
//!      │
//!      ▼
//! deduplicator.check(variants)               ← L1+L2 双层预筛
//!      │
//!      ├─ DefinitelyNew ──► 直接入队 + insert（或用 check_and_insert 原子化）
//!      │
//!      └─ MaybeExisting ──► find_existing_urls DB 精确校验
//! ```
//!
//! ## 设计原则
//!
//! - **DB 保权威**：Bloom 阳性不直接判定为"已存在"，必须回落 DB 校验
//! - **错误显性化**（规则 12）：去重错误显性返回 [`DedupError`]，不静默跳过
//! - **简洁优先**（规则 5）：仅暴露 [`Deduplicator::check`] 与 [`Deduplicator::insert`]，
//!   内部组合 UrlNormalizer + UrlInterner
//! - **TOCTOU 防御**：[`Deduplicator::check_and_insert`] 原子化 check + insert，
//!   避免多 worker 并发时重复入队
//! - **接口隔离**（规则 10）：`mod.rs` 只放 trait/pub 结构体/re-export，
//!   实现见 [`deduplicator`]、[`bloom`]、[`interner`] 子模块

pub mod bloom;
pub mod deduplicator;
pub mod interner;

pub use bloom::MmapBloom;
pub use deduplicator::Deduplicator;
pub use interner::UrlInterner;

use crate::utils::url::UrlError;

/// 去重检查结果
///
/// 严格区分"绝对新"与"可能已存在"两种语义：
/// - `DefinitelyNew`：Bloom 全部阴性 → 100% 未爬过（无假阴性保证）
/// - `MaybeExisting`：Bloom 至少一个阳性 → 可能已爬过，需 DB 精确校验
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupResult {
    /// 绝对新 URL（Bloom 全部阴性），可直接入队
    DefinitelyNew {
        /// 归一化后的标准形式（用于后续 insert）
        normalized: String,
    },
    /// 可能已存在（Bloom 至少一个阳性），需 DB 精确校验
    MaybeExisting {
        /// 归一化后的标准形式
        normalized: String,
        /// 所有等价变体（含 normalized，用于 DB 批量查询）
        variants: Vec<String>,
    },
}

/// 去重错误（规则 12：显性化）
#[derive(Debug, thiserror::Error)]
pub enum DedupError {
    /// URL 归一化失败
    #[error("URL normalize failed: {0}")]
    Normalize(#[from] UrlError),
}
