// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 基础设施服务模块
///
/// 提供基础设施层的服务实现
/// 包括限流服务等核心功能
pub mod config_service;
/// 通用 HTTP rerank provider（无条件编译：仅依赖 reqwest，覆盖
/// Cohere v2 / Jina / TEI / 自建 /rerank 端点）
pub mod http_rerank_provider;
/// limiteron 限流服务实现
///
/// rate-limit feature 关闭时不编译此模块。
/// rate-limit-off 模式下，`init_rate_limiting_service` 装配
/// `NoopRateLimitingService` 替代，不需要 limiteron 依赖。
#[cfg(feature = "rate-limit")]
pub mod limiteron_service;
/// Noop 限流服务实现（rate-limit feature 关闭时使用）
///
/// rate-limit feature 关闭时编译此模块，
/// 提供 `NoopRateLimitingService` 替代 `LimiteronService`，
/// 所有方法返回放行/成功。
#[cfg(not(feature = "rate-limit"))]
pub mod noop_rate_limiting_service;
#[cfg(feature = "rag-remote")]
pub mod rig_embedding_provider;
/// RAG 嵌入/重排 provider 实现
///
/// - `vecboost_provider`：vecboost 进程内本地推理，`rag-local` feature 门控
/// - `rig_embedding_provider`：rig 远端嵌入（OpenAI 兼容），`rag-remote` feature 门控
#[cfg(feature = "rag-local")]
pub mod vecboost_provider;
/// webhook 发送器实现
///
/// webhook feature 关闭时不编译此模块。
/// webhook-off 模式下，`init_services` 装配 `NoopWebhookService`（不发送 webhook），
/// 不需要 `WebhookSenderImpl`。
#[cfg(feature = "webhook")]
pub mod webhook_sender_impl;
