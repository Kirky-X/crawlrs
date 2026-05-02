// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 领域模型模块
///
/// 该模块定义了系统的核心业务实体，遵循 DDD 原则：
/// - 纯领域模型（*_model.rs）：无 ORM 注解，包含业务逻辑
/// - 领域类型（*_domain.rs）：枚举、错误类型等
/// - ORM 实体已移动到 infrastructure/database/entities/
///
/// 命名规则：
/// - *_model.rs: 纯领域模型（无 ORM 注解）
/// - *_domain.rs: 领域业务逻辑（枚举、错误类型）
// Pure domain models (no ORM annotations)
pub mod crawl_model;
pub mod credits_model;
pub mod task_model;
pub mod webhook_model;

// Domain types (enums, errors)
pub mod task_domain;

// Legacy modules kept for compatibility
pub mod scrape_result;
pub mod scrape_result_entity;
pub mod search_result;

// Re-export pure domain models
pub use crawl_model::{Crawl, CrawlStatus};
pub use credits_model::{Credits, CreditsError, CreditsTransaction, CreditsTransactionType};
pub use task_domain::{DomainError, TaskStatus, TaskType};
pub use task_model::Task;
pub use webhook_model::{Webhook, WebhookError, WebhookEvent, WebhookEventType, WebhookStatus};

// Legacy re-exports for backward compatibility
pub use scrape_result_entity::{Entity as ScrapeResultEntity, Model as ScrapeResult};
pub use search_result::SearchResult;
