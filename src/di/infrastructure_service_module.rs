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
