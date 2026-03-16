// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Mapper module - converts between domain models and database entities
//!
//! This module provides bidirectional conversion between:
//! - Pure domain models (in domain/models/)
//! - Database entities (in infrastructure/database/entities/)

pub mod crawl_mapper;
pub mod credits_mapper;
pub mod task_mapper;
pub mod webhook_mapper;

// Re-export mappers
pub use crawl_mapper::CrawlMapper;
pub use credits_mapper::{CreditsMapper, CreditsTransactionMapper};
pub use task_mapper::TaskMapper;
pub use webhook_mapper::{WebhookEventMapper, WebhookMapper};
