// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! HTTP 客户端模块 — 提供 `Arc<reqwest::Client>`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::settings_module::SettingsModule;
use super::ModuleBuildError;

/// HTTP 客户端模块 — 提供 `Arc<reqwest::Client>`
///
/// 依赖 `SettingsModule`，根据配置创建 reqwest::Client。
pub struct HttpModule;

impl ModuleMeta for HttpModule {
    const NAME: &'static str = "http-client";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl AsyncAutoBuilder for HttpModule {
    type Capability = Arc<reqwest::Client>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let client = crate::bootstrap::infrastructure::init_http_client(&settings, None)
                .map_err(|e| ModuleBuildError::HttpInit(e.to_string()))?;
            Ok(client)
        })
    }
}
