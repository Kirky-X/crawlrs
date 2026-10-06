// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! ID 生成领域服务 —— 封装 NebulaId 嵌入式 SDK 生成器。
//!
//! 这是 crawlrs 全项目 ID 生成点（迁移自 `crate::domain::id::Id::generate()`）的**唯一入口**。
//! 由 DI（[`crate::di::modules::id_module::IdModule`]）在启动期构建一次并
//! 经 [`crate::di::CrawlRsState`] 注入，供各层 `async` 上下文调用
//! [`IdService::next_id`]。
//!
//! # 设计约束（门禁 1 & 3）
//!
//! - NebulaId 生成 API 是 `async`，故 `next_id` 为 `async`；非 `async` 上下文
//!   （`impl Default`、结构体字面量）改为由上游生成后注入，不在此服务内隐藏 await。
//! - `IdGenerator` 为廉价 `Clone` handle（内部 `Arc<AlgorithmRouter>`），
//!   可跨任务共享；`IdService` 同时持有底层 `NebulaIdKit` 以维持其生命周期
//!   （降级后台健康检查任务 / `on_shutdown` 钩子）。
//! - 生成失败**显性上抛**（fail-closed），绝不静默回退到 `crate::domain::id::Id::generate()`——
//!   那会让「已切换 NebulaId」的假设落空。

use std::sync::Arc;

use anyhow::Result;

use crate::domain::id::Id;

/// ID 生成服务。
pub struct IdService {
    /// 底层 NebulaId Kit（持有以维持 router/后台任务生命周期）。
    kit: Arc<nebulaid::sdk::NebulaIdKit>,
    /// ID 生成 handle（自 `kit` 抽取，独立持有 `Arc<AlgorithmRouter>`）。
    generator: nebulaid::sdk::IdGenerator,
    /// 生成时传入的默认 workspace 标识。
    workspace: String,
    /// 生成时传入的默认 group 标识。
    group: String,
}

impl IdService {
    /// 由已构建的 Kit 与 handle 组装服务（供 `IdModule` 调用）。
    pub fn new(
        kit: Arc<nebulaid::sdk::NebulaIdKit>,
        generator: nebulaid::sdk::IdGenerator,
        workspace: String,
        group: String,
    ) -> Self {
        Self {
            kit,
            generator,
            workspace,
            group,
        }
    }

    /// 生成下一个 ID。
    ///
    /// `biz_tag` 为业务域标签（如 `"task"`、`"crawl"`、`"audit"`），透传给
    /// NebulaId 生成器。默认算法 `uuid_v8` 为纯算法，零数据库/零号段依赖。
    pub async fn next_id(&self, biz_tag: &str) -> Result<Id> {
        let nebula_id = self
            .generator
            .generate(&self.workspace, &self.group, biz_tag)
            .await
            .map_err(|e| anyhow::anyhow!("NebulaId generate failed (biz_tag={biz_tag}): {e}"))?;
        Ok(Id::from(nebula_id))
    }

    /// 批量生成 ID（供高吞吐路径预取，降低逐次 await 往返开销）。
    pub async fn next_ids(&self, biz_tag: &str, size: usize) -> Result<Vec<Id>> {
        let batch = self
            .generator
            .batch_generate(&self.workspace, &self.group, biz_tag, size)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "NebulaId batch_generate failed (biz_tag={biz_tag}, size={size}): {e}"
                )
            })?;
        Ok(batch.ids.into_iter().map(Id::from).collect())
    }

    /// 各模块健康摘要（透传 NebulaId Kit，供 `/ready` 等出口聚合）。
    pub fn health_summary(&self) -> serde_json::Value {
        self.kit.health_summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端验证：用纯算法 `uuid_v8` 构建真实 Kit，`next_id` 产出可用的 `Id`，
    /// 且可无损转回 `crate::domain::id::Id`（默认迁移路径契约）。
    #[tokio::test]
    async fn test_next_id_generates_uuid_shaped_id() {
        use nebulaid::sdk::NebulaIdKitBuilder;

        let mut config = nebulaid::core::Config::default();
        config.algorithm.default = "uuid_v8".to_string();
        // etcd feature 未启用 → SDK 走本地锁；清空 endpoints 与默认行为一致。
        config.etcd.endpoints = Vec::new();

        let kit = Arc::new(
            NebulaIdKitBuilder::new(config)
                .build()
                .await
                .expect("uuid_v8 纯算法 Kit build 必须成功"),
        );
        let generator = kit.id_generator().expect("id_generator 必须可用");
        let service = IdService::new(kit, generator, "crawlrs-test".to_string(), "ut".to_string());

        let id = service.next_id("task").await.expect("next_id 必须成功");
        assert!(!id.is_nil(), "生成的 ID 不得为 nil");
        // 默认 uuid_v8 → 渲染为 36 位 UUID 字符串（API/DB 契约保持）。
        let rendered = id.to_string();
        assert_eq!(rendered.matches('-').count(), 4, "实际渲染: {rendered}");
        // 可无损转 crate::domain::id::Id 并回到同一原值。
        let uuid = id.to_uuid_v8();
        assert_eq!(Id::from_uuid_v8(uuid), id);

        // 批量生成非空且互不相同。
        let ids = service.next_ids("task", 5).await.expect("批量必须成功");
        assert_eq!(ids.len(), 5);
        let uniq: std::collections::HashSet<_> = ids.iter().map(|i| i.as_u128()).collect();
        assert_eq!(uniq.len(), 5, "同批 ID 不得重复");
    }
}
