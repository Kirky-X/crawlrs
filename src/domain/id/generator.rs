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
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// 补水 in-flight 闸：周期任务与低水位触发可能并发，只允许一个 batch_generate。
    refilling: AtomicBool,
}

static GLOBAL: OnceLock<Arc<GlobalIdGen>> = OnceLock::new();

/// 尝试取得补水资格；返回 `false` 表示已有补水在途。
fn try_begin_refill(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// 补水结束后释放 in-flight 闸（panic 展开时同样释放）。
struct RefillGuard<'a>(&'a AtomicBool);

impl Drop for RefillGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 把一批 NebulaId 注入池，直到达到 `target`；返回**实际入池**数量
/// （因池满而 break 丢弃的 id 不计）。
fn drain_batch_into_pool(
    pool: &mut VecDeque<Id>,
    ids: Vec<nebulaid::core::Id>,
    target: usize,
) -> usize {
    let mut inserted = 0;
    for id in ids {
        if pool.len() >= target {
            break;
        }
        pool.push_back(Id::from(id));
        inserted += 1;
    }
    inserted
}

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
    ///
    /// 并发调用由 in-flight 闸串行化：已有补水在途时直接返回 0，避免各自按旧水位
    /// 申请最多 `TARGET` 个 ID 再丢弃的放大。
    async fn refill(&self) -> usize {
        if !try_begin_refill(&self.refilling) {
            log::debug!("id pool refill already in flight, skip");
            return 0;
        }
        let _guard = RefillGuard(&self.refilling);
        self.refill_locked().await
    }

    async fn refill_locked(&self) -> usize {
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
                let mut pool = self.pool.lock();
                drain_batch_into_pool(&mut pool, batch.ids, TARGET)
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
        refilling: AtomicBool::new(false),
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
        // 生命周期：fire-and-forget——不保存 JoinHandle、无 shutdown 信号；tokio runtime
        // drop 时任务随之终止。属有意设计（不为单一后台补水引入 CancellationToken）。
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(REFILL_INTERVAL_MS)).await;
                if global.pool_len() < LOW_WATER {
                    let inserted = global.refill().await;
                    log::debug!("id pool refill inserted {inserted} ids");
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
                        let inserted = g.refill().await;
                        log::debug!("id pool refill inserted {inserted} ids");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn nebula_batch(size: usize) -> Vec<nebulaid::core::Id> {
        (0..size).map(|_| Id::test_new().as_nebulaid()).collect()
    }

    /// 池满时 refill 返回值必须是实际入池数，而非批量大小
    #[test]
    fn refill_counts_only_inserted_ids() {
        let mut pool = VecDeque::new();
        for _ in 0..TARGET - 1 {
            pool.push_back(Id::test_new());
        }
        let inserted = drain_batch_into_pool(&mut pool, nebula_batch(64), TARGET);
        assert_eq!(inserted, 1, "only one free slot must be reported");
        assert_eq!(pool.len(), TARGET);
    }

    /// 池有空位时全部入池，返回值等于批量大小
    #[test]
    fn refill_counts_all_ids_when_pool_has_room() {
        let mut pool = VecDeque::new();
        let inserted = drain_batch_into_pool(&mut pool, nebula_batch(8), TARGET);
        assert_eq!(inserted, 8);
        assert_eq!(pool.len(), 8);
    }

    /// in-flight 闸：同一时刻只允许一个补水，释放后可再次取得
    #[test]
    fn refill_gate_allows_only_one_in_flight() {
        let flag = AtomicBool::new(false);
        assert!(try_begin_refill(&flag), "first caller must acquire");
        assert!(!try_begin_refill(&flag), "second caller must be rejected");
        flag.store(false, Ordering::Release);
        assert!(try_begin_refill(&flag), "released gate must be acquirable");
    }

    /// 闸在 drop 时释放（含 panic 展开路径）
    #[test]
    fn refill_guard_releases_on_drop() {
        let flag = AtomicBool::new(false);
        assert!(try_begin_refill(&flag));
        {
            let _guard = RefillGuard(&flag);
        }
        assert!(try_begin_refill(&flag), "guard drop must release the gate");
    }
}
