// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task handlers - HTTP request handlers for task operations

#![allow(unused_variables)]

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
use crate::presentation::handlers::extract_task_ids;
use crate::presentation::middleware::auth_middleware::AuthState;
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
/// - 最大轮询次数：60次（防止过多数据库查询）
/// - 根据任务完成进度调整间隔
/// - 任务完成率越高，轮询间隔越长
pub async fn wait_for_tasks_completion(
    task_repo: &dyn TaskRepository,
    task_ids: &[uuid::Uuid],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
    base_poll_interval_ms: u64,
) -> Result<(), AppError> {
    let start_time = Instant::now();
    let timeout_duration = Duration::from_millis(sync_wait_ms as u64);
    let min_interval = 500u64;
    let max_interval = 2000u64;
    let max_poll_count = 60u32;

    let mut current_interval = base_poll_interval_ms.clamp(min_interval, max_interval);
    let mut last_completion_rate = 0.0f64;
    let mut poll_count = 0u32;

    while start_time.elapsed() < timeout_duration {
        poll_count += 1;
        if poll_count_exceeded(poll_count, max_poll_count) {
            return Ok(());
        }

        let tasks = query_tasks_for_poll(task_repo, team_id, task_ids).await?;
        let completion_rate = calculate_completion_rate(&tasks, task_ids);

        if completion_rate >= 1.0 {
            return Ok(());
        }

        let new_interval = calculate_next_interval(
            completion_rate,
            last_completion_rate,
            current_interval,
            min_interval,
            max_interval,
        );
        current_interval = new_interval;
        last_completion_rate = completion_rate;

        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        let wait_duration = Duration::from_millis(current_interval).min(remaining_time);

        if !wait_duration.is_zero() {
            sleep(wait_duration).await;
        }
    }

    Ok(())
}

/// 检查是否达到最大轮询次数
#[inline]
fn poll_count_exceeded(count: u32, max_count: u32) -> bool {
    if count >= max_count {
        tracing::debug!("Reached max poll count ({}) for task completion", max_count);
        true
    } else {
        false
    }
}

/// 查询任务状态用于轮询
async fn query_tasks_for_poll(
    task_repo: &dyn TaskRepository,
    team_id: uuid::Uuid,
    task_ids: &[uuid::Uuid],
) -> Result<Vec<crate::domain::models::task::Task>, AppError> {
    let (tasks, _) = task_repo
        .query_tasks(TaskQueryParams {
            team_id,
            task_ids: Some(task_ids.to_vec()),
            limit: task_ids.len() as u32,
            ..Default::default()
        })
        .await?;
    Ok(tasks)
}

/// 计算任务完成率
#[inline]
fn calculate_completion_rate(
    tasks: &[crate::domain::models::task::Task],
    task_ids: &[uuid::Uuid],
) -> f64 {
    if task_ids.is_empty() {
        return 1.0;
    }

    let completed_count = tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            )
        })
        .count();

    completed_count as f64 / task_ids.len() as f64
}

/// 根据完成进度计算下一次轮询间隔
#[inline]
fn calculate_next_interval(
    completion_rate: f64,
    last_rate: f64,
    current_interval: u64,
    min_interval: u64,
    max_interval: u64,
) -> u64 {
    let progress = completion_rate - last_rate;
    let rate_based = min_interval + ((max_interval - min_interval) as f64 * completion_rate) as u64;

    match progress {
        p if p > 0.0 => ((current_interval as f64 * 1.2).max(rate_based as f64) as u64)
            .clamp(min_interval, max_interval),
        p if p < 0.0 => ((current_interval as f64 * 0.8).min(rate_based as f64) as u64)
            .clamp(min_interval, max_interval),
        _ => rate_based.clamp(min_interval, max_interval),
    }
}

/// 统一任务查询处理器
pub async fn query_tasks<T: TaskRepository>(
    Extension(auth_state): Extension<AuthState>,
    Extension(task_repo): Extension<Arc<T>>,
    Extension(scrape_result_repo): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Json(request): Json<TaskQueryRequestDto>,
) -> Result<Json<TaskQueryResponseDto>, AppError> {
    let team_id = auth_state.team_id;
    let start_time = Instant::now();

    // 验证请求参数
    validate_request(&request)?;

    // 设置默认值并提取参数
    let (limit, offset, include_results, sync_wait_ms) = apply_defaults(&request);

    // 克隆过滤条件供后续使用
    let task_types_clone = request.task_types.clone();
    let statuses_clone = request.statuses.clone();

    // 执行任务查询
    let (mut tasks, total) =
        execute_task_query(task_repo.as_ref(), team_id, &request, limit, offset).await?;

    // 处理同步等待模式
    let sync_mode = sync_wait_ms > 0 && !tasks.is_empty();
    let mut waited_time_ms = 0u64;

    if sync_mode {
        waited_time_ms =
            handle_sync_wait(task_repo.as_ref(), &tasks, team_id, sync_wait_ms).await?;

        // 重新查询任务状态
        if waited_time_ms > 0 {
            (tasks, _) =
                execute_task_query(task_repo.as_ref(), team_id, &request, limit, offset).await?;
        }
    }

    // 获取抓取结果（如果需要）
    let task_id_to_result = if include_results && !tasks.is_empty() {
        fetch_scrape_results(scrape_result_repo.as_ref(), &tasks).await?
    } else {
        None
    };

    // 构建任务信息列表
    let task_infos = build_task_infos(&tasks, task_id_to_result.as_ref());

    // 构建并返回响应
    let response_time_ms = start_time.elapsed().as_millis() as u64;
    let has_more = (offset + limit) < total as u32;
    let status = determine_sync_status(sync_mode, waited_time_ms, sync_wait_ms);

    Ok(Json(TaskQueryResponseDto {
        success: true,
        status,
        data: TaskQueryDataDto {
            tasks: task_infos,
            total,
            has_more,
        },
        credits_used: 1,
        response_time_ms,
    }))
}

