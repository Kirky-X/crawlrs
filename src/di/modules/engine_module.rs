// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 引擎模块 — 提供 `EngineComponents`

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::http_module::HttpModule;
use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::bootstrap::engines::EngineComponents;
use crate::engines::provider::ProxyProvider;
use crate::engines::proxy_pool::ProxyPool;

/// 引擎模块 — 提供 `EngineComponents`
///
/// 依赖 `HttpModule` 和 `SettingsModule`，创建 EngineRouter + EngineClient。
pub struct EngineModule;

impl ModuleMeta for EngineModule {
    const NAME: &'static str = "engines";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 2] = [
            (HttpModule::NAME, TypeId::of::<HttpModule>()),
            (SettingsModule::NAME, TypeId::of::<SettingsModule>()),
        ];
        &DEPS
    }
}

impl AsyncAutoBuilder for EngineModule {
    type Capability = EngineComponents;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;
            let http_client = kit.require::<HttpModule>()?;
            // T056/R-identity-003 + H1/H2 修复: 构造 ProxyProvider 注入 ReqwestEngine
            // - proxy.enabled=true 且 urls 非空 → Some(Arc<dyn ProxyProvider>)
            // - 否则 → None（ReqwestEngine 不使用代理）
            //
            // MEDIUM-2 修复：sticky_ttl / cooldown 从 settings.proxy 注入（原硬编码 60s/30s）
            //
            // 策略：从 settings.proxy.strategy 注入（H1：RoundRobin / Sticky 路由）
            let proxy_provider: Option<Arc<dyn ProxyProvider>> =
                if settings.proxy.enabled && !settings.proxy.urls.is_empty() {
                    Some(Arc::new(ProxyPool::from_urls(
                        settings.proxy.urls.clone(),
                        std::time::Duration::from_secs(settings.proxy.sticky_ttl_seconds),
                        std::time::Duration::from_secs(settings.proxy.cooldown_seconds),
                    )))
                } else {
                    None
                };
            // FlareSolverr 单代理：取 urls.first() 作为 fallback（FlareSolverr 暂未接入 ProxyProvider，
            // T056 范围外）。proxy.enabled=false 时为 None。
            let proxy_url: Option<String> = if settings.proxy.enabled {
                settings.proxy.urls.first().cloned()
            } else {
                None
            };
            let engines = crate::bootstrap::engines::init_engine_components(
                http_client,
                proxy_provider,
                settings.proxy.strategy,
                proxy_url,
                &settings.engines,
                // T061：注入完整 EngineTimeoutSettings（含 default_timeout_seconds + 三个 MRT 字段）
                &settings.timeouts.engines,
            );
            Ok(engines)
        })
    }
}
