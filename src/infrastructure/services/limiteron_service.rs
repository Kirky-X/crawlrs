// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Limiteron 服务实现
//!
//! 使用 limiteron 库实现速率限制、并发控制和配额管理功能

use std::sync::Arc;

use ahash::AHashMap;
use async_trait::async_trait;
use chrono::Utc;
use limiteron::prelude::*;
use limiteron::storage::{BanStorage, MemoryBanStorage, MemoryStorage, Storage};
use log::{debug, warn};

use crate::domain::repositories::{
    credits_repository::CreditsRepository, task_repository::TaskRepository,
    tasks_backlog_repository::TasksBacklogRepository,
};
use crate::domain::services::rate_limiting_service::{
    BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, QuotaService,
    RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError, RateLimitingService,
};

/// 限流服务配置
#[derive(Debug, Clone)]
pub struct RateLimitingConfig {
    /// Redis键前缀（保留用于兼容性）
    pub redis_key_prefix: String,
    /// 限流配置
    pub rate_limit: RateLimitConfig,
    /// 并发控制配置
    pub concurrency: ConcurrencyConfig,
    /// 积压任务处理间隔（秒）
    pub backlog_process_interval_seconds: u64,
    /// 限流记录过期时间（秒）
    pub rate_limit_ttl_seconds: u64,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            redis_key_prefix: "crawlrs:ratelimit".to_string(),
            rate_limit: RateLimitConfig::default(),
            concurrency: ConcurrencyConfig::default(),
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600,
        }
    }
}

/// Limiteron 服务实现
///
/// 使用 limiteron 库实现速率限制、并发控制和配额管理
#[derive(Clone)]
pub struct LimiteronService {
    /// Limiteron Governor 实例
    governor: Arc<Governor>,
    /// 限流服务配置
    config: RateLimitingConfig,
    /// 任务仓库
    task_repository: Arc<dyn TaskRepository>,
    /// 积压任务仓库
    tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
    /// 积分仓库
    credits_repository: Arc<dyn CreditsRepository>,
}

impl LimiteronService {
    /// 创建新的 LimiteronService
    pub async fn new(
        task_repository: Arc<dyn TaskRepository>,
        tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
        credits_repository: Arc<dyn CreditsRepository>,
        config: RateLimitingConfig,
    ) -> Result<Self, RateLimitingError> {
        // 创建内存存储（生产环境应使用 PostgreSQL 存储）
        let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
        let ban_storage: Arc<dyn BanStorage> = Arc::new(MemoryBanStorage::new());

        // 创建流量控制配置
        let flow_config = Self::build_flow_control_config(&config)?;

        // 构建 Governor
        let governor = Governor::builder()
            .with_config(flow_config)
            .with_storage(storage)
            .with_ban_storage(ban_storage)
            .with_l1_cache_enabled(false) // 禁用 L1 缓存，使用 Redis 缓存
            .build()
            .await
            .map_err(|e| RateLimitingError::ConfigurationError(e.to_string()))?;

        Ok(Self {
            governor: Arc::new(governor),
            config,
            task_repository,
            tasks_backlog_repository,
            credits_repository,
        })
    }

    /// 从配置构建 FlowControlConfig
    fn build_flow_control_config(
        config: &RateLimitingConfig,
    ) -> Result<FlowControlConfig, RateLimitingError> {
        use limiteron::config::{Action, ActionConfig, GlobalConfig, LimiterConfig, Matcher, Rule};

        // 构建用户限流规则（使用 User matcher 匹配 API Key）
        let user_rule = Rule {
            id: "user_rate_limit".to_string(),
            name: "User Rate Limit".to_string(),
            priority: 100,
            matchers: vec![Matcher::User {
                user_ids: vec!["*".to_string()],
            }],
            limiters: vec![LimiterConfig::TokenBucket {
                capacity: config.rate_limit.bucket_capacity.unwrap_or(100) as u64,
                refill_rate: config.rate_limit.requests_per_second as u64,
            }],
            action: ActionConfig {
                on_exceed: Action::Reject,
                ban: None,
            },
        };

        // 构建 IP 限流规则
        let ip_rule = Rule {
            id: "ip_rate_limit".to_string(),
            name: "IP Rate Limit".to_string(),
            priority: 90,
            matchers: vec![Matcher::Ip {
                ip_ranges: vec!["*".to_string()],
            }],
            limiters: vec![LimiterConfig::TokenBucket {
                capacity: config.rate_limit.bucket_capacity.unwrap_or(50) as u64,
                refill_rate: (config.rate_limit.requests_per_second / 2) as u64,
            }],
            action: ActionConfig {
                on_exceed: Action::Reject,
                ban: None,
            },
        };

        let flow_config = FlowControlConfig {
            version: "0.1.0".to_string(),
            global: GlobalConfig::default(),
            rules: vec![user_rule, ip_rule],
        };

        Ok(flow_config)
    }

