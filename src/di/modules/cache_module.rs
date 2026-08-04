// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 缓存模块 — 提供 `CacheComponents`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::settings_module::SettingsModule;
use super::CacheComponents;
use super::ModuleBuildError;
use crate::infrastructure::oxcache::ConcurrencyController;

/// 缓存模块 — 提供 `CacheComponents`
///
/// 依赖 `SettingsModule`，使用 `create_cache()` 创建 oxcache 实例。
pub struct CacheModule;

impl ModuleMeta for CacheModule {
    const NAME: &'static str = "cache";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl AsyncAutoBuilder for CacheModule {
    type Capability = CacheComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;

            let search_cache = crate::infrastructure::oxcache::create_cache(&settings.cache)
                .await
                .map_err(|e| ModuleBuildError::CacheInit(e.to_string()))?;

            let max_permits = std::cmp::max(1, settings.concurrency.default_team_limit as usize);
            let concurrency_controller = Arc::new(ConcurrencyController::new(max_permits));

            Ok(CacheComponents {
                search_cache,
                concurrency_controller,
            })
        })
    }
}
