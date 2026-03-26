// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task-related use cases

use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use chrono::{DateTime, FixedOffset};
use std::sync::Arc;
use uuid::Uuid;

/// 创建任务请求
pub struct CreateTaskRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub task_type: TaskType,
    pub url: String,
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 创建任务响应
pub struct CreateTaskResponse {
    pub task: Task,
}

/// 查询任务请求
pub struct QueryTasksRequest {
    pub team_id: Uuid,
    pub task_ids: Option<Vec<Uuid>>,
    pub task_types: Option<Vec<TaskType>>,
    pub statuses: Option<Vec<TaskStatus>>,
    pub created_after: Option<DateTime<FixedOffset>>,
    pub created_before: Option<DateTime<FixedOffset>>,
    pub crawl_id: Option<Uuid>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// 查询任务响应
pub struct QueryTasksResponse {
    pub tasks: Vec<Task>,
    pub total: u64,
    pub has_more: bool,
}

/// 取消任务请求
pub struct CancelTasksRequest {
    pub team_id: Uuid,
    pub task_ids: Vec<Uuid>,
    pub force: Option<bool>,
}

/// 取消任务响应
pub struct CancelTasksResponse {
    pub cancelled: Vec<Uuid>,
    pub failed: Vec<(Uuid, String)>,
    pub total_cancelled: u64,
    pub total_failed: u64,
}

/// 创建任务用例
#[allow(dead_code)]
pub struct CreateTaskUseCase<T: TaskRepository, R: CreditsRepository> {
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<T: TaskRepository, R: CreditsRepository> CreateTaskUseCase<T, R> {
    pub fn new(task_repo: Arc<T>, credits_service: Arc<CreditsService<R>>) -> Self {
        Self {
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: CreateTaskRequest,
    ) -> Result<CreateTaskResponse, anyhow::Error> {
        // 创建任务
        let payload = request.config.unwrap_or_else(|| serde_json::json!({}));
        let mut task = Task::new(
            Uuid::new_v4(),
            request.task_type,
            request.team_id,
            request.api_key_id,
            request.url,
            payload,
        );
        
        // 设置可选参数
        if let Some(priority) = request.priority {
            task.priority = priority;
        }
        if let Some(max_retries) = request.max_retries {
            task.max_retries = max_retries;
        }
        if let Some(expires_at) = request.expires_at {
            task.expires_at = Some(expires_at);
        }

        self.task_repo.create(&task).await?;

        Ok(CreateTaskResponse { task })
    }
}

/// 查询任务用例
pub struct QueryTasksUseCase<T: TaskRepository> {
    task_repo: Arc<T>,
}

impl<T: TaskRepository> QueryTasksUseCase<T> {
    pub fn new(task_repo: Arc<T>) -> Self {
        Self { task_repo }
    }

    pub async fn execute(
        &self,
        request: QueryTasksRequest,
    ) -> Result<QueryTasksResponse, anyhow::Error> {
        let params = crate::domain::repositories::task_repository::TaskQueryParams {
            team_id: request.team_id,
            task_ids: request.task_ids,
            task_types: request.task_types,
            statuses: request.statuses,
            created_after: request.created_after.map(|dt| dt.with_timezone(&chrono::Utc)),
            created_before: request.created_before.map(|dt| dt.with_timezone(&chrono::Utc)),
            crawl_id: request.crawl_id,
            limit: request.limit.unwrap_or(100),
            offset: request.offset.unwrap_or(0),
            cursor: None,
            cursor_id: None,
        };

        let (tasks, total) = self.task_repo.query_tasks(params).await?;

        let has_more = (u64::from(request.offset.unwrap_or(0)) + tasks.len() as u64) < total;

        Ok(QueryTasksResponse {
            tasks,
            total,
            has_more,
        })
    }
}

/// 取消任务用例
#[allow(dead_code)]
pub struct CancelTasksUseCase<T: TaskRepository, R: CreditsRepository> {
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<T: TaskRepository, R: CreditsRepository> CancelTasksUseCase<T, R> {
    pub fn new(task_repo: Arc<T>, credits_service: Arc<CreditsService<R>>) -> Self {
        Self {
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: CancelTasksRequest,
    ) -> Result<CancelTasksResponse, anyhow::Error> {
        let mut cancelled = Vec::new();
        let mut failed = Vec::new();

        for task_id in &request.task_ids {
            match self.task_repo.find_by_id(*task_id).await {
                Ok(Some(task)) => {
                    if task.team_id != request.team_id {
                        failed.push((*task_id, "Task does not belong to team".to_string()));
                        continue;
                    }

                    if task.status == TaskStatus::Completed || task.status == TaskStatus::Failed {
                        failed.push((
                            *task_id,
                            format!("Cannot cancel task in status: {}", task.status),
                        ));
                        continue;
                    }

                    // 如果不是强制取消且任务正在执行中，则不允许取消
                    if !request.force.unwrap_or(false) && task.status == TaskStatus::Active {
                        failed.push((
                            *task_id,
                            "Task is running, use force=true to cancel".to_string(),
                        ));
                        continue;
                    }

                    // 更新任务状态为已取消
                    self.task_repo.mark_cancelled(*task_id).await?;
                    cancelled.push(*task_id);
                }
                Ok(None) => {
                    failed.push((*task_id, "Task not found".to_string()));
                }
                Err(e) => {
                    failed.push((*task_id, format!("Repository error: {:?}", e)));
                }
            }
        }

        let total_cancelled = cancelled.len() as u64;
        let total_failed = failed.len() as u64;

        Ok(CancelTasksResponse {
            cancelled,
            failed,
            total_cancelled,
            total_failed,
        })
    }
}
