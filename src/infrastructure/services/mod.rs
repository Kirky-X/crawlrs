// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

/// 基础设施服务模块
///
/// 提供基础设施层的服务实现
/// 包括限流服务等核心功能
pub mod config_service;
/// fluxweir 承载的 LLM carrier（变更 fluxweir-llm-gateway D4：
/// 经 SDK 经嵌入式网关/外部实例发出提取请求，gateway feature 门控）
#[cfg(feature = "gateway")]
pub mod fluxweir_llm_carrier;
/// fluxweir 承载的嵌入/重排 provider（变更 fluxweir-llm-gateway D5：
/// RAG 降级链 secondary，gateway feature 门控）
#[cfg(feature = "gateway")]
pub mod fluxweir_providers;
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
#[cfg(feature = "rag-local")]
pub mod vecboost_provider;
/// webhook 发送器实现
///
/// webhook feature 关闭时不编译此模块。
/// webhook-off 模式下，`init_services` 装配 `NoopWebhookService`（不发送 webhook），
/// 不需要 `WebhookSenderImpl`。
#[cfg(feature = "webhook")]
pub mod webhook_sender_impl;
