// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 领域服务模块
//!
//! 该模块包含系统的核心业务逻辑服务，这些服务封装了复杂的
//! 业务规则和领域逻辑，协调多个领域对象来完成业务操作。
//!
//! 包含的服务：
//! - 审计服务（audit_service）：处理认证和授权决策的审计日志
//! - 提取服务（extraction_service）：处理内容提取和数据解析逻辑
//! - 提取工具（extraction_utils）：消除提取逻辑重复的共享工具函数
//! - 地理位置服务（geo_location）：提供IP地址地理位置查询的抽象接口
//! - LLM服务（llm_service）：集成大语言模型进行智能处理
//! - 重试处理器（retry_handler）：处理任务失败的重试逻辑
//! - 搜索服务（search_service）：处理内容搜索和索引逻辑
//! - 团队服务（team_service）：处理团队地理限制验证逻辑
//! - 限流服务（rate_limiting_service）：处理请求限流逻辑
//! - Webhook服务（webhook_service）：处理 Webhook 通知逻辑
//!
//! 领域服务与应用程序服务的区别在于：领域服务包含纯粹的业务逻辑，
//! 而应用程序服务负责协调和编排，可能包含技术实现细节。
//!
//! ## Stage 3 重构（R-auth-engine-003）
//!
//! 已删除 `auth_scope_service` 模块——API Key 权限范围管理已迁移到 garrison
//! RBAC，crawlrs 侧不再持有 `AuthScopeService`。Scope 由 `auth_middleware_inner`
//! 通过 `bridge_to_auth_state` 从 garrison `perms` 动态构造。

pub mod audit_log_builder;
pub mod audit_service;
/// 正文提取模块（content-processing R2/R3，T044-T049/R-content-002、R-content-003）
///
/// 特性门控（R-content-003）：
/// - `trafilatura`：启用 trafilatura 实现（主路径）
/// - `dom-smoothie`：启用 dom_smoothie 实现（性能回退）
/// - `extractors`：聚合两者
/// - 三特性均关闭时 Facade 退化为 CssRule 兜底（编译通过，功能可用）
pub mod content_extractor;
pub mod extraction_service;
pub mod extraction_utils;
pub mod geo_location;
pub mod llm;
pub mod llm_provider_strategy;
/// Markdown 转换服务（content-processing R1，T040/R-content-001）
///
/// gated `markdown` 特性（依赖 `htmd`）。`markdown` 已加入 `standard`/`full`。
#[cfg(feature = "content")]
pub mod markdown_service;
/// Noop Webhook 服务实现（webhook feature 关闭时使用）
///
/// R-wh-002 / T025：webhook feature 关闭时编译此模块，
/// 提供 `NoopWebhookService` 替代 `WebhookServiceImpl`，
/// 所有方法返回 `Ok(())`。
#[cfg(not(feature = "webhook"))]
pub mod noop_webhook_service;
/// RAG 增强提取策略（T072-T076）
///
/// DOM 语义分块 + 向量嵌入 + 检索增强 LLM 提取。
pub mod rag_strategy;
pub mod rate_limiting_service;
pub mod relevance_scorer;
pub mod retry_handler;
pub mod search_service;
pub mod team_semaphore;
pub mod team_service;
pub mod webhook_event_builder;
/// R-wh-001 / T026：webhook feature 关闭时不编译此模块
/// （`WebhookSender` trait 只在 `WebhookServiceImpl` 中使用，后者已被门控）
#[cfg(feature = "webhook")]
pub mod webhook_sender;
pub mod webhook_service;
