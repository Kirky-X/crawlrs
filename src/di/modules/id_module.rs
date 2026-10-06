// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! ID 生成模块 —— 提供 `Arc<IdService>`（NebulaId 嵌入式 SDK 装配）。
//!
//! 依赖 `SettingsModule`：从 `settings.id` 读取算法/workspace/group，构建一次
//! NebulaId `NebulaIdKit`（默认 `uuid_v8` 纯算法，零 DB/零号段），抽取
//! `IdGenerator` handle 并封装为 [`IdService`]。API 与 worker 两条 personality
//! 共用同一装配点（`src/main.rs`），故本模块注册一次即覆盖两种模式。

use std::any::TypeId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nebulaid::sdk::NebulaIdKitBuilder;
use trait_kit::core::{AsyncAutoBuilder, ModuleMeta};
use trait_kit::kit::AsyncKit;

use super::settings_module::SettingsModule;
use super::ModuleBuildError;
use crate::domain::services::id_service::IdService;

/// ID 生成模块 —— 能力为 `Arc<IdService>`。
pub struct IdModule;

impl ModuleMeta for IdModule {
    const NAME: &'static str = "id-generation";

    fn dependencies() -> &'static [(&'static str, TypeId)] {
        static DEPS: [(&str, TypeId); 1] = [(SettingsModule::NAME, TypeId::of::<SettingsModule>())];
        &DEPS
    }
}

impl AsyncAutoBuilder for IdModule {
    type Capability = Arc<IdService>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;

            // 从 crawlrs 配置装配 NebulaId Config。默认算法 uuid_v8 为纯算法
            // （零 DB/零号段）；清空 etcd endpoints 确保走进程内本地锁，无外部依赖。
            let mut config = nebulaid::core::Config::default();
            config.algorithm.default = settings.id.algorithm.clone();
            config.etcd.endpoints = Vec::new();

            let nebula_kit = NebulaIdKitBuilder::new(config)
                .build()
                .await
                .map_err(|e| ModuleBuildError::IdInit(e.to_string()))?;
            let generator = nebula_kit
                .id_generator()
                .map_err(|e| ModuleBuildError::IdInit(e.to_string()))?;
            let nebula_kit = Arc::new(nebula_kit);

            // 初始化进程级同步生成器池（供 `Id::generate()` 在各同步/字面量/
            // mapper 上下文直接弹出 NebulaId 生成的 ID）。
            crate::domain::id::generator::init(
                generator.clone(),
                settings.id.workspace.clone(),
                settings.id.group.clone(),
            )
            .await
            .map_err(|e| ModuleBuildError::IdInit(e.to_string()))?;

            let service = Arc::new(IdService::new(
                nebula_kit,
                generator,
                settings.id.workspace.clone(),
                settings.id.group.clone(),
            ));
            log::info!(
                "IdService initialized (algorithm={}, workspace={}, group={})",
                settings.id.algorithm,
                settings.id.workspace,
                settings.id.group,
            );
            Ok(service)
        })
    }
}
