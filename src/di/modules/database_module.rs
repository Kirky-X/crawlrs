// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 数据库模块 — 提供 `Arc<DatabasePool>`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::infrastructure::database::dbnexus_connection::DatabasePool;

/// 数据库模块 — 提供 `Arc<DatabasePool>`
///
/// 依赖 `SettingsModule`，使用 `init_database()` 创建连接池。
pub struct DatabaseModule;

impl ModuleMeta for DatabaseModule {
    const NAME: &'static str = "database";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl AsyncAutoBuilder for DatabaseModule {
    type Capability = Arc<DatabasePool>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let pool = crate::bootstrap::infrastructure::init_database(&settings)
                .await
                .map_err(|e| ModuleBuildError::DatabaseInit(e.to_string()))?;
            Ok(pool)
        })
    }
}
