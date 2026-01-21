// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape-related use cases

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::dto::scrape_response::ScrapeResponseDto;
use crate::domain::models::task::{Task, TaskType};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use std::sync::Arc;
use uuid::Uuid;

/// 异步抓取请求
pub struct AsyncScrapeRequest {
    pub team_id: Uuid,
    pub request: ScrapeRequestDto,
    pub engine: Option<String>,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 异步抓取响应
pub struct AsyncScrapeResponse {
    pub task_id: Uuid,
}

/// 同步抓取请求
pub struct SyncScrapeRequest {
    pub team_id: Uuid,
    pub request: ScrapeRequestDto,
    pub engine: Option<String>,
    pub timeout_ms: Option<u32>,
}

/// 同步抓取响应
pub struct SyncScrapeResponse {
    pub task_id: Uuid,
    pub url: String,
    pub response_time_ms: u64,
}

/// 抓取结果查询请求
pub struct GetScrapeResultRequest {
    pub team_id: Uuid,
    pub task_id: Uuid,
}

/// 抓取结果查询响应
pub struct GetScrapeResultResponse {
    pub result: Option<ScrapeResponseDto>,
    pub status: String,
    pub is_complete: bool,
}

/// 异步抓取用例
pub struct AsyncScrapeUseCase<T: TaskRepository, R: ScrapeResultRepository, Cr: CreditsRepository> {
    task_repo: Arc<T>,
    result_repo: Arc<R>,
    credits_service: Arc<CreditsService<Cr>>,
}

impl<T: TaskRepository, R: ScrapeResultRepository, Cr: CreditsRepository> AsyncScrapeUseCase<T, R, Cr> {
    pub fn new(task_repo: Arc<T>, result_repo: Arc<R>, credits_service: Arc<CreditsService<Cr>>) -> Self {
        Self {
            task_repo,
            result_repo,
            credits_service,
        }
    }

    pub async fn execute(&self, request: AsyncScrapeRequest) -> Result<AsyncScrapeResponse, anyhow::Error> {
        // 创建抓取任务
        let payload = serde_json::to_value(&request.request).unwrap_or_default();
        let task = Task::new(
            TaskType::Scrape,
            request.team_id,
            request.request.url.clone(),
            payload,
        );

        self.task_repo.create(&task).await?;

        Ok(AsyncScrapeResponse { task_id: task.id })
    }
}

/// 同步抓取用例
pub struct SyncScrapeUseCase<R: ScrapeResultRepository, Cr: CreditsRepository> {
    result_repo: Arc<R>,
    credits_service: Arc<CreditsService<Cr>>,
}

impl<R: ScrapeResultRepository, Cr: CreditsRepository> SyncScrapeUseCase<R, Cr> {
    pub fn new(result_repo: Arc<R>, credits_service: Arc<CreditsService<Cr>>) -> Self {
        Self {
            result_repo,
            credits_service,
        }
    }

    /// 创建任务并返回基本信息供后续处理
    pub async fn prepare(&self, team_id: Uuid, request: &ScrapeRequestDto) -> Result<Task, anyhow::Error> {
        let payload = serde_json::to_value(request).unwrap_or_default();
        let task = Task::new(
            TaskType::Scrape,
            team_id,
            request.url.clone(),
            payload,
        );

        Ok(task)
    }
}

/// 获取抓取结果用例
pub struct GetScrapeResultUseCase<R: ScrapeResultRepository> {
    result_repo: Arc<R>,
}

impl<R: ScrapeResultRepository> GetScrapeResultUseCase<R> {
    pub fn new(result_repo: Arc<R>) -> Self {
        Self { result_repo }
    }

    pub async fn execute(&self, request: GetScrapeResultRequest) -> Result<GetScrapeResultResponse, anyhow::Error> {
        match self.result_repo.find_by_task_id(request.task_id).await? {
            Some(result) => {
                let response = Some(ScrapeResponseDto {
                    success: true,
                    id: request.task_id,
                    url: result.url,
                    credits_used: 1,
                });

                Ok(GetScrapeResultResponse {
                    result: response,
                    status: "Completed".to_string(),
                    is_complete: true,
                })
            }
            None => Ok(GetScrapeResultResponse {
                result: None,
                status: "NotFound".to_string(),
                is_complete: false,
            }),
        }
    }
}
