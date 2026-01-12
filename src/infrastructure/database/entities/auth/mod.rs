// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Auth entities module

pub mod audit_log;
pub mod feature_flag;
pub mod feature_flag_override;
pub mod scope;

pub use audit_log::{
    ActiveModel as AuditLogActiveModel, Entity as AuditLogEntity, Model as AuditLogModel,
};
pub use feature_flag::{
    ActiveModel as FeatureFlagActiveModel, Entity as FeatureFlagEntity, Model as FeatureFlagModel,
};
pub use feature_flag_override::{
    ActiveModel as FeatureFlagOverrideActiveModel, Entity as FeatureFlagOverrideEntity,
    Model as FeatureFlagOverrideModel,
};
pub use scope::{ActiveModel as ScopeActiveModel, Entity as ScopeEntity, Model as ScopeModel};
