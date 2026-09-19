// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Persistence module - handles data persistence layer
//!
//! This module contains:
//! - Mappers: Convert between domain models and database entities
//!
//! Architecture:
//! ```ignore
//! Domain Layer (pure models)
//!         ↕
//!    Mappers (conversion)
//!         ↕
//! Infrastructure Layer (database entities)
//! ```

pub mod mappers;

// Re-export mappers for convenience
pub use mappers::{
    CrawlMapper, CreditsMapper, CreditsTransactionMapper, TaskMapper, WebhookEventMapper,
    WebhookMapper,
};
