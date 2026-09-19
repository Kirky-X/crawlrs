// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Auth entities module

pub mod audit_log;
pub mod scope;

pub use audit_log::{
    ActiveModel as AuditLogActiveModel, Entity as AuditLogEntity, Model as AuditLogModel,
};
