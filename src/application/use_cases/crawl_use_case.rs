// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::{
    application::dto::crawl_request::CrawlRequestDto,
    domain::{
        models::{scrape_result::ScrapeResult, Crawl, CrawlStatus, Task, TaskStatus, TaskType},
        repositories::{
            crawl_repository::CrawlRepository,
            geo_restriction_repository::GeoRestrictionRepository,
            scrape_result_repository::ScrapeResultRepository,
            task_repository::{RepositoryError, TaskRepository},
            webhook_repository::WebhookRepository,
        },
        services::team_service::TeamService,
    },
};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use log::error;
use uuid::Uuid;

/// 爬取用例错误类型
///
/// 定义爬取用例中可能发生的各种错误情况
#[derive(Error, Debug)]
pub enum CrawlUseCaseError {
    /// 验证失败错误
    #[error("Validation failed: {0}")]
    ValidationError(String),

    /// 仓库层错误
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),

    /// 爬取任务未找到错误
    #[error("Crawl not found")]
    NotFound,

    /// 通用错误
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

/// 爬取用例
///
/// 处理爬取任务的核心业务逻辑，包括创建、查询、取消等操作
#[allow(dead_code)]
pub struct CrawlUseCase {
    /// 爬取任务仓库
    crawl_repo: Arc<dyn CrawlRepository>,
    /// 任务仓库
    task_repo: Arc<dyn TaskRepository>,
    /// Webhook 仓库
    webhook_repo: Arc<dyn WebhookRepository>,
    /// 抓取结果仓库
    scrape_result_repo: Arc<dyn ScrapeResultRepository>,
    /// 地理限制仓库
    geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
    /// 团队服务
    team_service: Arc<TeamService>,
}

