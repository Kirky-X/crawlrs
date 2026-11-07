// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 积分流水线：scrape / crawl / extract 的扣费与失败退款统一实现。
//!
//! 三个入口的同构部分集中在此：`check_and_deduct_quota` → 失败映射 402 →
//! 入队/创建失败时 `refund_quota` 补偿。各入口的实质差异（金额、交易类型、
//! 描述文案、退款引用、失败日志）由 [`CreditCharge`] 参数化，日志与错误响应
//! 逐字保留既有行为。

use axum::response::Response;
use log::error;

use crate::common::constants::crawl_task::{CRAWL_TASK_CREDITS_COST, EXTRACT_TASK_CREDITS_COST};
use crate::domain::id::Id;
use crate::domain::models::CreditsTransactionType;
use crate::domain::services::rate_limiting_service::{RateLimitingError, RateLimitingService};
use crate::presentation::handlers::response_builder::errors;

/// 一次扣费/退款的能力与计费目标。
///
/// 变体同时绑定能力与目标，避免「能力与目标不匹配」的非法组合。
#[derive(Debug, Clone, Copy)]
pub(super) enum CreditCharge<'a> {
    /// scrape：按目标 URL 计费，固定 1 credit（既有行为，不随常量变化）。
    Scrape { url: &'a str },
    /// crawl：按目标 URL 计费，金额为 [`CRAWL_TASK_CREDITS_COST`]。
    Crawl { url: &'a str },
    /// extract：按 URL 数量计费，金额为 [`EXTRACT_TASK_CREDITS_COST`]。
    Extract { url_count: usize },
}

impl CreditCharge<'_> {
    /// 本次扣费金额。
    fn amount(self) -> i64 {
        match self {
            Self::Scrape { .. } => 1,
            Self::Crawl { .. } => CRAWL_TASK_CREDITS_COST,
            Self::Extract { .. } => EXTRACT_TASK_CREDITS_COST,
        }
    }

    /// 扣费交易类型。
    fn transaction_type(self) -> CreditsTransactionType {
        match self {
            Self::Scrape { .. } => CreditsTransactionType::Scrape,
            Self::Crawl { .. } => CreditsTransactionType::Crawl,
            Self::Extract { .. } => CreditsTransactionType::Extract,
        }
    }

    /// 扣费流水描述（逐字保留既有文案）。
    fn deduct_description(self) -> String {
        match self {
            Self::Scrape { url } => format!("Scrape URL: {}", url),
            Self::Crawl { url } => format!("Crawl URL: {}", url),
            Self::Extract { url_count } => format!("Extract {} URL(s)", url_count),
        }
    }

    /// 退款流水描述（逐字保留既有文案）。
    fn refund_description(self) -> String {
        match self {
            Self::Scrape { url } => format!("Refund: enqueue failed for {}", url),
            Self::Crawl { url } => format!("Refund: crawl creation failed for {}", url),
            Self::Extract { url_count } => {
                format!("Refund: extract enqueue failed for {} URL(s)", url_count)
            }
        }
    }

    /// 扣费失败日志（crawl 既有路径不打印，保持不打印）。
    fn log_deduct_failure(self, team_id: Id, e: &RateLimitingError) {
        if !matches!(self, Self::Crawl { .. }) {
            error!("Quota check failed for team {}: {}", team_id, e);
        }
    }

    /// 退款失败日志（三个入口文案逐字保留）。
    fn log_refund_failure(self, team_id: Id, amount: i64, e: &RateLimitingError) {
        match self {
            Self::Scrape { .. } => error!(
                "CRITICAL: Failed to refund 1 credit for team {} after enqueue failure: {}",
                team_id, e
            ),
            Self::Crawl { .. } => error!(
                "CRITICAL: Failed to refund {} credits for team {} after crawl creation failure: {}",
                amount, team_id, e
            ),
            Self::Extract { .. } => error!(
                "CRITICAL: Failed to refund extract credits for team {}: {}",
                team_id, e
            ),
        }
    }
}

/// 积分配额流水：绑定团队与限流服务，提供扣费与补偿退款。
pub(super) struct CreditFlow<'a> {
    service: &'a dyn RateLimitingService,
    team_id: Id,
}

impl<'a> CreditFlow<'a> {
    /// 绑定团队与限流服务。
    pub(super) fn new(service: &'a dyn RateLimitingService, team_id: Id) -> Self {
        Self { service, team_id }
    }

    /// 扣费：成功返回 `None`；不足/失败返回 402 响应。
    pub(super) async fn deduct(&self, charge: CreditCharge<'_>) -> Option<Response> {
        if let Err(e) = self
            .service
            .check_and_deduct_quota(
                self.team_id,
                charge.amount(),
                charge.transaction_type(),
                charge.deduct_description(),
                None,
            )
            .await
        {
            charge.log_deduct_failure(self.team_id, &e);
            return Some(errors::payment_required(e.to_string()));
        }
        None
    }

    /// 补偿退款：失败仅记录 CRITICAL 日志，不阻断原始错误路径。
    pub(super) async fn refund(&self, charge: CreditCharge<'_>, reference_id: Option<Id>) {
        if let Err(refund_err) = self
            .service
            .refund_quota(
                self.team_id,
                charge.amount(),
                charge.refund_description(),
                reference_id,
            )
            .await
        {
            charge.log_refund_failure(self.team_id, charge.amount(), &refund_err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// scrape 既有行为固定扣 1 credit（不是 SCRAPE_TASK_CREDITS_COST=5）。
    #[test]
    fn scrape_amount_stays_one_credit() {
        assert_eq!(CreditCharge::Scrape { url: "u" }.amount(), 1);
        assert_eq!(CreditCharge::Crawl { url: "u" }.amount(), 10);
        assert_eq!(CreditCharge::Extract { url_count: 3 }.amount(), 8);
    }

    /// 扣费/退款描述与三个入口既有文案逐字一致。
    #[test]
    fn descriptions_preserve_existing_wording() {
        assert_eq!(
            CreditCharge::Scrape { url: "https://a" }.deduct_description(),
            "Scrape URL: https://a"
        );
        assert_eq!(
            CreditCharge::Crawl { url: "https://b" }.deduct_description(),
            "Crawl URL: https://b"
        );
        assert_eq!(
            CreditCharge::Extract { url_count: 2 }.deduct_description(),
            "Extract 2 URL(s)"
        );
        assert_eq!(
            CreditCharge::Scrape { url: "https://a" }.refund_description(),
            "Refund: enqueue failed for https://a"
        );
        assert_eq!(
            CreditCharge::Crawl { url: "https://b" }.refund_description(),
            "Refund: crawl creation failed for https://b"
        );
        assert_eq!(
            CreditCharge::Extract { url_count: 2 }.refund_description(),
            "Refund: extract enqueue failed for 2 URL(s)"
        );
    }

    /// 交易类型与能力一一对应。
    #[test]
    fn transaction_types_match_capability() {
        assert_eq!(
            CreditCharge::Scrape { url: "u" }.transaction_type(),
            CreditsTransactionType::Scrape
        );
        assert_eq!(
            CreditCharge::Crawl { url: "u" }.transaction_type(),
            CreditsTransactionType::Crawl
        );
        assert_eq!(
            CreditCharge::Extract { url_count: 1 }.transaction_type(),
            CreditsTransactionType::Extract
        );
    }
}
