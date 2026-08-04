// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础设施模块 — 提供 `InfrastructureComponents`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::cache_module::CacheModule;
use super::database_module::DatabaseModule;
use super::http_module::HttpModule;
use super::repository_module::RepositoryModule;
use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::bootstrap::infrastructure::InfrastructureComponents;

/// 基础设施模块 — 提供 `InfrastructureComponents`
///
/// 依赖 `DatabaseModule`、`HttpModule`、`CacheModule`、`RepositoryModule`，
/// 聚合所有基础设施组件。
pub struct InfrastructureModule;

impl ModuleMeta for InfrastructureModule {
    const NAME: &'static str = "infrastructure";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 5] = [
            (DatabaseModule::NAME, TypeId::of::<DatabaseModule>()),
            (HttpModule::NAME, TypeId::of::<HttpModule>()),
            (CacheModule::NAME, TypeId::of::<CacheModule>()),
            (RepositoryModule::NAME, TypeId::of::<RepositoryModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

impl AsyncAutoBuilder for InfrastructureModule {
    type Capability = InfrastructureComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let db = kit.require::<DatabaseModule>()?;
            let http_client = kit.require::<HttpModule>()?;
            let cache_components = kit.require::<CacheModule>()?;
            let repositories = kit.require::<RepositoryModule>()?;

            let cache_service = crate::bootstrap::infrastructure::init_cache_service(&settings)
                .await
                .map_err(|e| ModuleBuildError::InfrastructureInit(e.to_string()))?;

            Ok(InfrastructureComponents {
                db,
                oxcache: Some(cache_components.search_cache),
                cache_service,
                http_client,
                repositories,
            })
        })
    }
}
