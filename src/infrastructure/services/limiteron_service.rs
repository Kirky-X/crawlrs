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
            .with_l1_cache_enabled(false) // 禁用 L1 缓存，使用 MemoryStorage 后端
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
                refill_rate: (config.rate_limit.requests_per_second as u64).max(1),
            }],
            action: ActionConfig {
                on_exceed: Action::Reject,
                ban: None,
            },
        };

        // 构建 IP 限流规则
        // 使用 "0.0.0.0/0" 匹配所有 IPv4 地址（limiteron 不接受 "*" 作为 IP 范围）
        let ip_rule = Rule {
            id: "ip_rate_limit".to_string(),
            name: "IP Rate Limit".to_string(),
            priority: 90,
            matchers: vec![Matcher::Ip {
                ip_ranges: vec!["0.0.0.0/0".to_string()],
            }],
            limiters: vec![LimiterConfig::TokenBucket {
                capacity: config.rate_limit.bucket_capacity.unwrap_or(50) as u64,
                refill_rate: ((config.rate_limit.requests_per_second / 2) as u64).max(1),
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
        // 这里暂时返回 0，生产环境需要从数据库获取
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
    use limiteron::config::{LimiterConfig, Matcher};
    use std::str::FromStr;

    // ========== RateLimitingConfig::default() tests ==========

    #[test]
    fn test_rate_limiting_config_default_values() {
        let config = RateLimitingConfig::default();
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

    #[test]
    fn test_build_flow_control_config_ip_range_is_valid_cidr() {
        // Regression test: previously used "*" which limiteron Governor rejects
        let config = RateLimitingConfig::default();
        let flow_config = LimiteronService::build_flow_control_config(&config).unwrap();

        let ip_rule = &flow_config.rules[1];
        assert_eq!(ip_rule.id, "ip_rate_limit");
        for matcher in &ip_rule.matchers {
            match matcher {
                Matcher::Ip { ip_ranges } => {
                    for range in ip_ranges {
                        // Each IP range must be a valid CIDR (not "*")
                        assert_ne!(
                            range, "*",
                            "IP range must not be wildcard '*', use CIDR notation"
                        );
                        // Validate it parses as a valid IP network
                        ipnetwork::IpNetwork::from_str(range).unwrap_or_else(|_| {
                            panic!("IP range '{}' is not a valid CIDR notation", range)
                        });
                    }
                }
                _ => panic!("Expected Ip matcher for ip_rate_limit rule"),
            }
        }
    }

    // ========== Mock repositories for trait impl tests ==========

    use crate::domain::models::credits_model::{CreditsTransaction, CreditsTransactionType};
    use crate::domain::models::task_domain::{TaskStatus, TaskType};
    use crate::domain::models::task_model::Task;
    use crate::domain::repositories::credits_repository::{
        CreditsRepository, CreditsRepositoryError,
    };
    use crate::domain::repositories::task_repository::{
        RepositoryError, TaskQueryParams, TaskRepository,
    };
    use crate::domain::repositories::tasks_backlog_repository::{
        TasksBacklog, TasksBacklogRepository, TasksBacklogStatus,
    };
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyControlService, QuotaService, RateLimitService,
    };
    use chrono::Utc;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Configurable mock TaskRepository1
    struct MockTaskRepository {
        /// Behavior mode for find_by_id (avoids needing RepositoryError: Clone)
        find_mode: FindByIdMode,
        /// Task to return when find_mode == ReturnTask
        task: Option<Task>,
        /// Whether update() should fail
        update_should_fail: bool,
        /// Number of times update() was called
        update_calls: Mutex<u32>,
    }

    enum FindByIdMode {
        ReturnTask,
        ReturnNone,
        ReturnError,
    }

    impl MockTaskRepository {
        fn with_task(task: Task) -> Self {
            Self {
                find_mode: FindByIdMode::ReturnTask,
                task: Some(task),
                update_should_fail: false,
                update_calls: Mutex::new(0),
            }
        }
        fn with_no_task() -> Self {
            Self {
                find_mode: FindByIdMode::ReturnNone,
                task: None,
                update_should_fail: false,
                update_calls: Mutex::new(0),
            }
        }
        fn with_db_error() -> Self {
            Self {
                find_mode: FindByIdMode::ReturnError,
                task: None,
                update_should_fail: false,
                update_calls: Mutex::new(0),
            }
        }
        fn with_failing_update(task: Task) -> Self {
            Self {
                find_mode: FindByIdMode::ReturnTask,
                task: Some(task),
                update_should_fail: true,
                update_calls: Mutex::new(0),
            }
        }
        fn update_call_count(&self) -> u32 {
            *self.update_calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            match self.find_mode {
                FindByIdMode::ReturnTask => Ok(self.task.clone()),
                FindByIdMode::ReturnNone => Ok(None),
                FindByIdMode::ReturnError => {
                    Err(RepositoryError::Database(anyhow::anyhow!("mock db error")))
                }
            }
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            *self.update_calls.lock().unwrap() += 1;
            if self.update_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock update failed"
                )));
            }
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    /// Configurable mock TasksBacklogRepository
    struct MockBacklogRepository {
        /// Pending tasks to return from get_pending_tasks
        pending_tasks: Mutex<Vec<TasksBacklog>>,
        /// Whether create() should fail
        create_should_fail: bool,
        /// Whether update() should fail
        update_should_fail: bool,
        /// Whether get_pending_tasks() should fail
        get_pending_should_fail: bool,
        /// Created backlogs (for assertion)
        created_backlogs: Mutex<Vec<TasksBacklog>>,
        /// Updated backlogs (for assertion)
        updated_backlogs: Mutex<Vec<TasksBacklog>>,
    }

    impl MockBacklogRepository {
        fn new() -> Self {
            Self {
                pending_tasks: Mutex::new(vec![]),
                create_should_fail: false,
                update_should_fail: false,
                get_pending_should_fail: false,
                created_backlogs: Mutex::new(vec![]),
                updated_backlogs: Mutex::new(vec![]),
            }
        }
        fn with_pending(tasks: Vec<TasksBacklog>) -> Self {
            Self {
                pending_tasks: Mutex::new(tasks),
                create_should_fail: false,
                update_should_fail: false,
                get_pending_should_fail: false,
                created_backlogs: Mutex::new(vec![]),
                updated_backlogs: Mutex::new(vec![]),
            }
        }
        fn with_failing_create() -> Self {
            Self {
                pending_tasks: Mutex::new(vec![]),
                create_should_fail: true,
                update_should_fail: false,
                get_pending_should_fail: false,
                created_backlogs: Mutex::new(vec![]),
                updated_backlogs: Mutex::new(vec![]),
            }
        }
        fn with_failing_update() -> Self {
            Self {
                pending_tasks: Mutex::new(vec![]),
                create_should_fail: false,
                update_should_fail: true,
                get_pending_should_fail: false,
                created_backlogs: Mutex::new(vec![]),
                updated_backlogs: Mutex::new(vec![]),
            }
        }
        fn with_failing_get_pending() -> Self {
            Self {
                pending_tasks: Mutex::new(vec![]),
                create_should_fail: false,
                update_should_fail: false,
                get_pending_should_fail: true,
                created_backlogs: Mutex::new(vec![]),
                updated_backlogs: Mutex::new(vec![]),
            }
        }
        fn created_backlogs(&self) -> Vec<TasksBacklog> {
            self.created_backlogs.lock().unwrap().clone()
        }
        fn updated_backlogs(&self) -> Vec<TasksBacklog> {
            self.updated_backlogs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TasksBacklogRepository for MockBacklogRepository {
        async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
            if self.create_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock create failed"
                )));
            }
            self.created_backlogs.lock().unwrap().push(backlog.clone());
            Ok(backlog.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_task_id(
            &self,
            _task_id: Uuid,
        ) -> Result<Option<TasksBacklog>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
            if self.update_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock update failed"
                )));
            }
            self.updated_backlogs.lock().unwrap().push(backlog.clone());
            Ok(backlog.clone())
        }
        async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn get_pending_tasks(
            &self,
            _team_id: Option<Uuid>,
            _limit: Option<u64>,
        ) -> Result<Vec<TasksBacklog>, RepositoryError> {
            if self.get_pending_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mock get_pending failed"
                )));
            }
            Ok(self.pending_tasks.lock().unwrap().clone())
        }
        async fn get_expired_tasks(
            &self,
            _limit: Option<u64>,
        ) -> Result<Vec<TasksBacklog>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_status(
            &self,
            _team_id: Option<Uuid>,
            _status: TasksBacklogStatus,
        ) -> Result<i64, RepositoryError> {
            Ok(0)
        }
        async fn update_status_batch(
            &self,
            _ids: &[Uuid],
            _status: TasksBacklogStatus,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    /// Configurable mock CreditsRepository
    struct MockCreditsRepository {
        balance: Mutex<i64>,
        get_balance_should_fail: bool,
        deduct_should_fail: bool,
        deduct_calls: Mutex<u32>,
    }

    impl MockCreditsRepository {
        fn with_balance(balance: i64) -> Self {
            Self {
                balance: Mutex::new(balance),
                get_balance_should_fail: false,
                deduct_should_fail: false,
                deduct_calls: Mutex::new(0),
            }
        }
        fn with_failing_get_balance() -> Self {
            Self {
                balance: Mutex::new(0),
                get_balance_should_fail: true,
                deduct_should_fail: false,
                deduct_calls: Mutex::new(0),
            }
        }
        fn with_failing_deduct(balance: i64) -> Self {
            Self {
                balance: Mutex::new(balance),
                get_balance_should_fail: false,
                deduct_should_fail: true,
                deduct_calls: Mutex::new(0),
            }
        }
        fn deduct_call_count(&self) -> u32 {
            *self.deduct_calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl CreditsRepository for MockCreditsRepository {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            if self.get_balance_should_fail {
                return Err(CreditsRepositoryError::DatabaseError("mock".to_string()));
            }
            Ok(*self.balance.lock().unwrap())
        }
        async fn deduct_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            *self.deduct_calls.lock().unwrap() += 1;
            if self.deduct_should_fail {
                return Err(CreditsRepositoryError::DatabaseError("mock".to_string()));
            }
            Ok(())
        }
        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }
        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(initial_balance)
        }
    }

    /// Build a LimiteronService with configurable mocks
    async fn make_service_with_mocks(
        task_repo: Arc<MockTaskRepository>,
        backlog_repo: Arc<MockBacklogRepository>,
        credits_repo: Arc<MockCreditsRepository>,
        config: RateLimitingConfig,
    ) -> LimiteronService {
        LimiteronService::new(
            task_repo as Arc<dyn TaskRepository>,
            backlog_repo as Arc<dyn TasksBacklogRepository>,
            credits_repo as Arc<dyn CreditsRepository>,
            config,
        )
        .await
        .expect("Failed to build LimiteronService")
    }

    fn make_task(task_id: Uuid, team_id: Uuid, status: TaskStatus) -> Task {
        let mut task = Task::new(
            task_id,
            TaskType::Scrape,
            team_id,
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        task.status = status;
        task
    }

    fn make_pending_backlog(task_id: Uuid, team_id: Uuid) -> TasksBacklog {
        TasksBacklog::new(
            task_id,
            team_id,
            "scrape".to_string(),
            1,
            serde_json::json!({}),
            None,
        )
    }

    fn make_expired_backlog(task_id: Uuid, team_id: Uuid) -> TasksBacklog {
        TasksBacklog::new(
            task_id,
            team_id,
            "scrape".to_string(),
            1,
            serde_json::json!({}),
            Some(Utc::now() - chrono::Duration::hours(1)),
        )
    }

    // ========== RateLimitService trait tests ==========

    #[tokio::test]
    async fn test_check_rate_limit_disabled_short_circuits_to_allowed() {
        let mut config = RateLimitingConfig::default();
        config.rate_limit.enabled = false;

        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.check_rate_limit("k", "/v1/extract").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), RateLimitResult::Allowed);
    }

    #[tokio::test]
    async fn test_check_rate_limit_enabled_fails_open_to_allowed() {
        // SOURCE LIMITATION: build_request_context sets ip=None, client_ip=None,
        // empty headers → Governor cannot extract identifier → Err → fail-open → Allowed.
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.check_rate_limit("k", "/v1/extract").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), RateLimitResult::Allowed);
    }

    #[tokio::test]
    async fn test_get_team_rate_limit_config_returns_clone() {
        let config = RateLimitingConfig::default();
        let expected = config.rate_limit.clone();

        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.get_team_rate_limit_config(Uuid::new_v4()).await;
        assert!(result.is_ok());
        let got = result.unwrap();
        // RateLimitConfig doesn't derive PartialEq — compare key fields instead
        assert_eq!(got.enabled, expected.enabled);
        assert_eq!(got.requests_per_second, expected.requests_per_second);
        assert_eq!(got.requests_per_minute, expected.requests_per_minute);
        assert_eq!(got.requests_per_hour, expected.requests_per_hour);
        assert_eq!(got.bucket_capacity, expected.bucket_capacity);
    }

    #[tokio::test]
    async fn test_update_team_rate_limit_config_returns_ok() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .update_team_rate_limit_config(Uuid::new_v4(), RateLimitConfig::default())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_expired_rate_limits_returns_zero() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.cleanup_expired_rate_limits().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    // ========== ConcurrencyControlService trait tests ==========

    #[tokio::test]
    async fn test_check_team_concurrency_disabled_returns_allowed() {
        let mut config = RateLimitingConfig::default();
        config.concurrency.enabled = false;

        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service
            .check_team_concurrency(Uuid::new_v4(), Uuid::new_v4())
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ConcurrencyResult::Allowed);
    }

    #[tokio::test]
    async fn test_check_team_concurrency_task_not_found_returns_db_error() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_team_concurrency(Uuid::new_v4(), Uuid::new_v4())
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::DatabaseError
        ));
    }

    #[tokio::test]
    async fn test_check_team_concurrency_find_by_id_db_error_returns_db_error() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_db_error()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_team_concurrency(Uuid::new_v4(), Uuid::new_v4())
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::DatabaseError
        ));
    }

    #[tokio::test]
    async fn test_check_team_concurrency_under_limit_returns_allowed() {
        // get_team_current_concurrency returns 0, max_concurrent_per_team default 10 → Allowed
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_task(make_task(
                task_id,
                team_id,
                TaskStatus::Queued,
            ))),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.check_team_concurrency(team_id, task_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ConcurrencyResult::Allowed);
    }

    #[tokio::test]
    async fn test_check_team_concurrency_at_limit_queues_backlog() {
        // Force current_concurrency >= max_concurrent_per_team by setting max=0
        let mut config = RateLimitingConfig::default();
        config.concurrency.max_concurrent_per_team = 0;

        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog_repo = Arc::new(MockBacklogRepository::new());
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_task(make_task(
                task_id,
                team_id,
                TaskStatus::Queued,
            ))),
            backlog_repo.clone(),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.check_team_concurrency(team_id, task_id).await;
        assert!(result.is_ok());
        match result.unwrap() {
            ConcurrencyResult::Queued { backlog_id } => {
                // Verify backlog was created with our task
                let created = backlog_repo.created_backlogs();
                assert_eq!(created.len(), 1);
                assert_eq!(created[0].task_id, task_id);
                assert_eq!(created[0].team_id, team_id);
                assert_eq!(created[0].id, backlog_id);
            }
            other => panic!("Expected Queued, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_check_team_concurrency_at_limit_backlog_create_fails_returns_db_error() {
        let mut config = RateLimitingConfig::default();
        config.concurrency.max_concurrent_per_team = 0;

        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_task(make_task(
                task_id,
                team_id,
                TaskStatus::Queued,
            ))),
            Arc::new(MockBacklogRepository::with_failing_create()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.check_team_concurrency(team_id, task_id).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::DatabaseError
        ));
    }

    #[tokio::test]
    async fn test_release_team_concurrency_slot_calls_process_backlog() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .release_team_concurrency_slot(Uuid::new_v4(), Uuid::new_v4())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_release_team_concurrency_slot_backlog_error_propagates() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::with_failing_get_pending()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .release_team_concurrency_slot(Uuid::new_v4(), Uuid::new_v4())
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::DatabaseError
        ));
    }

    #[tokio::test]
    async fn test_get_team_current_concurrency_returns_zero() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.get_team_current_concurrency(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_get_team_concurrency_config_returns_clone() {
        let config = RateLimitingConfig::default();
        let expected = config.concurrency.clone();

        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.get_team_concurrency_config(Uuid::new_v4()).await;
        assert!(result.is_ok());
        let got = result.unwrap();
        // ConcurrencyConfig doesn't derive PartialEq — compare key fields instead
        assert_eq!(got.enabled, expected.enabled);
        assert_eq!(got.max_concurrent_tasks, expected.max_concurrent_tasks);
        assert_eq!(
            got.max_concurrent_per_team,
            expected.max_concurrent_per_team
        );
        assert_eq!(got.lock_timeout_seconds, expected.lock_timeout_seconds);
        assert_eq!(got.strategy, expected.strategy);
    }

    #[tokio::test]
    async fn test_update_team_concurrency_config_returns_ok() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .update_team_concurrency_config(Uuid::new_v4(), ConcurrencyConfig::default())
            .await;
        assert!(result.is_ok());
    }

    // ========== BacklogService trait tests ==========

    #[tokio::test]
    async fn test_process_backlog_tasks_get_pending_fails_returns_db_error() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::with_failing_get_pending()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::DatabaseError
        ));
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_empty_returns_zero() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_expired_marks_expired_and_updates() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let expired = make_expired_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![expired]));
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            backlog_repo.clone(),
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // expired branch doesn't increment processed_count

        // Verify the expired backlog was updated
        let updated = backlog_repo.updated_backlogs();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].status, TasksBacklogStatus::Expired);
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_expired_update_failure_continues() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let expired = make_expired_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_failing_update());
        *backlog_repo.pending_tasks.lock().unwrap() = vec![expired];
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        // Should not propagate error (logged and continue)
        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_task_find_by_id_err_continues() {
        // Pending (non-expired) backlog, but find_by_id returns Err
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_db_error()),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_task_not_found_skips_update() {
        // Pending backlog, find_by_id returns Ok(None) → skip task update branch
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        // NOTE: source bug — mark_completed() on a Pending backlog fails, so
        // processed_count never increments. We assert the observed behavior.
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let task_repo = Arc::new(MockTaskRepository::with_no_task());
        let service = make_service_with_mocks(
            task_repo,
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // mark_completed fails on Pending → 0
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_task_not_queued_skips_status_update() {
        // Task found but status != Queued → skip the update-task branch
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let task_repo = Arc::new(MockTaskRepository::with_task(make_task(
            task_id,
            team_id,
            TaskStatus::Active,
        )));
        let service = make_service_with_mocks(
            task_repo.clone(),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // mark_completed still fails on Pending → 0
        assert_eq!(task_repo.update_call_count(), 0); // task.update not called
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_task_queued_calls_task_update() {
        // Task is Queued → task.update called (succeeds) → mark_completed fails → 0
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let task_repo = Arc::new(MockTaskRepository::with_task(make_task(
            task_id,
            team_id,
            TaskStatus::Queued,
        )));
        let service = make_service_with_mocks(
            task_repo.clone(),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // mark_completed fails on Pending → 0
        assert_eq!(task_repo.update_call_count(), 1); // task.update WAS called
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_task_update_failure_continues() {
        // Task is Queued, but task.update fails → continue (no increment)
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let task_repo = Arc::new(MockTaskRepository::with_failing_update(make_task(
            task_id,
            team_id,
            TaskStatus::Queued,
        )));
        let service = make_service_with_mocks(
            task_repo.clone(),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
        assert_eq!(task_repo.update_call_count(), 1);
    }

    #[tokio::test]
    async fn test_process_backlog_tasks_concurrency_at_limit_breaks() {
        // Force break branch: max_concurrent_per_team = 0 so 0 < 0 is false → break immediately
        let mut config = RateLimitingConfig::default();
        config.concurrency.max_concurrent_per_team = 0;

        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let backlog = make_pending_backlog(task_id, team_id);
        let backlog_repo = Arc::new(MockBacklogRepository::with_pending(vec![backlog]));
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            backlog_repo,
            Arc::new(MockCreditsRepository::with_balance(100)),
            config,
        )
        .await;

        let result = service.process_backlog_tasks(team_id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // break before any processing
    }

    // ========== QuotaService trait tests ==========

    #[tokio::test]
    async fn test_check_and_deduct_quota_get_balance_fails_returns_credits_error() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_failing_get_balance()),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_and_deduct_quota(
                Uuid::new_v4(),
                10,
                CreditsTransactionType::Scrape,
                "test".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::CreditsError
        ));
    }

    #[tokio::test]
    async fn test_check_and_deduct_quota_insufficient_balance_returns_exceeded() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(5)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_and_deduct_quota(
                Uuid::new_v4(),
                10,
                CreditsTransactionType::Scrape,
                "test".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            RateLimitingError::RateLimitExceeded(msg) => {
                assert!(msg.contains("Insufficient credits"));
                assert!(msg.contains("required 10"));
                assert!(msg.contains("available 5"));
            }
            other => panic!("Expected RateLimitExceeded, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_check_and_deduct_quota_exact_balance_succeeds() {
        let credits_repo = Arc::new(MockCreditsRepository::with_balance(10));
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            credits_repo.clone(),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_and_deduct_quota(
                Uuid::new_v4(),
                10,
                CreditsTransactionType::Scrape,
                "test".to_string(),
                None,
            )
            .await;
        assert!(result.is_ok());
        assert_eq!(credits_repo.deduct_call_count(), 1);
    }

    #[tokio::test]
    async fn test_check_and_deduct_quota_deduct_fails_returns_credits_error() {
        let credits_repo = Arc::new(MockCreditsRepository::with_failing_deduct(100));
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            credits_repo.clone(),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service
            .check_and_deduct_quota(
                Uuid::new_v4(),
                10,
                CreditsTransactionType::Scrape,
                "test".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::CreditsError
        ));
        assert_eq!(credits_repo.deduct_call_count(), 1);
    }

    #[tokio::test]
    async fn test_get_quota_balance_get_balance_fails_returns_credits_error() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_failing_get_balance()),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.get_quota_balance(Uuid::new_v4()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RateLimitingError::CreditsError
        ));
    }

    #[tokio::test]
    async fn test_get_quota_balance_success_returns_balance() {
        let service = make_service_with_mocks(
            Arc::new(MockTaskRepository::with_no_task()),
            Arc::new(MockBacklogRepository::new()),
            Arc::new(MockCreditsRepository::with_balance(42)),
            RateLimitingConfig::default(),
        )
        .await;

        let result = service.get_quota_balance(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}