impl CrawlUseCase {
    /// 创建新的爬取用例实例
    ///
    /// # 参数
    ///
    /// * `crawl_repo` - 爬取任务仓库
    /// * `task_repo` - 任务仓库
    /// * `webhook_repo` - Webhook 仓库
    /// * `scrape_result_repo` - 抓取结果仓库
    /// * `geo_restriction_repo` - 地理限制仓库
    /// * `team_service` - 团队服务
    ///
    /// # 返回值
    ///
    /// 返回新的 `CrawlUseCase` 实例
    pub fn new(
        crawl_repo: Arc<dyn CrawlRepository>,
        task_repo: Arc<dyn TaskRepository>,
        webhook_repo: Arc<dyn WebhookRepository>,
        scrape_result_repo: Arc<dyn ScrapeResultRepository>,
        geo_restriction_repo: Arc<dyn GeoRestrictionRepository>,
        team_service: Arc<TeamService>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            webhook_repo,
            scrape_result_repo,
            geo_restriction_repo,
            team_service,
        }
    }

    /// 获取爬取任务的结果
    ///
    /// 根据爬取任务 ID 获取所有相关的抓取结果
    ///
    /// # 参数
    ///
    /// * `crawl_id` - 爬取任务 ID
    /// * `team_id` - 团队 ID，用于权限验证
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<ScrapeResult>)` - 成功时返回抓取结果列表
    /// * `Err(CrawlUseCaseError)` - 失败时返回错误
    ///
    /// # 错误
    ///
    /// 可能在以下情况下返回错误：
    /// - 爬取任务不存在（NotFound）
    /// - 爬取任务不属于指定团队（NotFound）
    /// - 数据库查询失败（RepositoryError）
    ///
    /// # 安全性
    ///
    /// 此方法验证爬取任务是否属于请求的团队，防止未授权访问
    pub async fn get_crawl_results(
        &self,
        crawl_id: Uuid,
        team_id: Uuid,
    ) -> Result<Vec<ScrapeResult>, CrawlUseCaseError> {
        // 首先验证爬取任务是否存在且属于该团队
        match self.crawl_repo.find_by_id(crawl_id).await? {
            Some(crawl) => {
                // 验证资源所有权
                if crawl.team_id != team_id {
                    return Err(CrawlUseCaseError::NotFound);
                }
            }
            None => return Err(CrawlUseCaseError::NotFound),
        }

        let tasks = self.task_repo.find_by_crawl_id(crawl_id).await?;

        let task_ids: Vec<Uuid> = tasks.iter().map(|t| t.id).collect();

        let results = self.scrape_result_repo.find_by_task_ids(&task_ids).await?;

        Ok(results)
    }

    /// 创建新的爬取任务
    ///
    /// 验证请求参数并检查地理限制，然后创建新的爬取任务记录
    ///
    /// # 参数
    ///
    /// * `team_id` - 团队 ID
    /// * `dto` - 爬取请求数据传输对象
    /// * `client_ip` - 客户端 IP 地址（用于地理限制验证）
    ///
    /// # 返回值
    ///
    /// * `Ok(Crawl)` - 成功时返回创建的爬取任务
    /// * `Err(CrawlUseCaseError)` - 失败时返回错误
    ///
    /// # 错误
    ///
    /// 可能在以下情况下返回错误：
    /// - 请求参数验证失败（ValidationError）
    /// - 地理限制验证失败（ValidationError）
    /// - 数据库操作失败（RepositoryError）
    pub async fn create_crawl(
        &self,
        team_id: Uuid,
        api_key_id: Uuid,
        dto: CrawlRequestDto,
        client_ip: &str,
    ) -> Result<Crawl, CrawlUseCaseError> {
        // 1. 验证请求参数 (URL 验证在 handler 中进行)

        // 简化 config 验证
        if dto.config.max_depth > 5 {
            return Err(CrawlUseCaseError::ValidationError(
                "max_depth must be between 0 and 5".to_string(),
            ));
        }
        if let Some(concurrency) = dto.config.max_concurrency {
            if concurrency > 100 {
                return Err(CrawlUseCaseError::ValidationError(
                    "max_concurrency must be between 1 and 100".to_string(),
                ));
            }
        }

        // 2. 检查地理限制
        let restrictions = self
            .geo_restriction_repo
            .get_team_restrictions(team_id)
            .await
            .map_err(|e| {
                CrawlUseCaseError::Anyhow(anyhow::anyhow!("Failed to get team restrictions: {}", e))
            })?;

        match self
            .team_service
            .validate_geographic_restriction(team_id, client_ip, &restrictions)
            .await
        {
            Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed) => {
                // 记录允许的访问日志
                if let Err(e) = self
                    .geo_restriction_repo
                    .log_geo_restriction_action(
                        team_id,
                        client_ip,
                        "",
                        "ALLOWED",
                        "Geographic restriction check passed",
                    )
                    .await
                {
                    error!("Failed to log geographic restriction action: {}", e);
                }
            }
            Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
                // 记录拒绝的访问日志
                if let Err(e) = self
                    .geo_restriction_repo
                    .log_geo_restriction_action(team_id, client_ip, "", "DENIED", &reason)
                    .await
                {
                    error!("Failed to log geographic restriction action: {}", e);
                }
                return Err(CrawlUseCaseError::ValidationError(format!(
                    "Geographic restriction check failed: {}",
                    reason
                )));
            }
            Err(e) => {
                error!("Geographic restriction validation error: {}", e);
                return Err(CrawlUseCaseError::Anyhow(anyhow::anyhow!(
                    "Geographic restriction validation failed: {}",
                    e
                )));
            }
        }

        // 3. 生成新的爬取任务 ID
        let crawl_id = Uuid::new_v4();
        let now = Utc::now();

        // 3. 创建爬取任务实体
        let url = dto.url.clone();
        let crawl = Crawl::with_all_fields(
            crawl_id,
            team_id,
            dto.name.unwrap_or_else(|| "Untitled Crawl".to_string()),
            url.clone(),
            url,
            CrawlStatus::Queued,
            json!(dto.config),
            1, // total_tasks
            0, // completed_tasks
            0, // failed_tasks
            now,
            now,
            None,
        );

        // 4. 保存爬取任务到数据库
        self.crawl_repo.create(&crawl).await?;

        // 5. 创建初始任务
        let initial_task = Task {
            id: Uuid::new_v4(),         // 生成任务 ID
            task_type: TaskType::Crawl, // 任务类型为爬取
            status: TaskStatus::Queued, // 任务状态为排队中
            priority: 100,              // 默认优先级 100
            team_id,
            api_key_id,   // API密钥ID
            url: dto.url, // 爬取目标 URL
            payload: json!({
                            "crawl_id": crawl_id,
                            "depth": 0,
                            "config": dto.config,
                            "domain_blacklist": restrictions.domain_blacklist
            }),
            retry_count: 0,     // 重试次数 0
            attempt_count: 0,   // 尝试次数 0
            max_retries: 3,     // 最大重试次数 3
            scheduled_at: None, // 尚未调度
            created_at: now,
            started_at: None,         // 尚未开始
            completed_at: None,       // 尚未完成
            crawl_id: Some(crawl_id), // 关联的爬取任务 ID
            updated_at: now,
            lock_token: None,           // 尚未加锁
            lock_expires_at: None,      // 锁未过期
            expires_at: dto.expires_at, // 任务过期时间
        };

        // 6. 保存初始任务到数据库
        self.task_repo.create(&initial_task).await?;

        // 7. 返回创建的爬取任务
        Ok(crawl)
    }

    /// 获取爬取任务详情
    ///
    /// # 参数
    ///
    /// * `crawl_id` - 爬取任务 ID
    /// * `team_id` - 团队 ID，用于权限验证
    ///
    /// # 返回值
    ///
    /// * `Ok(Some(Crawl))` - 找到爬取任务且属于该团队
    /// * `Ok(None)` - 未找到爬取任务
    /// * `Err(CrawlUseCaseError)` - 数据库查询失败
    ///
    /// # 安全性
    ///
    /// 此方法验证爬取任务是否属于请求的团队，防止未授权访问
    pub async fn get_crawl(
        &self,
        crawl_id: Uuid,
        team_id: Uuid,
    ) -> Result<Option<Crawl>, CrawlUseCaseError> {
        match self.crawl_repo.find_by_id(crawl_id).await? {
            Some(crawl) => {
                // 验证资源所有权
                if crawl.team_id != team_id {
                    return Ok(None); // 返回 None 而非错误，避免信息泄露
                }
                Ok(Some(crawl))
            }
            None => Ok(None),
        }
    }

    /// 取消爬取任务
    ///
    /// 将爬取任务状态设置为已取消，仅当任务尚未完成时有效
    ///
    /// # 参数
    ///
    /// * `id` - 爬取任务 ID
    /// * `team_id` - 团队 ID，用于权限验证
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功取消或任务已完成
    /// * `Err(CrawlUseCaseError)` - 失败时返回错误
    ///
    /// # 错误
    ///
    /// 可能在以下情况下返回错误：
    /// - 爬取任务不存在（NotFound）
    /// - 爬取任务不属于指定团队（NotFound）
    /// - 数据库更新失败（RepositoryError）
    pub async fn cancel_crawl(&self, id: Uuid, team_id: Uuid) -> Result<(), CrawlUseCaseError> {
        let crawl = self.crawl_repo.find_by_id(id).await?;

        match crawl {
            Some(mut c) => {
                // 确保爬取任务属于指定团队
                if c.team_id != team_id {
                    return Err(CrawlUseCaseError::NotFound); // 或返回 Forbidden
                }

                // 如果任务已完成、失败或已取消，则无需操作
                if c.status == CrawlStatus::Completed
                    || c.status == CrawlStatus::Failed
                    || c.status == CrawlStatus::Cancelled
                {
                    return Ok(()); // 任务已结束
                }

                // 更新任务状态为已取消
                c.status = CrawlStatus::Cancelled;
                c.updated_at = Utc::now();
                self.crawl_repo.update(&c).await?;

                // 取消所有关联的任务
                self.task_repo.cancel_tasks_by_crawl_id(id).await?;

                Ok(())
            }
            None => Err(CrawlUseCaseError::NotFound), // 爬取任务不存在
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::scrape_result::ScrapeResult;
    use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError;
    use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
    use crate::domain::services::team_service::TeamGeoRestrictions;
    use async_trait::async_trait;
    use std::net::IpAddr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // ============ MockCrawlRepository ============

    struct MockCrawlRepository {
        stored_crawl: Mutex<Option<Crawl>>,
        updated_crawls: Mutex<Vec<Crawl>>,
        created_count: AtomicU32,
        should_fail_find: bool,
        should_fail_create: bool,
        should_fail_update: bool,
    }

    impl MockCrawlRepository {
        fn with_crawl(crawl: Crawl) -> Self {
            Self {
                stored_crawl: Mutex::new(Some(crawl)),
                updated_crawls: Mutex::new(vec![]),
                created_count: AtomicU32::new(0),
                should_fail_find: false,
                should_fail_create: false,
                should_fail_update: false,
            }
        }

        fn empty() -> Self {
            Self {
                stored_crawl: Mutex::new(None),
                updated_crawls: Mutex::new(vec![]),
                created_count: AtomicU32::new(0),
                should_fail_find: false,
                should_fail_create: false,
                should_fail_update: false,
            }
        }

        fn failing_find() -> Self {
            Self {
                stored_crawl: Mutex::new(None),
                updated_crawls: Mutex::new(vec![]),
                created_count: AtomicU32::new(0),
                should_fail_find: true,
                should_fail_create: false,
                should_fail_update: false,
            }
        }

        fn failing_create() -> Self {
            let mut m = Self::empty();
            m.should_fail_create = true;
            m
        }

        fn failing_update(crawl: Crawl) -> Self {
            Self {
                stored_crawl: Mutex::new(Some(crawl)),
                updated_crawls: Mutex::new(vec![]),
                created_count: AtomicU32::new(0),
                should_fail_find: false,
                should_fail_create: false,
                should_fail_update: true,
            }
        }

        fn last_updated_status(&self) -> Option<CrawlStatus> {
            self.updated_crawls
                .lock()
                .unwrap()
                .last()
                .map(|c| c.status)
        }
    }

    #[async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            if self.should_fail_create {
                return Err(RepositoryError::Database(anyhow::anyhow!("crawl repo create down")));
            }
            self.created_count.fetch_add(1, Ordering::SeqCst);
            Ok(crawl.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            if self.should_fail_find {
                return Err(RepositoryError::Database(anyhow::anyhow!("crawl repo find down")));
            }
            Ok(self.stored_crawl.lock().unwrap().clone())
        }

        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            if self.should_fail_update {
                return Err(RepositoryError::Database(anyhow::anyhow!("crawl repo update down")));
            }
            self.updated_crawls.lock().unwrap().push(crawl.clone());
            Ok(crawl.clone())
        }

        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn update_status(&self, _id: Uuid, _status: CrawlStatus) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }

        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    // ============ MockTaskRepository ============

    struct MockTaskRepository {
        created_count: AtomicU32,
        should_fail_create: bool,
        should_fail_find_by_crawl: bool,
        should_fail_cancel: bool,
        stored_tasks: Mutex<Vec<Task>>,
    }

    impl MockTaskRepository {
        fn with_tasks(tasks: Vec<Task>) -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_create: false,
                should_fail_find_by_crawl: false,
                should_fail_cancel: false,
                stored_tasks: Mutex::new(tasks),
            }
        }

        fn empty() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_create: false,
                should_fail_find_by_crawl: false,
                should_fail_cancel: false,
                stored_tasks: Mutex::new(vec![]),
            }
        }

        fn failing_create() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_create: true,
                should_fail_find_by_crawl: false,
                should_fail_cancel: false,
                stored_tasks: Mutex::new(vec![]),
            }
        }

        fn failing_cancel() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_create: false,
                should_fail_find_by_crawl: false,
                should_fail_cancel: true,
                stored_tasks: Mutex::new(vec![]),
            }
        }

        fn failing_find_by_crawl() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_create: false,
                should_fail_find_by_crawl: true,
                should_fail_cancel: false,
                stored_tasks: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            if self.should_fail_create {
                return Err(RepositoryError::Database(anyhow::anyhow!("task repo create down")));
            }
            self.created_count.fetch_add(1, Ordering::SeqCst);
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
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
        ) -> Result<std::collections::HashSet<String>, RepositoryError> {
            Ok(std::collections::HashSet::new())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            if self.should_fail_cancel {
                return Err(RepositoryError::Database(anyhow::anyhow!("cancel tasks down")));
            }
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            if self.should_fail_find_by_crawl {
                return Err(RepositoryError::Database(anyhow::anyhow!("find_by_crawl_id down")));
            }
            Ok(self.stored_tasks.lock().unwrap().clone())
        }

        async fn query_tasks(
            &self,
            _params: crate::domain::repositories::task_repository::TaskQueryParams,
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

    // ============ MockWebhookRepository ============

    struct MockWebhookRepository;

    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &crate::domain::models::Webhook) -> Result<crate::domain::models::Webhook, RepositoryError> {
            Ok(webhook.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<crate::domain::models::Webhook>, RepositoryError> {
            Ok(None)
        }

        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<crate::domain::models::Webhook>, RepositoryError> {
            Ok(vec![])
        }
    }

    // ============ MockScrapeResultRepository ============

    struct MockScrapeResultRepository {
        stored_results: Mutex<Vec<ScrapeResult>>,
        should_fail: bool,
    }

    impl MockScrapeResultRepository {
        fn with_results(results: Vec<ScrapeResult>) -> Self {
            Self {
                stored_results: Mutex::new(results),
                should_fail: false,
            }
        }

        fn empty() -> Self {
            Self {
                stored_results: Mutex::new(vec![]),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                stored_results: Mutex::new(vec![]),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }

        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            if self.should_fail {
                return Err(anyhow::anyhow!("scrape result repo down"));
            }
            Ok(self.stored_results.lock().unwrap().clone())
        }

        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

    // ============ MockGeoRestrictionRepository ============

    struct MockGeoRestrictionRepository {
        restrictions: Mutex<TeamGeoRestrictions>,
        get_should_fail: bool,
        log_should_fail: bool,
        log_count: AtomicU32,
    }

    impl MockGeoRestrictionRepository {
        fn with_restrictions(restrictions: TeamGeoRestrictions) -> Self {
            Self {
                restrictions: Mutex::new(restrictions),
                get_should_fail: false,
                log_should_fail: false,
                log_count: AtomicU32::new(0),
            }
        }

        fn failing_get() -> Self {
            Self {
                restrictions: Mutex::new(TeamGeoRestrictions::default()),
                get_should_fail: true,
                log_should_fail: false,
                log_count: AtomicU32::new(0),
            }
        }

        fn with_failing_log(restrictions: TeamGeoRestrictions) -> Self {
            Self {
                restrictions: Mutex::new(restrictions),
                get_should_fail: false,
                log_should_fail: true,
                log_count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
            if self.get_should_fail {
                return Err(GeoRestrictionRepositoryError::Database(
                    "geo restriction repo down".to_string(),
                ));
            }
            Ok(self.restrictions.lock().unwrap().clone())
        }

        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }

        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            self.log_count.fetch_add(1, Ordering::SeqCst);
            if self.log_should_fail {
                return Err(GeoRestrictionRepositoryError::Other(
                    "log action down".to_string(),
                ));
            }
            Ok(())
        }
    }

    // ============ MockGeoLocationService ============

    struct MockGeoLocationService {
        should_fail: bool,
    }

    impl MockGeoLocationService {
        fn succeeding() -> Self {
            Self { should_fail: false }
        }

        fn failing() -> Self {
            Self { should_fail: true }
        }
    }

    #[async_trait]
    impl GeoLocationService for MockGeoLocationService {
        async fn get_location(&self, _ip: &IpAddr) -> anyhow::Result<GeoLocation> {
            if self.should_fail {
                return Err(anyhow::anyhow!("geolocation service down"));
            }
            Ok(GeoLocation {
                country_code: "US".to_string(),
                ..GeoLocation::default()
            })
        }
    }

    // ============ Helpers ============

    fn make_crawl(id: Uuid, team_id: Uuid, status: CrawlStatus) -> Crawl {
        Crawl::with_all_fields(
            id,
            team_id,
            "Test Crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            status,
            json!({}),
            1,
            0,
            0,
            Utc::now(),
            Utc::now(),
            None,
        )
    }

    fn make_crawl_dto() -> CrawlRequestDto {
        CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: Some("Test Crawl".to_string()),
            config: crate::application::dto::crawl_request::CrawlConfigDto {
                max_depth: 3,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: None,
            expires_at: None,
        }
    }

    fn make_scrape_result(task_id: Uuid) -> ScrapeResult {
        ScrapeResult {
            id: Uuid::new_v4(),
            task_id,
            url: "https://example.com".to_string(),
            status_code: 200,
            content: "<html></html>".to_string(),
            content_type: "text/html".to_string(),
            headers: json!({}),
            meta_data: json!({}),
            screenshot: None,
            response_time_ms: 100,
            created_at: Utc::now().naive_utc(),
        }
    }

    /// Build a CrawlUseCase with configurable mocks.
    fn build_use_case(
        crawl_repo: Arc<MockCrawlRepository>,
        task_repo: Arc<MockTaskRepository>,
        scrape_result_repo: Arc<MockScrapeResultRepository>,
        geo_repo: Arc<MockGeoRestrictionRepository>,
        geo_loc_service: Arc<MockGeoLocationService>,
    ) -> CrawlUseCase {
        let team_service = Arc::new(TeamService::new(
            geo_loc_service,
            Arc::new(MockGeoRestrictionRepository::with_restrictions(
                TeamGeoRestrictions::default(),
            )),
        ));
        CrawlUseCase::new(
            crawl_repo,
            task_repo,
            Arc::new(MockWebhookRepository),
            scrape_result_repo,
            geo_repo,
            team_service,
        )
    }

    /// Convenience: default mocks where geo restrictions are disabled (Allowed).
    fn build_use_case_allowed_geo(
        crawl_repo: Arc<MockCrawlRepository>,
        task_repo: Arc<MockTaskRepository>,
        scrape_result_repo: Arc<MockScrapeResultRepository>,
    ) -> CrawlUseCase {
        build_use_case(
            crawl_repo,
            task_repo,
            scrape_result_repo,
            Arc::new(MockGeoRestrictionRepository::with_restrictions(
                TeamGeoRestrictions::default(),
            )),
            Arc::new(MockGeoLocationService::succeeding()),
        )
    }

    // ============ new ============

    #[test]
    fn test_new_constructs_without_calling_repos() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );
        // Should construct without panicking
        let _ = &use_case;
    }

    // ============ get_crawl_results ============

    #[tokio::test]
    async fn test_get_crawl_results_success() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Completed);
        let task = Task::new(task_id, TaskType::Crawl, team_id, Uuid::nil(), "https://example.com".to_string(), json!({}));
        let result = make_scrape_result(task_id);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::with_tasks(vec![task])),
            Arc::new(MockScrapeResultRepository::with_results(vec![result])),
        );

        let results = use_case
            .get_crawl_results(crawl_id, team_id)
            .await
            .expect("should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, task_id);
    }

    #[tokio::test]
    async fn test_get_crawl_results_crawl_not_found() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.get_crawl_results(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(matches!(result, Err(CrawlUseCaseError::NotFound)));
    }

    #[tokio::test]
    async fn test_get_crawl_results_wrong_team_returns_not_found() {
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, Uuid::new_v4(), CrawlStatus::Completed);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.get_crawl_results(crawl_id, Uuid::new_v4()).await;
        assert!(
            matches!(result, Err(CrawlUseCaseError::NotFound)),
            "wrong team should return NotFound"
        );
    }

    #[tokio::test]
    async fn test_get_crawl_results_crawl_repo_error_propagates() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::failing_find()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.get_crawl_results(Uuid::new_v4(), Uuid::new_v4()).await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("find down"));
    }

    #[tokio::test]
    async fn test_get_crawl_results_task_repo_error_propagates() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Completed);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::failing_find_by_crawl()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.get_crawl_results(crawl_id, team_id).await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("find_by_crawl_id down"));
    }

    #[tokio::test]
    async fn test_get_crawl_results_scrape_result_repo_error_propagates() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Completed);
        let task = Task::new(task_id, TaskType::Crawl, team_id, Uuid::nil(), "https://example.com".to_string(), json!({}));

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::with_tasks(vec![task])),
            Arc::new(MockScrapeResultRepository::failing()),
        );

        let result = use_case.get_crawl_results(crawl_id, team_id).await;
        assert!(
            matches!(result, Err(CrawlUseCaseError::Anyhow(_))),
            "scrape result repo error should map to Anyhow"
        );
    }

    #[tokio::test]
    async fn test_get_crawl_results_empty_results_when_no_tasks() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Completed);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::with_tasks(vec![])),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let results = use_case
            .get_crawl_results(crawl_id, team_id)
            .await
            .expect("should succeed with empty results");
        assert!(results.is_empty());
    }

    // ============ create_crawl ============

    #[tokio::test]
    async fn test_create_crawl_success() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let dto = make_crawl_dto();

        let crawl_repo = Arc::new(MockCrawlRepository::empty());
        let task_repo = Arc::new(MockTaskRepository::empty());
        let use_case = build_use_case_allowed_geo(
            crawl_repo.clone(),
            task_repo.clone(),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let crawl = use_case
            .create_crawl(team_id, api_key_id, dto, "1.2.3.4")
            .await
            .expect("create_crawl should succeed");

        assert_eq!(crawl.team_id, team_id);
        assert_eq!(crawl.status, CrawlStatus::Queued);
        assert_eq!(crawl.name, "Test Crawl");
        assert_eq!(crawl.root_url, "https://example.com");
        assert_eq!(crawl_repo.created_count.load(Ordering::SeqCst), 1);
        assert_eq!(task_repo.created_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_create_crawl_name_none_uses_default() {
        let team_id = Uuid::new_v4();
        let mut dto = make_crawl_dto();
        dto.name = None;

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let crawl = use_case
            .create_crawl(team_id, Uuid::new_v4(), dto, "1.2.3.4")
            .await
            .expect("should succeed");
        assert_eq!(crawl.name, "Untitled Crawl");
    }

    #[tokio::test]
    async fn test_create_crawl_max_depth_exceeds_limit() {
        let mut dto = make_crawl_dto();
        dto.config.max_depth = 6;

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.create_crawl(Uuid::new_v4(), Uuid::new_v4(), dto, "1.2.3.4").await;
        let err = match result {
            Err(CrawlUseCaseError::ValidationError(msg)) => msg,
            e => panic!("expected ValidationError, got: {:?}", e),
        };
        assert!(err.contains("max_depth"), "got: {}", err);
    }

    #[tokio::test]
    async fn test_create_crawl_max_depth_at_boundary_5_succeeds() {
        let mut dto = make_crawl_dto();
        dto.config.max_depth = 5;

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.create_crawl(Uuid::new_v4(), Uuid::new_v4(), dto, "1.2.3.4").await;
        assert!(result.is_ok(), "max_depth=5 should be allowed");
    }

    #[tokio::test]
    async fn test_create_crawl_max_concurrency_exceeds_limit() {
        let mut dto = make_crawl_dto();
        dto.config.max_concurrency = Some(101);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.create_crawl(Uuid::new_v4(), Uuid::new_v4(), dto, "1.2.3.4").await;
        let err = match result {
            Err(CrawlUseCaseError::ValidationError(msg)) => msg,
            e => panic!("expected ValidationError, got: {:?}", e),
        };
        assert!(err.contains("max_concurrency"), "got: {}", err);
    }

    #[tokio::test]
    async fn test_create_crawl_max_concurrency_at_boundary_100_succeeds() {
        let mut dto = make_crawl_dto();
        dto.config.max_concurrency = Some(100);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.create_crawl(Uuid::new_v4(), Uuid::new_v4(), dto, "1.2.3.4").await;
        assert!(result.is_ok(), "max_concurrency=100 should be allowed");
    }

    #[tokio::test]
    async fn test_create_crawl_geo_denied_invalid_ip() {
        // enable_geo_restrictions=true + invalid IP → Denied without calling geolocation
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            ..Default::default()
        };

        let use_case = build_use_case(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
            Arc::new(MockGeoRestrictionRepository::with_restrictions(restrictions)),
            Arc::new(MockGeoLocationService::succeeding()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "not-an-ip")
            .await;
        let err = match result {
            Err(CrawlUseCaseError::ValidationError(msg)) => msg,
            e => panic!("expected ValidationError, got: {:?}", e),
        };
        assert!(
            err.contains("Geographic restriction check failed"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_crawl_geo_validation_error() {
        // enable_geo_restrictions=true + valid IP + geolocation fails → Err
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            ..Default::default()
        };

        let use_case = build_use_case(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
            Arc::new(MockGeoRestrictionRepository::with_restrictions(restrictions)),
            Arc::new(MockGeoLocationService::failing()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "8.8.8.8")
            .await;
        let err = match result {
            Err(CrawlUseCaseError::Anyhow(msg)) => msg.to_string(),
            e => panic!("expected Anyhow error, got: {:?}", e),
        };
        assert!(
            err.contains("Geographic restriction validation failed"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_crawl_geo_repo_get_error() {
        let use_case = build_use_case(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
            Arc::new(MockGeoRestrictionRepository::failing_get()),
            Arc::new(MockGeoLocationService::succeeding()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "1.2.3.4")
            .await;
        let err = match result {
            Err(CrawlUseCaseError::Anyhow(msg)) => msg.to_string(),
            e => panic!("expected Anyhow error, got: {:?}", e),
        };
        assert!(
            err.contains("Failed to get team restrictions"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_crawl_log_failure_does_not_block_allowed() {
        // Geo is allowed, but log_geo_restriction_action fails — flow should still succeed
        // (the error is only logged, not propagated).
        let restrictions = TeamGeoRestrictions::default(); // enable=false → Allowed
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_failing_log(restrictions));

        let use_case = build_use_case(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
            geo_repo,
            Arc::new(MockGeoLocationService::succeeding()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "1.2.3.4")
            .await;
        assert!(result.is_ok(), "log failure should not block the allowed path");
    }

    #[tokio::test]
    async fn test_create_crawl_crawl_repo_create_error() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::failing_create()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "1.2.3.4")
            .await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("create down"));
    }

    #[tokio::test]
    async fn test_create_crawl_task_repo_create_error() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::failing_create()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case
            .create_crawl(Uuid::new_v4(), Uuid::new_v4(), make_crawl_dto(), "1.2.3.4")
            .await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("create down"));
    }

    // ============ get_crawl ============

    #[tokio::test]
    async fn test_get_crawl_found() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Queued);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case
            .get_crawl(crawl_id, team_id)
            .await
            .expect("should succeed");
        let found = result.expect("should find crawl");
        assert_eq!(found.id, crawl_id);
        assert_eq!(found.team_id, team_id);
    }

    #[tokio::test]
    async fn test_get_crawl_not_found_returns_none() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case
            .get_crawl(Uuid::new_v4(), Uuid::new_v4())
            .await
            .expect("should succeed");
        assert!(result.is_none(), "non-existent crawl should return None");
    }

    #[tokio::test]
    async fn test_get_crawl_wrong_team_returns_none() {
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, Uuid::new_v4(), CrawlStatus::Queued);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case
            .get_crawl(crawl_id, Uuid::new_v4())
            .await
            .expect("should succeed");
        assert!(
            result.is_none(),
            "wrong team should return None to avoid info leak"
        );
    }

    #[tokio::test]
    async fn test_get_crawl_repo_error_propagates() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::failing_find()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.get_crawl(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(
            matches!(result, Err(CrawlUseCaseError::Repository(_))),
            "repo error should propagate"
        );
    }

    // ============ cancel_crawl ============

    #[tokio::test]
    async fn test_cancel_crawl_success() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Queued);
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));
        let task_repo = Arc::new(MockTaskRepository::empty());

        let use_case = build_use_case_allowed_geo(
            crawl_repo.clone(),
            task_repo.clone(),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        use_case
            .cancel_crawl(crawl_id, team_id)
            .await
            .expect("should succeed");

        assert_eq!(
            crawl_repo.last_updated_status(),
            Some(CrawlStatus::Cancelled),
            "crawl status should be updated to Cancelled"
        );
    }

    #[tokio::test]
    async fn test_cancel_crawl_not_found() {
        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::empty()),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.cancel_crawl(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(matches!(result, Err(CrawlUseCaseError::NotFound)));
    }

    #[tokio::test]
    async fn test_cancel_crawl_wrong_team_returns_not_found() {
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, Uuid::new_v4(), CrawlStatus::Queued);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.cancel_crawl(crawl_id, Uuid::new_v4()).await;
        assert!(matches!(result, Err(CrawlUseCaseError::NotFound)));
    }

    #[tokio::test]
    async fn test_cancel_crawl_already_completed_is_noop() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Completed);
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));

        let use_case = build_use_case_allowed_geo(
            crawl_repo.clone(),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        use_case
            .cancel_crawl(crawl_id, team_id)
            .await
            .expect("should succeed as no-op");
        assert!(
            crawl_repo.last_updated_status().is_none(),
            "completed crawl should not be updated"
        );
    }

    #[tokio::test]
    async fn test_cancel_crawl_already_failed_is_noop() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Failed);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        use_case
            .cancel_crawl(crawl_id, team_id)
            .await
            .expect("should succeed as no-op");
    }

    #[tokio::test]
    async fn test_cancel_crawl_already_cancelled_is_noop() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Cancelled);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        use_case
            .cancel_crawl(crawl_id, team_id)
            .await
            .expect("should succeed as no-op");
    }

    #[tokio::test]
    async fn test_cancel_crawl_processing_status_gets_cancelled() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Processing);
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));

        let use_case = build_use_case_allowed_geo(
            crawl_repo.clone(),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        use_case
            .cancel_crawl(crawl_id, team_id)
            .await
            .expect("should succeed");
        assert_eq!(
            crawl_repo.last_updated_status(),
            Some(CrawlStatus::Cancelled)
        );
    }

    #[tokio::test]
    async fn test_cancel_crawl_update_error_propagates() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Queued);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::failing_update(crawl)),
            Arc::new(MockTaskRepository::empty()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.cancel_crawl(crawl_id, team_id).await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("update down"));
    }

    #[tokio::test]
    async fn test_cancel_crawl_cancel_tasks_error_propagates() {
        let team_id = Uuid::new_v4();
        let crawl_id = Uuid::new_v4();
        let crawl = make_crawl(crawl_id, team_id, CrawlStatus::Queued);

        let use_case = build_use_case_allowed_geo(
            Arc::new(MockCrawlRepository::with_crawl(crawl)),
            Arc::new(MockTaskRepository::failing_cancel()),
            Arc::new(MockScrapeResultRepository::empty()),
        );

        let result = use_case.cancel_crawl(crawl_id, team_id).await;
        let err = match result {
            Err(CrawlUseCaseError::Repository(e)) => e,
            e => panic!("expected Repository error, got: {:?}", e),
        };
        assert!(err.to_string().contains("cancel tasks down"));
    }
}
