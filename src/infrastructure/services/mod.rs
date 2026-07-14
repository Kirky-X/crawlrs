// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 基础设施服务模块
///
/// 提供基础设施层的服务实现
/// 包括限流服务等核心功能
pub mod config_service;
#[cfg(feature = "rate-limiting")]
pub mod limiteron_service;
pub mod webhook_sender_impl;

/// Webhook 服务公共接口
/// 导出验证函数供接收方使用
pub mod webhook_service {
    pub use crate::domain::services::webhook_service::WebhookServiceImpl;

    /// 验证 webhook 签名
    pub use crate::domain::services::webhook_service::verify_webhook_signature;
}
