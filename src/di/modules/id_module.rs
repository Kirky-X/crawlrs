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

use nebulaid::core::algorithm::algorithm_factories;
use nebulaid::core::types::AlgorithmType;
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

/// 校验配置的 ID 算法：必须可解析为已知算法，且已编译进本构建。
///
/// NebulaId 路由对未编译算法静默跳过（`router::initialize` 对缺失工厂
/// `continue`），配了 `snowflake`/`segment` 会悄悄落回 uuid_v8——在此显性
/// 拒绝，让配置错误在启动期暴露。本构建的 nebulaid 仅启用 `sdk,uuid-v8`，
/// 故只有 `uuid_v8`（含 `uuid_v4`/`uuid_v7` 别名）可用。
fn resolve_algorithm(name: &str) -> Result<AlgorithmType, String> {
    let algorithm = name.parse::<AlgorithmType>().map_err(|e| {
        format!(
            "id.algorithm = {name:?} 不是已知算法（可选 uuid_v8；uuid_v4/uuid_v7 别名归入 uuid_v8）：{e}"
        )
    })?;
    if !algorithm_factories().contains_key(&algorithm) {
        return Err(format!(
            "id.algorithm = {name:?} 未编译进本构建（nebulaid 仅启用 sdk,uuid-v8 特性）；\
             请改用 uuid_v8，或在 Cargo.toml 启用对应 nebulaid 特性"
        ));
    }
    Ok(algorithm)
}

impl AsyncAutoBuilder for IdModule {
    type Capability = Arc<IdService>;
    type Error = ModuleBuildError;

    fn build<'a>(
        kit: &'a AsyncKit,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Capability, Self::Error>> + Send + 'a>> {
        Box::pin(async move {
            let settings = kit.require::<SettingsModule>()?;

            // 启动期校验：算法名可解析且已编译进本构建。NebulaId 路由对未编译
            // 算法静默跳过（配了 snowflake/segment 会悄悄落回 uuid_v8），在此
            // 显性拒绝（见 resolve_algorithm）。
            resolve_algorithm(&settings.id.algorithm).map_err(ModuleBuildError::IdInit)?;

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
                "{}",
                crate::i18n::tr_log_args(
                    "id-service-initialized",
                    &[
                        (
                            "algorithm",
                            fluent_bundle::FluentValue::from(settings.id.algorithm.to_string()),
                        ),
                        (
                            "workspace",
                            fluent_bundle::FluentValue::from(settings.id.workspace.as_str()),
                        ),
                        (
                            "group",
                            fluent_bundle::FluentValue::from(settings.id.group.as_str()),
                        ),
                    ],
                )
            );
            Ok(service)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未知算法名与未编译算法显性拒绝；uuid_v8 可用。
    /// snowflake/uuid_v4 断言依赖本构建的 nebulaid 仅启用 uuid-v8 特性，
    /// 且 nebulaid rc.3 起 uuid_v4/uuid_v7 为真算法而非 v8 别名（破坏性变更）。
    #[test]
    fn resolve_algorithm_rejects_unknown_and_uncompiled() {
        assert!(resolve_algorithm("md5").is_err());
        assert!(resolve_algorithm("snowflake").is_err());
        assert_eq!(resolve_algorithm("uuid_v8").unwrap(), AlgorithmType::UuidV8);
        assert!(
            resolve_algorithm("uuid_v4").is_err(),
            "rc.3 起 uuid_v4 为真算法（非 v8 别名），未编译时必须显性拒绝"
        );
    }
}
