// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 进程级同步 ID 生成器 —— NebulaId 生成能力的同步门面。
//!
//! # 为何存在
//!
//! NebulaId 的生成 API 是 `async`（[`crate::domain::services::id_service::IdService::next_id`]），
//! 但 crawlrs 的 **~1000 处 ID 生成点**中大量位于 `impl Default`、结构体字面量、
//! 同步 mapper、以及测试夹具——这些上下文**无法 `.await`**（Rust 语言限制）。
//! 为在不 async 传染全栈的前提下完成「所有生产 ID 均由 NebulaId 生成」的全量迁移，
//! 这里提供一个**同步、免 await、可作为 `Uuid::new_v4()` 直接替换物**的门面
//! [`generate`]：底层从启动期预取并后台补水的 NebulaId `uuid_v8` ID 池中弹出。
//!
//! # 生命周期
//!
//! - [`init`] 由 `IdModule` 在启动期 `async` 上下文调用一次：以 `batch_generate`
//!   预取 [`SEED_BATCH`] 个 ID 注入池，并 spawn 周期补水任务（低于
//!   [`LOW_WATER`] 时补至 [`TARGET`]）。
//! - [`generate`] 弹出一个 ID。池耗尽或未初始化（如单元/集成测试期）时回退到
//!   进程内全局唯一的 [`Id::test_new`]（仍保证非 nil、唯一，绝不返回错误、绝不
//!   中断调用方）。

use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex as ParkingMutex;

use crate::domain::id::Id;

/// 启动预取批量（一次性注入池的大小）。
const SEED_BATCH: usize = 100_000;
/// 池水位下限：低于此值触发后台补水。
const LOW_WATER: usize = 20_000;
/// 补水目标水位。
const TARGET: usize = 100_000;
/// 后台补水周期（毫秒）。
const REFILL_INTERVAL_MS: u64 = 30_000;

/// 进程级生成器句柄。
struct GlobalIdGen {
    generator: nebulaid::sdk::IdGenerator,
    workspace: String,
    group: String,
    pool: Arc<ParkingMutex<VecDeque<Id>>>,
}

static GLOBAL: OnceLock<Arc<GlobalIdGen>> = OnceLock::new();

impl GlobalIdGen {
    /// 弹出下一个 ID（不含补水逻辑的核心）。
    fn pop_one(&self) -> Option<Id> {
        self.pool.lock().pop_front()
    }

    fn pool_len(&self) -> usize {
        self.pool.lock().len()
    }

    /// 同步补水：仅在**专用补水任务/非 await 热路径**下由 `block_in_place` 包裹调用；
    /// 常规路径依赖周期补水任务，`generate` 本身绝不同步 await（避免嵌套 block_on panic）。
    async fn refill(&self) -> usize {
        let need = TARGET.saturating_sub(self.pool_len());
        if need == 0 {
            return 0;
        }
        match self
            .generator
            .batch_generate(&self.workspace, &self.group, "refill", need)
            .await
        {
            Ok(batch) => {
                let n = batch.ids.len();
                let mut pool = self.pool.lock();
                for id in batch.ids {
                    if pool.len() >= TARGET {
                        break;
                    }
                    pool.push_back(Id::from(id));
                }
                n
            }
            Err(e) => {
                log::error!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "id-refill-failed",
                        &[("error", fluent_bundle::FluentValue::from(e.to_string()))],
                    )
                );
                0
            }
        }
    }
}

/// 由 `IdModule` 在启动期调用：预取 ID 池、注册全局句柄、spawn 周期补水任务。
///
/// 幂等：重复调用以首次为准（`OnceLock`）。
pub async fn init(
    generator: nebulaid::sdk::IdGenerator,
    workspace: String,
    group: String,
) -> anyhow::Result<()> {
    let pool_arc = Arc::new(ParkingMutex::new(VecDeque::new()));

    // 预取种子批量注入池。
    let batch = generator
        .batch_generate(&workspace, &group, "bootstrap", SEED_BATCH)
        .await
        .map_err(|e| anyhow::anyhow!("NebulaId seed batch_generate failed: {e}"))?;
    {
        let mut pool = pool_arc.lock();
        for id in batch.ids {
            pool.push_back(Id::from(id));
        }
    }

    let global = Arc::new(GlobalIdGen {
        generator,
        workspace,
        group,
        pool: pool_arc,
    });

    // 仅首次成功注册；已初始化则直接返回（复用既有池与补水任务）。
    let first = GLOBAL.set(global.clone()).is_ok();
    if first {
        log::info!(
            "{}",
            crate::i18n::tr_log_args(
                "id-global-generator-initialized",
                &[
                    (
                        "workspace",
                        fluent_bundle::FluentValue::from(global.workspace.as_str()),
                    ),
                    (
                        "group",
                        fluent_bundle::FluentValue::from(global.group.as_str())
                    ),
                ],
            )
        );
        // spawn 周期补水任务（任务内 clone Arc 保持存活）。
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(REFILL_INTERVAL_MS)).await;
                if global.pool_len() < LOW_WATER {
                    global.refill().await;
                }
            }
        });
    }
    Ok(())
}

/// 同步弹出一个由 NebulaId 生成的 ID。
///
/// 这是全量迁移中所有 `Uuid::new_v4()` 生成点的**直接替换物**：免 await、免注入、
/// 返回非 nil 唯一 ID。池耗尽或未初始化（测试期）回退到进程内原子唯一的
/// [`Id::test_new`]，绝不失败。
#[inline]
pub fn generate() -> Id {
    match GLOBAL.get() {
        Some(global) => match global.pop_one() {
            Some(id) => {
                // 低水位时按需触发一次后台补水（best-effort，不阻塞调用方）；
                // 仅当当前线程处于 tokio 运行时时才 spawn（避免无运行时线程 panic）。
                if global.pool_len() < LOW_WATER && tokio::runtime::Handle::try_current().is_ok() {
                    let g = global.clone();
                    tokio::spawn(async move {
                        g.refill().await;
                    });
                }
                id
            }
            // 极端耗尽（补水未及）：回退唯一 ID，保证不中断。
            None => Id::test_new(),
        },
        // 未初始化（测试或启动前）：回退唯一 ID。
        None => Id::test_new(),
    }
}

/// 是否已完成启动初始化（供健康检查/测试断言）。
#[inline]
pub fn is_initialized() -> bool {
    GLOBAL.get().is_some()
}
