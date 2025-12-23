// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::{
    application::dto::crawl_request::CrawlRequestDto,
    domain::{
        models::{
            crawl::{Crawl, CrawlStatus},
            scrape_result::ScrapeResult,
            task::{Task, TaskStatus, TaskType},
        },
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
use chrono::{FixedOffset, Utc};
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use tracing::log::error;
use uuid::Uuid;
use validator::Validate;

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
///
/// # 类型参数
///
/// * `CR` - 爬取仓库类型，必须实现 `CrawlRepository`
/// * `TR` - 任务仓库类型，必须实现 `TaskRepository`
/// * `WR` - Webhook 仓库类型，必须实现 `WebhookRepository`
/// * `SRR` - 抓取结果仓库类型，必须实现 `ScrapeResultRepository`
/// * `GR` - 地理限制仓库类型，必须实现 `GeoRestrictionRepository`
pub struct CrawlUseCase<CR, TR, WR, SRR, GR> {
    /// 爬取任务仓库
    crawl_repo: Arc<CR>,
    /// 任务仓库
    task_repo: Arc<TR>,
    /// Webhook 仓库
    webhook_repo: Arc<WR>,
    /// 抓取结果仓库
    scrape_result_repo: Arc<SRR>,
    /// 地理限制仓库
    geo_restriction_repo: Arc<GR>,
    /// 团队服务
    team_service: Arc<TeamService>,
}

impl<CR, TR, WR, SRR, GR> CrawlUseCase<CR, TR, WR, SRR, GR>
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
    GR: GeoRestrictionRepository + 'static,
{
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
        crawl_repo: Arc<CR>,
        task_repo: Arc<TR>,
        webhook_repo: Arc<WR>,
        scrape_result_repo: Arc<SRR>,
        geo_restriction_repo: Arc<GR>,
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

    /// 获取 Webhook 仓库引用
    ///
    /// 允许未使用的 webhook_repo 直到完全集成
    #[allow(dead_code)]
    pub fn webhook_repo(&self) -> &Arc<WR> {
        &self.webhook_repo
    }

    /// 获取爬取任务的结果
    ///
    /// 根据爬取任务 ID 获取所有相关的抓取结果
    ///
    /// # 参数
    ///
    /// * `crawl_id` - 爬取任务 ID
    /// * `_team_id` - 团队 ID（当前未使用，用于权限验证）
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
    /// - 数据库查询失败（RepositoryError）
    pub async fn get_crawl_results(
        &self,
        crawl_id: Uuid,
        _team_id: Uuid,
    ) -> Result<Vec<ScrapeResult>, CrawlUseCaseError> {
        // 1. 检查爬取任务是否存在
        if self.crawl_repo.find_by_id(crawl_id).await?.is_none() {
            return Err(CrawlUseCaseError::NotFound);
        }

        // 2. 获取该爬取任务的所有子任务
        let tasks = self.task_repo.find_by_crawl_id(crawl_id).await?;

        // 3. 获取每个任务的抓取结果
        let mut results = Vec::new();
        for task in tasks {
            if let Some(result) = self.scrape_result_repo.find_by_task_id(task.id).await? {
                results.push(result);
            }
        }

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
        dto: CrawlRequestDto,
        client_ip: &str,
    ) -> Result<Crawl, CrawlUseCaseError> {
        // 1. 验证请求参数
        dto.validate()
            .map_err(|e| CrawlUseCaseError::ValidationError(e.to_string()))?;

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
        let crawl = Crawl {
            id: crawl_id,
            team_id,
            name: dto.name.unwrap_or_else(|| "Untitled Crawl".to_string()), // 默认名称
            root_url: dto.url.clone(),
            url: dto.url.clone(),
            status: CrawlStatus::Queued, // 初始状态为排队中
            config: json!(dto.config),   // 序列化配置为 JSON
            total_tasks: 1,              // 初始任务数 1
            completed_tasks: 0,          // 已完成任务数 0
            failed_tasks: 0,             // 失败任务数 0
            created_at: now,
            updated_at: now,
            completed_at: None, // 尚未完成
        };

        // 4. 保存爬取任务到数据库
        self.crawl_repo.create(&crawl).await?;

        // 5. 创建初始任务
        let initial_task = Task {
            id: Uuid::new_v4(),         // 生成任务 ID
            task_type: TaskType::Crawl, // 任务类型为爬取
            status: TaskStatus::Queued, // 任务状态为排队中
            priority: 100,              // 默认优先级 100
            team_id,
            url: dto.url, // 爬取目标 URL
            payload: json!({ "crawl_id": crawl_id, "depth": 0, "config": dto.config }), // 任务载荷
            attempt_count: 0, // 尝试次数 0
            max_retries: 3, // 最大重试次数 3
            scheduled_at: None, // 尚未调度
            created_at: now.into(),
            started_at: None,         // 尚未开始
            completed_at: None,       // 尚未完成
            crawl_id: Some(crawl_id), // 关联的爬取任务 ID
            updated_at: now.into(),
            lock_token: None,      // 尚未加锁
            lock_expires_at: None, // 锁未过期
            expires_at: dto
                .expires_at
                .map(|dt| dt.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())), // 任务过期时间
        };

        // 6. 保存初始任务到数据库
        self.task_repo.create(&initial_task).await?;

        // 7. 返回创建的爬取任务
        Ok(crawl)
    }

    /// 根据 ID 获取爬取任务
    ///
    /// # 参数
    ///
    /// * `crawl_id` - 爬取任务 ID
    ///
    /// # 返回值
    ///
    /// * `Ok(Some(Crawl))` - 找到爬取任务
    /// * `Ok(None)` - 未找到爬取任务
    /// * `Err(CrawlUseCaseError)` - 数据库查询失败
    pub async fn get_crawl(&self, crawl_id: Uuid) -> Result<Option<Crawl>, CrawlUseCaseError> {
        self.crawl_repo
            .find_by_id(crawl_id)
            .await
            .map_err(Into::into)
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
