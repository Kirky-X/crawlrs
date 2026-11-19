// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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
            // 出站隔离（T004/T006）：
            // - 租户代理注册表：从 `[proxy.pools]` + `[outbound_isolation]` 构建，
            //   空配置退化为只含 shared 池（行为与原全局单池一致）；
            //   引擎层注入 shared 池（`select(None)`），SessionManager 持注册表
            //   按租户身份选池。
            // - 租户浏览器门：仅当配置了任何 `max_concurrent_browser_contexts`
            //   时注册全局单例，浏览器池 acquire 先过租户配额再竞争全局信号量。
            let tenant_registry = std::sync::Arc::new(
                crate::engines::tenant_outbound::TenantProxyRegistry::from_settings(&settings),
            );
            let browser_gate = std::sync::Arc::new(
                crate::engines::tenant_outbound::TenantBrowserGate::from_settings(&settings),
            );
            if !browser_gate.is_empty() {
                crate::engines::tenant_outbound::set_global_browser_gate(browser_gate);
            }
            // 构造 ProxyProvider 注入 ReqwestEngine
            // - proxy.enabled=true 且 urls 非空 → Some(Arc<dyn ProxyProvider>)
            // - 否则 → None（ReqwestEngine 不使用代理）
            //
            // sticky_ttl / cooldown 从 settings.proxy 注入（原硬编码 60s/30s）
            //
            // 策略：从 settings.proxy.strategy 注入（H1：RoundRobin / Sticky 路由）
            //
            // T004：提供者来自注册表的 shared 池（select(None)），取代裸
            // ProxyPool::from_urls 全局池——租户专用池经 Session 身份路由。
            let proxy_provider: Option<Arc<dyn ProxyProvider>> =
                if settings.proxy.enabled && !settings.proxy.urls.is_empty() {
                    Some(tenant_registry.select(None))
                } else {
                    None
                };
            // FlareSolverr 单代理：取 urls.first 作为 fallback（FlareSolverr 暂未接入 ProxyProvider，
            // 范围外）。proxy.enabled=false 时为 None。
            let proxy_url: Option<String> = if settings.proxy.enabled {
                settings.proxy.urls.first().cloned()
            } else {
                None
            };
            // vision 适配器经 fluxweir 网关承载（engine-mllm 显式依赖 gateway，
            // 无单独编译态）；[fluxweir].enabled=false 时为 None，engine-mllm
            // 启用会在引擎装配期 fail-fast
            #[cfg(feature = "gateway")]
            let vision_adapter: Option<
                std::sync::Arc<
                    dyn crate::domain::services::llm::vision_adapter::VisionAdapterTrait,
                >,
            > = {
                use crate::domain::services::llm::vision_adapter::FluxweirVisionAdapter;
                crate::infrastructure::services::fluxweir_providers::build_client(settings.as_ref())
                    .map(|client| {
                        std::sync::Arc::new(FluxweirVisionAdapter::new(Some(client))) as _
                    })
            };
            #[cfg(not(feature = "gateway"))]
            let vision_adapter: Option<
                std::sync::Arc<
                    dyn crate::domain::services::llm::vision_adapter::VisionAdapterTrait,
                >,
            > = None;
            let engines = crate::bootstrap::engines::init_engine_components(
                http_client.clone(),
                proxy_provider,
                settings.proxy.strategy,
                proxy_url,
                &settings.engines,
                // 注入完整 EngineTimeoutSettings（含 default_timeout_seconds + 三个 MRT 字段）
                &settings.timeouts.engines,
                settings.engines.max_response_body_bytes as usize,
                // F1/F5：[politeness] 配置注入 per-host 纪律（AutoThrottle/退避/抖动）
                &settings.politeness,
                // F2：[session] 配置注入 Session 身份三件套（代理 + UA/TLS 档
                // + Cookie 绑定与记分退休；enabled=false 走现状盲轮换）；
                // T004：同时注入租户代理注册表（身份按租户选池）
                &settings.session,
                Some(tenant_registry),
                // F4：[observability] 配置注入失败现场快照器（目录空串 = 零成本）
                &settings.observability,
                vision_adapter,
            );
            Ok(engines)
        })
    }
}
