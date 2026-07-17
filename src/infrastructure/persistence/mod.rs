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
//! ```ignore
//! Domain Layer (pure models)
//!         ↕
//!    Mappers (conversion)
//!         ↕
//! Infrastructure Layer (database entities)
//! ```

#[cfg(feature = "dbnexus-postgres")]
pub mod mappers;

// Re-export mappers for convenience
#[cfg(feature = "dbnexus-postgres")]
pub use mappers::{
    CrawlMapper, CreditsMapper, CreditsTransactionMapper, TaskMapper, WebhookEventMapper,
    WebhookMapper,
};
