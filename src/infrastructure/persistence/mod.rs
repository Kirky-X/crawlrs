// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Persistence module - handles data persistence layer
//!
//! This module contains:
//! - Mappers: Convert between domain models and database entities
//!
//! Architecture:
//! ```
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
