// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::application::dto::task_query_request::{
    CancelledTaskInfoDto, FailedTaskInfoDto, TaskCancelDataDto, TaskCancelRequestDto,
    TaskCancelResponseDto, TaskInfoDto, TaskQueryDataDto, TaskQueryRequestDto,
    TaskQueryResponseDto,
};
use crate::domain::models::task::TaskStatus;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::presentation::errors::AppError;
use anyhow;
use axum::{extract::Extension, Json};
use chrono::Utc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use validator::Validate;

/// 智能轮询等待任务完成
///
/// # 参数
/// * `task_repo` - 任务仓库
/// * `task_ids` - 要等待的任务ID列表
/// * `team_id` - 团队ID
/// * `sync_wait_ms` - 同步等待时间（毫秒）
/// * `base_poll_interval_ms` - 基础轮询间隔（毫秒）
///
/// # 返回值
/// * `Ok(())` - 所有任务完成或超时
/// * `Err(AppError)` - 查询失败
///
/// # 智能轮询逻辑
/// - 初始轮询间隔：base_poll_interval_ms
/// - 动态调整范围：500ms - 2000ms
/// - 根据任务完成进度调整间隔
/// - 任务完成率越高，轮询间隔越长
pub async fn wait_for_tasks_completion<T: TaskRepository>(
    task_repo: &T,
    task_ids: &[uuid::Uuid],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
    base_poll_interval_ms: u64,
) -> Result<(), AppError> {
    let start_time = Instant::now();
    let timeout_duration = Duration::from_millis(sync_wait_ms as u64);
    let min_interval = 500u64; // 最小轮询间隔 500ms
    let max_interval = 2000u64; // 最大轮询间隔 2000ms

    let mut current_interval = base_poll_interval_ms.clamp(min_interval, max_interval);
    let mut last_completion_rate = 0.0f64;

    while start_time.elapsed() < timeout_duration {
        // 查询所有任务的状态
        let (tasks, _) = task_repo
            .query_tasks(TaskQueryParams {
                team_id,
                task_ids: Some(task_ids.to_vec()),
                limit: task_ids.len() as u32,
                ..Default::default()
            })
            .await?;

        // 计算任务完成率
        let completed_count = tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            })
            .count();

        let completion_rate = if task_ids.is_empty() {
            1.0
        } else {
            completed_count as f64 / task_ids.len() as f64
        };

        // 如果所有任务都已完成，立即返回
        if completion_rate >= 1.0 {
            return Ok(());
        }

        // 动态调整轮询间隔
        // - 完成率提升时，增加轮询间隔
        // - 完成率下降或不变时，减少轮询间隔
        // - 根据完成率线性调整间隔：完成率越高，间隔越长
        let completion_progress = completion_rate - last_completion_rate;
        let rate_based_interval =
            min_interval + ((max_interval - min_interval) as f64 * completion_rate) as u64;

        if completion_progress > 0.0 {
            // 完成率有提升，倾向于增加间隔
            current_interval = ((current_interval as f64 * 1.2).max(rate_based_interval as f64)
                as u64)
                .clamp(min_interval, max_interval);
        } else if completion_progress < 0.0 {
            // 完成率下降，减少间隔
            current_interval = ((current_interval as f64 * 0.8).min(rate_based_interval as f64)
                as u64)
                .clamp(min_interval, max_interval);
        } else {
            // 完成率不变，使用基于完成率的间隔
            current_interval = rate_based_interval.clamp(min_interval, max_interval);
        }

        last_completion_rate = completion_rate;

        // 等待下一轮轮询，但确保不会超出超时时间
        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        let wait_duration = Duration::from_millis(current_interval).min(remaining_time);

        if !wait_duration.is_zero() {
            sleep(wait_duration).await;
        }
    }

    // 超时返回，但不报错，只是表示同步等待结束
    Ok(())
}

