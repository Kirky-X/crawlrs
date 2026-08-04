// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task command handlers — POST/PUT/DELETE task mutation operations.
//!
//! 从 `task_handler.rs` 拆分出的命令类处理器。

#![allow(unused_variables)]

use crate::application::dto::task_query_request::{
    CancelledTaskInfoDto, FailedTaskInfoDto, TaskCancelDataDto, TaskCancelRequestDto,
};
use crate::domain::repositories::task_repository::TaskRepository;
use crate::presentation::errors::CrawlRsError;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::middleware::auth_middleware::AuthState;
use anyhow;
use axum::{extract::Extension, Json};
use chrono::Utc;
use std::sync::Arc;
use validator::Validate;

use super::task_queries::wait_for_tasks_completion;

/// Cancel one or more tasks by their IDs.
///
/// # Arguments
///
/// * `auth_state` - Authenticated caller state (team ownership check).
/// * `task_repo` - Task repository for persistence.
/// * `request` - DTO containing task IDs and optional `force` flag.
///
/// # Errors
///
/// Returns `CrawlRsError` when validation fails, the repository errors, or
/// the caller does not own the requested tasks.
pub async fn cancel_tasks<T: TaskRepository>(
    Extension(auth_state): Extension<AuthState>,
    Extension(task_repo): Extension<Arc<T>>,
    Json(request): Json<TaskCancelRequestDto>,
) -> Result<Json<ApiResponse<TaskCancelDataDto>>, CrawlRsError> {
    let team_id = auth_state.team_id;

    // 验证请求参数
    if let Err(errors) = request.validate() {
        return Err(CrawlRsError::from(anyhow::anyhow!(
            "Validation error: {:?}",
            errors
        )));
    }

    // 验证任务ID列表不为空
    if request.task_ids.is_empty() {
        return Err(CrawlRsError::Validation(
            "Task IDs cannot be empty".to_string(),
        ));
    }

    let force = request.force.unwrap_or(false);
    let sync_wait_ms = request.sync_wait_ms.unwrap_or(5000);

    // 执行批量取消（使用认证上下文的 team_id）
    let (cancelled_task_ids, failed_tasks) = task_repo
        .batch_cancel(request.task_ids.clone(), team_id, force) // 使用认证上下文的 team_id
        .await?;

    // 同步等待机制：如果指定了sync_wait_ms且有任务被取消，等待取消操作完成
    let sync_mode = sync_wait_ms > 0 && !cancelled_task_ids.is_empty();

    if sync_mode {
        // 智能轮询等待取消的任务状态更新完成
        // 取消操作使用更短的初始轮询间隔（500ms），更快响应取消状态变化
        wait_for_tasks_completion(
            task_repo.as_ref(),
            &cancelled_task_ids,
            request.team_id,
            sync_wait_ms,
            500, // 取消操作轮询间隔500ms
        )
        .await?;
    }

    // 构建取消成功的任务信息
    let cancelled_tasks: Vec<CancelledTaskInfoDto> = cancelled_task_ids
        .into_iter()
        .map(|task_id| CancelledTaskInfoDto {
            task_id,
            status: "cancelled".to_string(),
            cancelled_at: Utc::now().into(),
        })
        .collect();

    // 构建取消失败的任务信息
    let failed_tasks_info: Vec<FailedTaskInfoDto> = failed_tasks
        .into_iter()
        .map(|(task_id, reason)| FailedTaskInfoDto { task_id, reason })
        .collect();

    let total_cancelled = cancelled_tasks.len() as u64;
    let total_failed = failed_tasks_info.len() as u64;

    Ok(Json(ApiResponse::success(TaskCancelDataDto {
        cancelled_tasks,
        failed_tasks: failed_tasks_info,
        total_cancelled,
        total_failed,
    })))
}