    /// 构建请求上下文
    fn build_request_context(&self, api_key: &str, endpoint: &str) -> RequestContext {
        RequestContext {
            ip: None,
            user_id: Some(api_key.to_string()), // 使用 API Key 作为 user_id
            api_key: Some(api_key.to_string()),
            path: endpoint.to_string(),
            method: "GET".to_string(),
            headers: AHashMap::new(),
            query_params: AHashMap::new(),
            client_ip: None,
            mac: None,
            device_id: None,
        }
    }
}

#[async_trait]
impl RateLimitService for LimiteronService {
    async fn check_rate_limit(
        &self,
        api_key: &str,
        endpoint: &str,
    ) -> Result<RateLimitResult, RateLimitingError> {
        debug!(
            "LimiteronService: Checking rate limit for API key: {}..., endpoint: {}",
            &api_key[..std::cmp::min(8, api_key.len())],
            endpoint
        );

        if !self.config.rate_limit.enabled {
            debug!("LimiteronService: Rate limiting is disabled globally");
            return Ok(RateLimitResult::Allowed);
        }

        // 构建请求上下文
        let context = self.build_request_context(api_key, endpoint);

        // 使用 Governor 检查限流
        match self.governor.check(&context).await {
            Ok(Decision::Allowed(_)) => {
                debug!("LimiteronService: Rate limit check passed");
                Ok(RateLimitResult::Allowed)
            }
            Ok(Decision::Rejected(reason)) => {
                warn!(
                    "LimiteronService: Rate limit exceeded for API key: {}...: {:?}",
                    &api_key[..std::cmp::min(8, api_key.len())],
                    reason
                );
                Ok(RateLimitResult::Denied {
                    reason: reason.reason.clone(),
                })
            }
            Ok(Decision::Banned(ban_info)) => {
                warn!(
                    "LimiteronService: API key {}... is banned: {}",
                    &api_key[..std::cmp::min(8, api_key.len())],
                    ban_info.reason()
                );
                Ok(RateLimitResult::Denied {
                    reason: format!("Banned: {}", ban_info.reason()),
                })
            }
            Err(e) => {
                warn!(
                    "LimiteronService: Rate limit check error for API key: {}...: {}",
                    &api_key[..std::cmp::min(8, api_key.len())],
                    e
                );
                // 失败时允许请求（fail-open）
                Ok(RateLimitResult::Allowed)
            }
        }
    }

    async fn get_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError> {
        Ok(self.config.rate_limit.clone())
    }

    async fn update_team_rate_limit_config(
        &self,
        _team_id: uuid::Uuid,
        _config: RateLimitConfig,
    ) -> Result<(), RateLimitingError> {
        // 目前不支持动态更新配置
        // 未来可以添加动态配置更新逻辑
        Ok(())
    }

    async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
        // Limiteron 使用内存/数据库存储，自动处理过期
        // 这里返回 0 表示没有需要清理的记录
        Ok(0)
    }
}

