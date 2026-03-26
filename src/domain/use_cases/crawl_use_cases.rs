// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl-related use cases

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::domain::models::{Crawl, CrawlStatus};
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use std::sync::Arc;
use uuid::Uuid;

/// 异步爬取请求
pub struct AsyncCrawlRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub request: CrawlRequestDto,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 异步爬取响应
pub struct AsyncCrawlResponse {
    pub crawl_id: Uuid,
    pub root_task_id: Uuid,
    pub estimated_pages: u32,
}

/// 同步爬取请求
pub struct SyncCrawlRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub request: CrawlRequestDto,
    pub timeout_ms: Option<u32>,
}

/// 同步爬取响应
pub struct SyncCrawlResponse {
    pub crawl_id: Uuid,
    pub tasks: Vec<Task>,
    pub total_pages: u32,
    pub completed_pages: u32,
    pub response_time_ms: u64,
}

/// 爬取状态查询请求
pub struct GetCrawlStatusRequest {
    pub team_id: Uuid,
    pub crawl_id: Uuid,
}

/// 爬取状态响应
pub struct GetCrawlStatusResponse {
    pub crawl: Option<Crawl>,
    pub total_tasks: u64,
    pub completed_tasks: u64,
    pub failed_tasks: u64,
    pub pending_tasks: u64,
    pub progress_percentage: f64,
}

/// 异步爬取用例
#[allow(dead_code)]
pub struct AsyncCrawlUseCase<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> AsyncCrawlUseCase<C, T, R> {
    pub fn new(
        crawl_repo: Arc<C>,
        task_repo: Arc<T>,
        credits_service: Arc<CreditsService<R>>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: AsyncCrawlRequest,
    ) -> Result<AsyncCrawlResponse, anyhow::Error> {
        // 创建爬取任务
        let crawl = Crawl {
            id: Uuid::new_v4(),
            team_id: request.team_id,
            name: request.request.name.unwrap_or_default(),
            root_url: request.request.url.clone(),
            url: request.request.url.clone(),
            status: CrawlStatus::Queued,
            config: serde_json::to_value(&request.request.config).unwrap_or_default(),
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };
        self.crawl_repo.create(&crawl).await?;

        // 创建根任务
        let payload = serde_json::json!({
            "crawl_id": crawl.id,
            "config": request.request.config,
        });
        let now = chrono::Utc::now();
        let root_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: request.team_id,
            api_key_id: request.api_key_id,
            url: request.request.url.clone(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl.id),
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };
        self.task_repo.create(&root_task).await?;

        // 计算预估页面数
        let config = &request.request.config;
        let max_depth = config
            .max_depth
            .min(crate::application::dto::crawl_request::MAX_CRAWL_DEPTH);
        let estimated_pages = calculate_estimated_pages(max_depth);

        Ok(AsyncCrawlResponse {
            crawl_id: crawl.id,
            root_task_id: root_task.id,
            estimated_pages,
        })
    }
}

/// 同步爬取用例
#[allow(dead_code)]
pub struct SyncCrawlUseCase<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> SyncCrawlUseCase<C, T, R> {
    pub fn new(
        crawl_repo: Arc<C>,
        task_repo: Arc<T>,
        credits_service: Arc<CreditsService<R>>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: SyncCrawlRequest,
    ) -> Result<SyncCrawlResponse, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // 创建爬取任务
        let crawl = Crawl {
            id: Uuid::new_v4(),
            team_id: request.team_id,
            name: request.request.name.unwrap_or_default(),
            root_url: request.request.url.clone(),
            url: request.request.url.clone(),
            status: CrawlStatus::Queued,
            config: serde_json::to_value(&request.request.config).unwrap_or_default(),
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };
        self.crawl_repo.create(&crawl).await?;

        // 创建根任务
        let payload = serde_json::json!({
            "crawl_id": crawl.id,
            "config": request.request.config,
        });
        let now = chrono::Utc::now();
        let root_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: request.team_id,
            api_key_id: request.api_key_id,
            url: request.request.url.clone(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl.id),
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };
        self.task_repo.create(&root_task).await?;

        let response_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(SyncCrawlResponse {
            crawl_id: crawl.id,
            tasks: vec![root_task],
            total_pages: 1,
            completed_pages: 0,
            response_time_ms,
        })
    }
}

/// 获取爬取状态用例
pub struct GetCrawlStatusUseCase<C: CrawlRepository, T: TaskRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
}

impl<C: CrawlRepository, T: TaskRepository> GetCrawlStatusUseCase<C, T> {
    pub fn new(crawl_repo: Arc<C>, task_repo: Arc<T>) -> Self {
        Self {
            crawl_repo,
            task_repo,
        }
    }

    pub async fn execute(
        &self,
        request: GetCrawlStatusRequest,
    ) -> Result<GetCrawlStatusResponse, anyhow::Error> {
        match self.crawl_repo.find_by_id(request.crawl_id).await? {
            Some(crawl) => {
                if crawl.team_id != request.team_id {
                    return Err(anyhow::anyhow!("Crawl not found"));
                }

                let tasks = self.task_repo.find_by_crawl_id(request.crawl_id).await?;
                let total_tasks = tasks.len() as u64;
                let completed_tasks = tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Completed)
                    .count() as u64;
                let failed_tasks = tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Failed)
                    .count() as u64;
                let pending_tasks = total_tasks - completed_tasks - failed_tasks;
                let progress_percentage = if total_tasks > 0 {
                    (completed_tasks as f64 / total_tasks as f64) * 100.0
                } else {
                    0.0
                };

                Ok(GetCrawlStatusResponse {
                    crawl: Some(crawl),
                    total_tasks,
                    completed_tasks,
                    failed_tasks,
                    pending_tasks,
                    progress_percentage,
                })
            }
            None => Ok(GetCrawlStatusResponse {
                crawl: None,
                total_tasks: 0,
                completed_tasks: 0,
                failed_tasks: 0,
                pending_tasks: 0,
                progress_percentage: 0.0,
            }),
        }
    }
}

/// 计算预估页面数量
fn calculate_estimated_pages(max_depth: u32) -> u32 {
    // 简单的估算：假设每个页面平均有 10 个链接，第一层有 1 个页面
    if max_depth == 0 {
        return 1;
    }
    let mut pages = 1;
    let mut current_level_pages = 1;
    for _ in 1..=max_depth {
        current_level_pages *= 10;
        pages += current_level_pages;
        if pages > 10000 {
            return 10000; // 限制最大预估页面数
        }
    }
    pages
}
