// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Settings 模块 — 提供 `Arc<Settings>`

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::ModuleBuildError;
use crate::config::settings::Settings;

/// Settings 模块 — 提供 `Arc<Settings>`
///
/// 从 kit 的 config store 读取预先加载的 Settings。
pub struct SettingsModule;

impl ModuleMeta for SettingsModule {
    const NAME: &'static str = "settings";

    fn dependencies() -> &'static [(&'static str, std::any::TypeId)] {
        &[]
    }
}

impl AsyncAutoBuilder for SettingsModule {
    type Capability = Arc<Settings>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            kit.config::<Arc<Settings>>()
                .map_err(|e| ModuleBuildError::SettingsNotConfigured(e.to_string()))
        })
    }
}