/// 验证请求参数
fn validate_request(request: &TaskQueryRequestDto) -> Result<(), AppError> {
    if let Err(errors) = request.validate() {
        Err(AppError::from(anyhow::anyhow!(
            "Validation error: {:?}",
            errors
        )))
    } else {
        Ok(())
    }
}

/// 应用请求默认值并提取参数
fn apply_defaults(request: &TaskQueryRequestDto) -> (u32, u32, bool, u32) {
    (
        request.limit.unwrap_or(100).min(1000),
        request.offset.unwrap_or(0),
        request.include_results.unwrap_or(false),
        request.sync_wait_ms.unwrap_or(5000),
    )
}

/// 执行任务查询
async fn execute_task_query<T: TaskRepository>(
    task_repo: &T,
    team_id: uuid::Uuid,
    request: &TaskQueryRequestDto,
    limit: u32,
    offset: u32,
) -> Result<(Vec<crate::domain::models::task::Task>, u64), AppError> {
    task_repo
        .query_tasks(TaskQueryParams {
            team_id,
            task_ids: request.task_ids.clone(),
            task_types: request.task_types.clone(),
            statuses: request.statuses.clone(),
            created_after: request.created_after,
            created_before: request.created_before,
            crawl_id: request.crawl_id,
            limit,
            offset,
        })
        .await
        .map_err(|e| AppError::from(anyhow::anyhow!("Query failed: {:?}", e)))
}

/// 处理同步等待模式
async fn handle_sync_wait<T: TaskRepository>(
    task_repo: &T,
    tasks: &[crate::domain::models::task::Task],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
) -> Result<u64, AppError> {
    let task_ids = extract_task_ids(tasks);
    let wait_start = Instant::now();

    wait_for_tasks_completion(task_repo, &task_ids, team_id, sync_wait_ms, 1000).await?;

    Ok(wait_start.elapsed().as_millis() as u64)
}

/// 获取抓取结果
async fn fetch_scrape_results(
    scrape_result_repo: &ScrapeResultRepositoryImpl,
    tasks: &[crate::domain::models::task::Task],
) -> Result<
    Option<
        std::collections::HashMap<uuid::Uuid, crate::domain::models::scrape_result::ScrapeResult>,
    >,
    AppError,
> {
    let task_ids = extract_task_ids(tasks);
    let results = scrape_result_repo.find_by_task_ids(&task_ids).await?;

    let mut map = std::collections::HashMap::with_capacity(results.len());
    for result in results {
        map.insert(result.task_id, result);
    }
    Ok(Some(map))
}

/// 构建任务信息列表
fn build_task_infos(
    tasks: &[crate::domain::models::task::Task],
    results_map: Option<
        &std::collections::HashMap<uuid::Uuid, crate::domain::models::scrape_result::ScrapeResult>,
    >,
) -> Vec<TaskInfoDto> {
    tasks
        .iter()
        .map(|task| {
            let result = results_map
                .and_then(|m| m.get(&task.id))
                .map(build_scrape_result_json);
            TaskInfoDto {
                id: task.id,
                task_type: task.task_type,
                status: task.status,
                priority: task.priority,
                url: task.url.clone(),
                attempt_count: task.attempt_count,
                max_retries: task.max_retries,
                created_at: task.created_at,
                started_at: task.started_at,
                completed_at: task.completed_at,
                crawl_id: task.crawl_id,
                result,
            }
        })
        .collect()
}

/// 构建抓取结果 JSON
fn build_scrape_result_json(
    scrape_result: &crate::domain::models::scrape_result::ScrapeResult,
) -> serde_json::Value {
    let escaped_content = html_escape::encode_text(&scrape_result.content);
    serde_json::json!({
        "id": scrape_result.id,
        "status_code": scrape_result.status_code,
        "content": escaped_content,
        "metadata": scrape_result.meta_data,
    })
}

/// 确定同步状态
fn determine_sync_status(sync_mode: bool, waited_time_ms: u64, sync_wait_ms: u32) -> String {
    if !sync_mode {
        return "async".to_string();
    }
    if waited_time_ms >= sync_wait_ms as u64 {
        "sync_timeout".to_string()
    } else {
        "sync_completed".to_string()
    }
}

/// 统一任务取消处理器
pub async fn cancel_tasks<T: TaskRepository>(
    Extension(auth_state): Extension<AuthState>,
    Extension(task_repo): Extension<Arc<T>>,
    Json(request): Json<TaskCancelRequestDto>,
) -> Result<Json<TaskCancelResponseDto>, AppError> {
    let team_id = auth_state.team_id;
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
        return Err(AppError::Validation("Task IDs cannot be empty".to_string()));
    }

    let force = request.force.unwrap_or(false);
    let sync_wait_ms = request.sync_wait_ms.unwrap_or(5000);

    // 执行批量取消（使用认证上下文的 team_id）
    let (cancelled_task_ids, failed_tasks) = task_repo
        .batch_cancel(request.task_ids.clone(), team_id, force) // 使用认证上下文的 team_id
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
        credits_used: total_cancelled as u32, // 每个取消的任务消耗1个credit，批量取消按数量计费
        response_time_ms,
    }))
}
