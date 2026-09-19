// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Webhook Service Module
//!
//! Re-exports webhook service implementations.

#[cfg(feature = "webhook")]
mod management;

#[cfg(feature = "webhook")]
pub use management::WebhookManagementServiceImpl;