/// 统一任务查询处理器
pub async fn query_tasks<T: TaskRepository>(
    Extension(task_repo): Extension<Arc<T>>,
    Extension(scrape_result_repo): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Json(request): Json<TaskQueryRequestDto>,
) -> Result<Json<TaskQueryResponseDto>, AppError> {
    let start_time = Instant::now();

    // 验证请求参数
    if let Err(errors) = request.validate() {
        return Err(AppError::from(anyhow::anyhow!(
            "Validation error: {:?}",
            errors
        )));
    }

    // 设置默认值
    let limit = request.limit.unwrap_or(100).min(1000);
    let offset = request.offset.unwrap_or(0);
    let include_results = request.include_results.unwrap_or(false);
    let sync_wait_ms = request.sync_wait_ms.unwrap_or(5000);

    // 克隆过滤条件，避免值移动问题
    let task_types_clone = request.task_types.clone();
    let statuses_clone = request.statuses.clone();

    // 执行查询
    let (mut tasks, total) = task_repo
        .query_tasks(TaskQueryParams {
            team_id: request.team_id,
            task_ids: request.task_ids.clone(),
            task_types: request.task_types,
            statuses: request.statuses,
            created_after: request.created_after,
            created_before: request.created_before,
            crawl_id: request.crawl_id,
            limit,
            offset,
        })
        .await?;

    // 同步等待机制：如果指定了sync_wait_ms且任务列表不为空，等待任务完成
    let sync_mode = sync_wait_ms > 0 && !tasks.is_empty();
    let mut waited_time_ms = 0u64;

    if sync_mode {
        let task_ids: Vec<uuid::Uuid> = tasks.iter().map(|task| task.id).collect();
        let wait_start = Instant::now();

        // 智能轮询等待任务完成，轮询间隔动态调整（500ms-2000ms）
        // 根据任务完成进度自动调整轮询频率，完成率越高轮询越慢
        wait_for_tasks_completion(
            task_repo.as_ref(),
            &task_ids,
            request.team_id,
            sync_wait_ms,
            1000, // 基础轮询间隔1秒
        )
        .await?;

        waited_time_ms = wait_start.elapsed().as_millis() as u64;

        // 重新查询任务状态以获取最新状态
        if waited_time_ms > 0 {
            (tasks, _) = task_repo
                .query_tasks(TaskQueryParams {
                    team_id: request.team_id,
                    task_ids: request.task_ids.clone(),
                    task_types: task_types_clone,
                    statuses: statuses_clone,
                    created_after: request.created_after,
                    created_before: request.created_before,
                    crawl_id: request.crawl_id,
                    limit,
                    offset,
                })
                .await?;
        }
    }

    // 构建任务信息列表
    let mut task_infos = Vec::new();
    for task in tasks {
        let mut result = None;

        // 如果需要包含结果数据，查询相关的结果
        if include_results {
            if let Ok(Some(scrape_result)) = scrape_result_repo.find_by_task_id(task.id).await {
                result = Some(serde_json::json!({
                    "id": scrape_result.id,
                    "status_code": scrape_result.status_code,
                    "content": scrape_result.content,
                    "metadata": scrape_result.meta_data,
                }));
            }
        }

        task_infos.push(TaskInfoDto {
            id: task.id,
            task_type: task.task_type,
            status: task.status,
            priority: task.priority,
            url: task.url,
            attempt_count: task.attempt_count,
            max_retries: task.max_retries,
            created_at: task.created_at,
            started_at: task.started_at,
            completed_at: task.completed_at,
            crawl_id: task.crawl_id,
            result,
        });
    }

    let response_time_ms = start_time.elapsed().as_millis() as u64;
    let has_more = (offset + limit) < total as u32;

    // 构建响应状态，包含同步等待信息
    let status = if sync_mode {
        if waited_time_ms >= sync_wait_ms as u64 {
            "sync_timeout" // 同步等待超时
        } else {
            "sync_completed" // 同步等待完成
        }
    } else {
        "async" // 异步模式
    };

    Ok(Json(TaskQueryResponseDto {
        success: true,
        status: status.to_string(),
        data: TaskQueryDataDto {
            tasks: task_infos,
            total,
            has_more,
        },
        credits_used: 1, // 查询任务消耗1个credit
        response_time_ms,
    }))
}

/// 统一任务取消处理器
pub async fn cancel_tasks<T: TaskRepository>(
    Extension(task_repo): Extension<Arc<T>>,
    Json(request): Json<TaskCancelRequestDto>,
) -> Result<Json<TaskCancelResponseDto>, AppError> {
    let start_time = Instant::now();

    // 验证请求参数
    if let Err(errors) = request.validate() {
        return Err(AppError::from(anyhow::anyhow!(
            "Validation error: {:?}",
            errors
        )));
    }

    // 验证任务ID列表不为空
    if request.task_ids.is_empty() {
        return Err(AppError::from(anyhow::anyhow!("Task IDs cannot be empty")));
    }

    let force = request.force.unwrap_or(false);
    let sync_wait_ms = request.sync_wait_ms.unwrap_or(5000);

    // 执行批量取消
    let (cancelled_task_ids, failed_tasks) = task_repo
        .batch_cancel(request.task_ids.clone(), request.team_id, force)
        .await?;

    // 同步等待机制：如果指定了sync_wait_ms且有任务被取消，等待取消操作完成
    let sync_mode = sync_wait_ms > 0 && !cancelled_task_ids.is_empty();
    let mut waited_time_ms = 0u64;

    if sync_mode {
        let wait_start = Instant::now();

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

        waited_time_ms = wait_start.elapsed().as_millis() as u64;
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

    let response_time_ms = start_time.elapsed().as_millis() as u64;
    let total_cancelled = cancelled_tasks.len() as u64;
    let total_failed = failed_tasks_info.len() as u64;

    // 构建响应状态，包含同步等待信息
    let status = if sync_mode {
        if waited_time_ms >= sync_wait_ms as u64 {
            "sync_timeout" // 同步等待超时
        } else {
            "sync_completed" // 同步等待完成
        }
    } else {
        "async" // 异步模式
    };

    Ok(Json(TaskCancelResponseDto {
        success: true,
        status: status.to_string(),
        data: TaskCancelDataDto {
            cancelled_tasks,
            failed_tasks: failed_tasks_info,
            total_cancelled,
            total_failed,
        },
        credits_used: total_cancelled as u32, // 每个取消的任务消耗1个credit
        response_time_ms,
    }))
}
