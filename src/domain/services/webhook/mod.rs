// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Service Module
//!
//! Re-exports webhook service implementations.

#[cfg(feature = "webhook")]
mod management;

#[cfg(feature = "webhook")]
pub use management::WebhookManagementServiceImpl;
