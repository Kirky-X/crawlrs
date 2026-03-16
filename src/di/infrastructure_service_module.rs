// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Infrastructure Service module for Shaku dependency injection.
//!
//! This module provides Shaku components for infrastructure service layer dependencies
//! including WebhookSender and other infrastructure-level services.

use std::sync::Arc;

use shaku::{Component, Interface};

use crate::domain::services::webhook_sender::WebhookSender;
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;

// =============================================================================
// WebhookSender Component
// =============================================================================

/// Trait for WebhookSender component
pub trait WebhookSenderTrait: Interface + Send + Sync {
    fn get(&self) -> Arc<dyn WebhookSender>;
}

/// WebhookSender component for Shaku DI
///
/// This component provides WebhookSender through WebhookSenderImpl
#[derive(Component)]
#[shaku(interface = WebhookSenderTrait)]
pub struct WebhookSenderComponent {
    #[shaku(default = WebhookSenderImpl::with_default_config())]
    sender: WebhookSenderImpl,
}

impl WebhookSenderTrait for WebhookSenderComponent {
    fn get(&self) -> Arc<dyn WebhookSender> {
        Arc::new(self.sender.clone())
    }
}

// Infrastructure service module components - for Shaku DI