#[async_trait]
impl ConcurrencyControlService for LimiteronService {
    async fn check_team_concurrency(
        &self,
        team_id: uuid::Uuid,
        task_id: uuid::Uuid,
    ) -> Result<ConcurrencyResult, RateLimitingError> {
        if !self.config.concurrency.enabled {
            return Ok(ConcurrencyResult::Allowed);
        }

        debug!(
            "LimiteronService: Checking team concurrency for team: {}, task: {}",
            team_id, task_id
        );

        // 尝试查找任务
        let task = match self.task_repository.find_by_id(task_id).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                warn!("LimiteronService: Task not found: {}", task_id);
                return Err(RateLimitingError::DatabaseError);
            }
            Err(e) => {
                log::error!("LimiteronService: Database error finding task: {:?}", e);
                return Err(RateLimitingError::DatabaseError);
            }
        };

        // 检查团队当前并发数
        let current_concurrency = self.get_team_current_concurrency(team_id).await?;

        if current_concurrency < self.config.concurrency.max_concurrent_per_team {
            debug!(
                "LimiteronService: Team {} concurrency check passed (current: {}, max: {})",
                team_id, current_concurrency, self.config.concurrency.max_concurrent_per_team
            );
            Ok(ConcurrencyResult::Allowed)
        } else {
            debug!(
                "LimiteronService: Team {} concurrency limit reached, queueing task",
                team_id
            );

            // 创建积压任务
            let backlog = crate::domain::repositories::tasks_backlog_repository::TasksBacklog::new(
                task_id,
                team_id,
                task.task_type.to_string(),
                task.priority,
                task.payload,
                task.expires_at.map(|dt| dt.with_timezone(&Utc)),
            );

            match self.tasks_backlog_repository.create(&backlog).await {
                Ok(saved_backlog) => Ok(ConcurrencyResult::Queued {
                    backlog_id: saved_backlog.id,
                }),
                Err(e) => {
                    log::error!("LimiteronService: Database error creating backlog: {:?}", e);
                    Err(RateLimitingError::DatabaseError)
                }
            }
        }
    }

    async fn release_team_concurrency_slot(
        &self,
        team_id: uuid::Uuid,
        _task_id: uuid::Uuid,
    ) -> Result<(), RateLimitingError> {
        debug!(
            "LimiteronService: Releasing concurrency slot for team: {}",
            team_id
        );

        // 处理积压任务
        self.process_backlog_tasks(team_id).await?;

        Ok(())
    }

    async fn get_team_current_concurrency(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<u32, RateLimitingError> {
        // 在实际实现中，应该从存储中获取当前并发数
        // 这里暂时返回 0，生产环境需要从 Redis 或数据库获取
        Ok(0)
    }

    async fn get_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError> {
        Ok(self.config.concurrency.clone())
    }

    async fn update_team_concurrency_config(
        &self,
        _team_id: uuid::Uuid,
        _config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError> {
        // 目前不支持动态更新配置
        Ok(())
    }
}

#[async_trait]
impl BacklogService for LimiteronService {
    async fn process_backlog_tasks(&self, team_id: uuid::Uuid) -> Result<u32, RateLimitingError> {
        debug!(
            "LimiteronService: Processing backlog tasks for team: {}",
            team_id
        );

        // 获取待处理的积压任务
        let pending_backlogs = match self
            .tasks_backlog_repository
            .get_pending_tasks(Some(team_id), Some(10))
            .await
        {
            Ok(backlogs) => backlogs,
            Err(e) => {
                log::error!(
                    "LimiteronService: Database error getting pending tasks: {:?}",
                    e
                );
                return Err(RateLimitingError::DatabaseError);
            }
        };

        let mut processed_count = 0;

        for backlog in pending_backlogs {
            // 检查任务是否已过期
            if backlog.is_expired() {
                let mut expired_backlog = backlog.clone();
                if let Err(e) = expired_backlog.mark_expired() {
                    log::error!("LimiteronService: Error marking backlog expired: {}", e);
                    continue;
                }

                if let Err(e) = self.tasks_backlog_repository.update(&expired_backlog).await {
                    log::error!("LimiteronService: Database error updating backlog: {:?}", e);
                }
                continue;
            }

            // 尝试获取并发槽位
            let current_concurrency = self.get_team_current_concurrency(team_id).await?;

            if current_concurrency < self.config.concurrency.max_concurrent_per_team {
                // 成功获取槽位，更新任务状态
                if let Ok(Some(mut task)) = self.task_repository.find_by_id(backlog.task_id).await {
                    if task.status == crate::domain::models::TaskStatus::Queued {
                        task.status = crate::domain::models::TaskStatus::Active;
                        task.started_at = Some(chrono::Utc::now());
                        task.lock_token = None;
                        task.lock_expires_at =
                            Some(chrono::Utc::now() + chrono::Duration::seconds(300));

                        if let Err(e) = self.task_repository.update(&task).await {
                            log::error!("LimiteronService: Database error updating task: {:?}", e);
                            continue;
                        }
                    }
                }

                // 更新积压任务状态
                let mut updated_backlog = backlog.clone();
                if let Err(e) = updated_backlog.mark_completed() {
                    log::error!("LimiteronService: Error marking backlog completed: {}", e);
                    continue;
                }

                if let Err(e) = self.tasks_backlog_repository.update(&updated_backlog).await {
                    log::error!("LimiteronService: Database error updating backlog: {:?}", e);
                    continue;
                }

                processed_count += 1;
            } else {
                // 无法获取槽位，停止处理
                break;
            }
        }

        debug!(
            "LimiteronService: Processed {} backlog tasks for team: {}",
            processed_count, team_id
        );

        Ok(processed_count)
    }
}

#[async_trait]
impl QuotaService for LimiteronService {
    async fn check_and_deduct_quota(
        &self,
        team_id: uuid::Uuid,
        amount: i64,
        transaction_type: crate::domain::models::CreditsTransactionType,
        description: String,
        reference_id: Option<uuid::Uuid>,
    ) -> Result<(), RateLimitingError> {
        debug!(
            "LimiteronService: Checking and deducting quota for team: {}, amount: {}",
            team_id, amount
        );

        // 检查余额
        let balance = match self.credits_repository.get_balance(team_id).await {
            Ok(balance) => balance,
            Err(e) => {
                log::error!("LimiteronService: Credits error getting balance: {:?}", e);
                return Err(RateLimitingError::CreditsError);
            }
        };

        if balance < amount {
            return Err(RateLimitingError::RateLimitExceeded(format!(
                "Insufficient credits: required {}, available {}",
                amount, balance
            )));
        }

        // 扣除积分
        match self
            .credits_repository
            .deduct_credits(team_id, amount, transaction_type, description, reference_id)
            .await
        {
            Ok(_) => {
                debug!(
                    "LimiteronService: Quota deducted successfully for team: {}",
                    team_id
                );
                Ok(())
            }
            Err(e) => {
                log::error!("LimiteronService: Credits error deducting: {:?}", e);
                Err(RateLimitingError::CreditsError)
            }
        }
    }

    async fn get_quota_balance(&self, team_id: uuid::Uuid) -> Result<i64, RateLimitingError> {
        match self.credits_repository.get_balance(team_id).await {
            Ok(balance) => Ok(balance),
            Err(e) => {
                log::error!("LimiteronService: Credits error getting balance: {:?}", e);
                Err(RateLimitingError::CreditsError)
            }
        }
    }
}

/// 为 LimiteronService 实现组合 trait RateLimitingService（向后兼容）
#[async_trait]
impl RateLimitingService for LimiteronService {}

#[cfg(all(test, feature = "rate-limiting"))]
mod tests {
    use super::*;
    use limiteron::config::LimiterConfig;

    // ========== RateLimitingConfig::default() tests ==========

    #[test]
    fn test_rate_limiting_config_default_values() {
        let config = RateLimitingConfig::default();
        assert_eq!(config.redis_key_prefix, "crawlrs:ratelimit");
        assert_eq!(config.backlog_process_interval_seconds, 30);
        assert_eq!(config.rate_limit_ttl_seconds, 3600);
        assert!(config.rate_limit.enabled);
        assert!(config.concurrency.enabled);
    }

    // ========== build_flow_control_config tests ==========

    #[test]
    fn test_build_flow_control_config_default() {
        let config = RateLimitingConfig::default();
        let flow_config = LimiteronService::build_flow_control_config(&config).unwrap();

        assert_eq!(flow_config.version, "0.1.0");
        assert_eq!(flow_config.rules.len(), 2);

        // user_rate_limit rule
        assert_eq!(flow_config.rules[0].id, "user_rate_limit");
        assert_eq!(flow_config.rules[0].name, "User Rate Limit");
        assert_eq!(flow_config.rules[0].priority, 100);

        // ip_rate_limit rule
        assert_eq!(flow_config.rules[1].id, "ip_rate_limit");
        assert_eq!(flow_config.rules[1].name, "IP Rate Limit");
        assert_eq!(flow_config.rules[1].priority, 90);
    }

    #[test]
    fn test_build_flow_control_config_custom_bucket_capacity() {
        let mut config = RateLimitingConfig::default();
        config.rate_limit.bucket_capacity = Some(200);
        config.rate_limit.requests_per_second = 20;

        let flow_config = LimiteronService::build_flow_control_config(&config).unwrap();

        // With custom bucket_capacity=200, the user rule capacity should be 200
        let user_limiter = &flow_config.rules[0].limiters[0];
        match user_limiter {
            LimiterConfig::TokenBucket { capacity, .. } => {
                assert_eq!(*capacity, 200);
            }
            _ => panic!("Expected TokenBucket limiter for user rule"),
        }
    }

    #[test]
    fn test_build_flow_control_config_none_bucket_capacity_uses_defaults() {
        let mut config = RateLimitingConfig::default();
        config.rate_limit.bucket_capacity = None;

        let flow_config = LimiteronService::build_flow_control_config(&config).unwrap();

        // With None bucket_capacity, user rule defaults to 100, ip rule to 50
        let user_limiter = &flow_config.rules[0].limiters[0];
        match user_limiter {
            LimiterConfig::TokenBucket { capacity, .. } => {
                assert_eq!(*capacity, 100);
            }
            _ => panic!("Expected TokenBucket limiter for user rule"),
        }

        let ip_limiter = &flow_config.rules[1].limiters[0];
        match ip_limiter {
            LimiterConfig::TokenBucket { capacity, .. } => {
                assert_eq!(*capacity, 50);
            }
            _ => panic!("Expected TokenBucket limiter for ip rule"),
        }
    }

    // Note: build_request_context is an instance method that requires a
    // LimiteronService instance. However, LimiteronService::new() fails due to
    // a pre-existing bug in build_flow_control_config: it uses "*" as an IP
    // range, which the limiteron Governor rejects as an invalid IP address.
    // Testing build_request_context requires either fixing that bug or adding
    // a test-only constructor, both of which are out of scope for this task.
}
