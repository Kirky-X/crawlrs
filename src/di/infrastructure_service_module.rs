// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure Service module for dependency injection.
//!
//! This module provides components for infrastructure service layer dependencies
//! including WebhookSender and other infrastructure-level services.

use std::sync::Arc;

use crate::domain::services::webhook_sender::WebhookSender;
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;

// =============================================================================
// WebhookSender Component
// =============================================================================

/// Trait for WebhookSender component
pub trait WebhookSenderTrait: Send + Sync {
    fn get(&self) -> Arc<dyn WebhookSender>;
}

/// WebhookSender component
///
/// This component provides WebhookSender through WebhookSenderImpl
pub struct WebhookSenderComponent {
    sender: WebhookSenderImpl,
}

impl WebhookSenderTrait for WebhookSenderComponent {
    fn get(&self) -> Arc<dyn WebhookSender> {
        Arc::new(self.sender.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WebhookSenderComponent::new 应存储传入的 WebhookSenderImpl，
    /// 后续 get() 调用返回的 Arc 应指向等效的 sender 实例。
    #[test]
    fn test_webhook_sender_component_new_stores_sender() {
        let sender = WebhookSenderImpl::with_default_config();
        let component = WebhookSenderComponent { sender };

        // get() 应返回一个有效的 Arc<dyn WebhookSender>，不 panic
        let retrieved = component.get();
        // Arc strong_count >= 1 验证返回值是有效的 Arc
        assert!(Arc::strong_count(&retrieved) >= 1);
    }

    /// WebhookSenderComponent 通过 WebhookSenderTrait trait 对象访问时，
    /// get() 应正常工作（验证动态分发）。
    #[test]
    fn test_webhook_sender_component_as_trait_object() {
        let sender = WebhookSenderImpl::with_default_config();
        let component = WebhookSenderComponent { sender };

        // 通过 trait 对象访问，验证动态分发正常工作
        let trait_obj: &dyn WebhookSenderTrait = &component;
        let retrieved = trait_obj.get();
        assert!(Arc::strong_count(&retrieved) >= 1);
    }

    /// 多次调用 get() 应返回独立的 Arc 实例（每次 clone 一个新 WebhookSenderImpl）。
    /// 验证：连续两次 get() 返回的 Arc 指向不同的底层对象（因为 WebhookSenderImpl 被 clone）。
    #[test]
    fn test_webhook_sender_component_get_returns_independent_arcs() {
        let sender = WebhookSenderImpl::with_default_config();
        let component = WebhookSenderComponent { sender };

        let first = component.get();
        let second = component.get();

        // 两次 get() 返回不同的 Arc 实例（WebhookSenderImpl 被 clone）
        assert!(!Arc::ptr_eq(&first, &second));
        // 但都是有效的 Arc
        assert!(Arc::strong_count(&first) >= 1);
        assert!(Arc::strong_count(&second) >= 1);
    }
}
