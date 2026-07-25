// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Noop 限流服务实现（rate-limit feature 关闭时使用）
//!
//! R-rl-002 / T018-T019：当 `rate-limit` feature 关闭时，`init_rate_limiting_service`
//! 装配此 `NoopRateLimitingService` 替代 `LimiteronService`，所有方法返回放行/成功，
//! 保证 handler 内 `check_rate_limit`/`check_and_deduct_quota` 调用经 trait 走 Noop 放行。
//!
//! # 契约
//!
//! - `check_rate_limit` → `Allowed`（不限流）
//! - `check_team_concurrency` → `Allowed`（不限制并发）
//! - `check_and_deduct_quota` → `Ok(())`（不扣配额）
//! - `get_quota_balance` → `Ok(i64::MAX)`（无限配额）
//! - `process_backlog_tasks` → `Ok(0)`（幂等空转，不处理积压）
//! - 其余方法返回合理默认值（配置/计数器等）

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::models::CreditsTransactionType;
use crate::domain::services::rate_limiting_service::{
    BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, QuotaService,
    RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError, RateLimitingService,
};

/// Noop 限流服务
///
/// R-rl-002 / T019：rate-limit feature 关闭时的限流服务实现。
/// 所有方法返回放行/成功，保证业务逻辑在无限流后端时正常运转。
#[derive(Debug, Clone, Default)]
pub struct NoopRateLimitingService;

impl NoopRateLimitingService {
    /// 创建新的 NoopRateLimitingService
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RateLimitService for NoopRateLimitingService {
    /// 检查 API 限流 → 始终放行
    async fn check_rate_limit(
        &self,
        _api_key: &str,
        _endpoint: &str,
    ) -> Result<RateLimitResult, RateLimitingError> {
        Ok(RateLimitResult::Allowed)
    }

    /// 获取团队的限流配置 → 返回默认配置
    async fn get_team_rate_limit_config(
        &self,
        _team_id: Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError> {
        Ok(RateLimitConfig::default())
    }

    /// 更新团队的限流配置 → 空操作成功
    async fn update_team_rate_limit_config(
        &self,
        _team_id: Uuid,
        _config: RateLimitConfig,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }

    /// 清理过期的限流记录 → 返回 0（无清理）
    async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
        Ok(0)
    }
}

#[async_trait]
impl ConcurrencyControlService for NoopRateLimitingService {
    /// 检查团队并发限制 → 始终放行
    async fn check_team_concurrency(
        &self,
        _team_id: Uuid,
        _task_id: Uuid,
    ) -> Result<ConcurrencyResult, RateLimitingError> {
        Ok(ConcurrencyResult::Allowed)
    }

    /// 释放团队并发槽位 → 空操作成功
    async fn release_team_concurrency_slot(
        &self,
        _team_id: Uuid,
        _task_id: Uuid,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }

    /// 获取团队的当前并发数 → 返回 0
    async fn get_team_current_concurrency(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
        Ok(0)
    }

    /// 获取团队的并发配置 → 返回默认配置
    async fn get_team_concurrency_config(
        &self,
        _team_id: Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError> {
        Ok(ConcurrencyConfig::default())
    }

    /// 更新团队的并发配置 → 空操作成功
    async fn update_team_concurrency_config(
        &self,
        _team_id: Uuid,
        _config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }
}

#[async_trait]
impl BacklogService for NoopRateLimitingService {
    /// 处理积压任务 → 返回 0（幂等空转，不处理积压）
    ///
    /// R-rl-004 / T022：rate-limit 关闭时不门控 backlog_worker，
    /// backlog 处理走 Noop 返回 0，保证幂等空转不产生副作用。
    async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
        Ok(0)
    }
}

#[async_trait]
impl QuotaService for NoopRateLimitingService {
    /// 检查并扣除团队配额 → 始终成功（不扣配额）
    async fn check_and_deduct_quota(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), RateLimitingError> {
        Ok(())
    }

    /// 获取团队配额余额 → 返回 i64::MAX（无限配额）
    async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
        Ok(i64::MAX)
    }
}

/// 自动实现组合 trait `RateLimitingService`
///
/// R-rl-002 / T019：`RateLimitingService` 是 `RateLimitService` +
/// `ConcurrencyControlService` + `BacklogService` + `QuotaService` 的组合 trait，
/// 上述四个 trait 已全部实现，组合 trait 自动满足。
#[async_trait]
impl RateLimitingService for NoopRateLimitingService {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::RateLimitResult;
    use uuid::Uuid;

    // R-rl-002 / T018：以下测试钉住 Noop 放行契约。

    #[tokio::test]
    async fn test_noop_check_rate_limit_returns_allowed() {
        let svc = NoopRateLimitingService::new();
        let result = svc.check_rate_limit("test_api_key", "/v1/crawl").await;
        assert!(result.is_ok(), "check_rate_limit should always succeed");
        assert_eq!(
            result.unwrap(),
            RateLimitResult::Allowed,
            "check_rate_limit should return Allowed (no rate limiting)"
        );
    }

    #[tokio::test]
    async fn test_noop_get_quota_balance_returns_max() {
        let svc = NoopRateLimitingService::new();
        let team_id = Uuid::new_v4();
        let result = svc.get_quota_balance(team_id).await;
        assert!(result.is_ok(), "get_quota_balance should always succeed");
        assert_eq!(
            result.unwrap(),
            i64::MAX,
            "get_quota_balance should return i64::MAX (unlimited quota)"
        );
    }

    #[tokio::test]
    async fn test_noop_check_and_deduct_quota_returns_ok() {
        let svc = NoopRateLimitingService::new();
        let team_id = Uuid::new_v4();
        let result = svc
            .check_and_deduct_quota(
                team_id,
                100,
                CreditsTransactionType::Crawl,
                "test deduction".to_string(),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "check_and_deduct_quota should always succeed (no deduction)"
        );
    }

    /// R-rl-004 / T022：rate-limit 关闭时 backlog 幂等空转契约
    ///
    /// `process_backlog_tasks` 返回 `Ok(0)`，表示：
    /// - 不处理任何积压任务（幂等空转）
    /// - 不产生副作用
    /// - backlog_worker 不门控，但走 Noop 后不会实际执行业务逻辑
    #[tokio::test]
    async fn test_noop_process_backlog_tasks_returns_zero() {
        let svc = NoopRateLimitingService::new();
        let team_id = Uuid::new_v4();
        let result = svc.process_backlog_tasks(team_id).await;
        assert!(
            result.is_ok(),
            "process_backlog_tasks should always succeed"
        );
        assert_eq!(
            result.unwrap(),
            0,
            "process_backlog_tasks should return 0 (no backlog processed)"
        );
    }

    /// R-rl-004 / T022：多次调用 process_backlog_tasks 保持幂等
    ///
    /// 验证连续调用 `process_backlog_tasks` 始终返回 `Ok(0)`，
    /// 不会因状态累积而产生副作用。
    #[tokio::test]
    async fn test_noop_process_backlog_tasks_idempotent_across_calls() {
        let svc = NoopRateLimitingService::new();
        let team_id = Uuid::new_v4();

        for i in 0..3 {
            let result = svc.process_backlog_tasks(team_id).await;
            assert!(
                result.is_ok(),
                "call {} should succeed",
                i
            );
            assert_eq!(
                result.unwrap(),
                0,
                "call {} should return 0 (idempotent)",
                i
            );
        }
    }
}
