// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Auth entities module

pub mod audit_log;
pub mod scope;

pub use audit_log::{
    ActiveModel as AuditLogActiveModel, Entity as AuditLogEntity, Model as AuditLogModel,
};
// T028：`scope::Model` 已弃用（garrison RBAC 接管），re-export 加 `#[allow(deprecated)]`
// 供历史数据迁移/查询保留使用，待全量重签完成后移除。
#[allow(deprecated)]
pub use scope::{ActiveModel as ScopeActiveModel, Entity as ScopeEntity, Model as ScopeModel};
