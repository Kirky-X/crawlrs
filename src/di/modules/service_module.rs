// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 服务模块 — 提供 `ServicesComponents`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::engine_module::EngineModule;
use super::infrastructure_module::InfrastructureModule;
use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::bootstrap::services::ServicesComponents;

/// 服务模块 — 提供 `ServicesComponents`
///
/// 依赖 `InfrastructureModule`、`EngineModule`、`SettingsModule`，
/// 创建所有应用服务实例。
pub struct ServiceModule;

impl ModuleMeta for ServiceModule {
    const NAME: &'static str = "services";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 3] = [
            (
                InfrastructureModule::NAME,
                TypeId::of::<InfrastructureModule>(),
            ),
            (EngineModule::NAME, TypeId::of::<EngineModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

impl AsyncAutoBuilder for ServiceModule {
    type Capability = ServicesComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let infrastructure = kit.require::<InfrastructureModule>()?;
            let engines = kit.require::<EngineModule>()?;

            let services = crate::bootstrap::services::init_services(
                &infrastructure,
                engines.engine_client.clone(),
                infrastructure.http_client.clone(),
                &settings,
            )
            .await;

            Ok(services)
        })
    }
}
