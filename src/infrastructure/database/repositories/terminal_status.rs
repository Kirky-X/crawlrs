// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! crawl 终态判定 SQL 单一构造器。
//!
//! `update_status_guarded`、`finalize_if_all_terminal`、`finalize_failed_after_enqueue`
//! 与过期对账共用同一谓词/CASE，避免多处逐字复制后漂移。
//! `finalize_failed_after_enqueue` 只用非终态守卫（入队中断不等价于
//! 计数可对账），其余三处共用完整终态谓词。

use crate::domain::models::crawl_model::ACTIVE_CRAWL_STATUSES;
use crate::infrastructure::database::entities::crawl;
use dbnexus::sea_orm::{sea_query::Expr, ColumnTrait, Condition, ExprTrait};

/// 非终态过滤：状态属于仍可迁移的集合（`queued` / `processing`）。
///
/// 用 `IN (...)` 而非 `NOT IN (终态...)`：已知状态枚举只有 5 个取值，
/// 两者等价，但 `IN` 可命中 status 索引，周期对账不必全表扫描。
pub(crate) fn non_terminal_condition() -> Condition {
    Condition::all().add(crawl::Column::Status.is_in(ACTIVE_CRAWL_STATUSES))
}

/// 终态落库谓词：非终态 + total>0 + completed+failed==total。
pub(crate) fn terminal_eligible_condition() -> Condition {
    non_terminal_condition()
        .add(crawl::Column::TotalTasks.gt(0))
        .add(
            Expr::col(crawl::Column::CompletedTasks)
                .add(Expr::col(crawl::Column::FailedTasks))
                .eq(Expr::col(crawl::Column::TotalTasks)),
        )
}

/// 终态 CASE：completed==0 → failed，否则 completed。
pub(crate) fn terminal_status_case() -> Expr {
    Expr::cust("CASE WHEN completed_tasks = 0 THEN 'failed' ELSE 'completed' END")
}
