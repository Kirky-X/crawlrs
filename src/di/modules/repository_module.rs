// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 仓储模块 — 提供 `Repositories`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::database_module::DatabaseModule;
use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::bootstrap::infrastructure::Repositories;

/// 仓储模块 — 提供 `Repositories`
///
/// 依赖 `DatabaseModule`，创建所有仓储实现实例。
pub struct RepositoryModule;

impl ModuleMeta for RepositoryModule {
    const NAME: &'static str = "repositories";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 2] = [
            (DatabaseModule::NAME, TypeId::of::<DatabaseModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

impl AsyncAutoBuilder for RepositoryModule {
    type Capability = Repositories;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let db = kit.require::<DatabaseModule>()?;
            let repos = crate::bootstrap::infrastructure::init_repositories(db, &settings);
            Ok(repos)
        })
    }
}
