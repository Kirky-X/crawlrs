// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task query handlers — GET-style task query operations.
//!
//! 从 `task_handler.rs` 拆分出的查询类处理器及其辅助函数。

#![allow(unused_variables)]

use crate::application::dto::task_query_request::{
    ScrapeResultInfoDto, TaskInfoDto, TaskQueryDataDto, TaskQueryRequestDto,
};
use crate::common::constants::crawl_task;
use crate::common::constants::server_config;
use crate::domain::models::TaskStatus;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::presentation::errors::CrawlRsError;
use crate::presentation::handlers::extract_task_ids;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::middleware::auth_middleware::AuthState;
use anyhow;
use axum::{extract::Extension, Json};
use chrono::{TimeZone, Utc};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use validator::Validate;

/// Poll task statuses until all complete or the timeout expires.
///
/// Uses adaptive polling: starts at `base_poll_interval_ms`, dynamically
/// adjusts between 500 ms – 2 000 ms based on completion rate, and caps
/// total iterations at `MAX_POLL_COUNT`.
///
/// # Arguments
///
/// * `task_repo` - Task repository for status polling.
/// * `task_ids` - IDs of tasks to wait for.
/// * `team_id` - Owning team (used for logging / authorization).
/// * `sync_wait_ms` - Maximum wait time in milliseconds.
/// * `base_poll_interval_ms` - Initial polling interval.
///
/// # Errors
///
/// Returns `CrawlRsError` if the repository cannot be reached.
pub async fn wait_for_tasks_completion(
    task_repo: &dyn TaskRepository,
    task_ids: &[uuid::Uuid],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
    base_poll_interval_ms: u64,
) -> Result<(), CrawlRsError> {
    let start_time = Instant::now();
    let timeout_duration = Duration::from_millis(sync_wait_ms as u64);
    let min_interval = 500u64;
    let max_interval = 2000u64;

    let mut current_interval = base_poll_interval_ms.clamp(min_interval, max_interval);
    let mut last_completion_rate = 0.0f64;
    let mut poll_count = 0u32;

    while start_time.elapsed() < timeout_duration {
        poll_count += 1;
        if poll_count_exceeded(poll_count, crawl_task::MAX_POLL_COUNT) {
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
pub(crate) fn poll_count_exceeded(count: u32, max_count: u32) -> bool {
    if count >= max_count {
        log::debug!("Reached max poll count ({}) for task completion", max_count);
        true
    } else {
        false
    }
}

/// 查询任务状态用于轮询
pub(crate) async fn query_tasks_for_poll(
    task_repo: &dyn TaskRepository,
    team_id: uuid::Uuid,
    task_ids: &[uuid::Uuid],
) -> Result<Vec<crate::domain::models::Task>, CrawlRsError> {
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
pub(crate) fn calculate_completion_rate(
    tasks: &[crate::domain::models::Task],
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
pub(crate) fn calculate_next_interval(
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

/// 任务查询响应扩展数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskQueryResponseMeta {
    /// 同步状态
    pub status: String,
    /// 消耗的积分
    pub credits_used: u32,
    /// 响应时间（毫秒）
    pub response_time_ms: u64,
}

/// Query tasks with pagination, filtering, and optional scrape-result enrichment.
///
/// # Arguments
///
/// * `auth_state` - Authenticated caller state.
/// * `task_repo` - Task repository for querying.
/// * `scrape_result_repo` - Scrape result repository for enrichment.
/// * `request` - Query parameters (pagination, filters, sync wait).
///
/// # Errors
///
/// Returns `CrawlRsError` on validation failure or repository error.
pub async fn query_tasks<T: TaskRepository>(
    Extension(auth_state): Extension<AuthState>,
    Extension(task_repo): Extension<Arc<T>>,
    Extension(scrape_result_repo): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Json(request): Json<TaskQueryRequestDto>,
) -> Result<Json<ApiResponse<TaskQueryDataDto>>, CrawlRsError> {
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
    let _waited_time_ms;

    if sync_mode {
        _waited_time_ms =
            handle_sync_wait(task_repo.as_ref(), &tasks, team_id, sync_wait_ms).await?;

        // 重新查询任务状态
        if _waited_time_ms > 0 {
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
    let has_more = (offset + limit) < total as u32;

    Ok(Json(ApiResponse::success(TaskQueryDataDto {
        tasks: task_infos,
        total,
        has_more,
    })))
}

/// 验证请求参数
pub(crate) fn validate_request(request: &TaskQueryRequestDto) -> Result<(), CrawlRsError> {
    if let Err(errors) = request.validate() {
        Err(CrawlRsError::from(anyhow::anyhow!(
            "Validation error: {:?}",
            errors
        )))
    } else {
        Ok(())
    }
}

/// 应用请求默认值并提取参数
pub(crate) fn apply_defaults(request: &TaskQueryRequestDto) -> (u32, u32, bool, u32) {
    (
        request
            .limit
            .unwrap_or(server_config::DEFAULT_PAGE_LIMIT)
            .min(server_config::MAX_PAGE_LIMIT),
        request.offset.unwrap_or(0),
        request.include_results.unwrap_or(false),
        request
            .sync_wait_ms
            .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32),
    )
}

/// 执行任务查询
pub(crate) async fn execute_task_query<T: TaskRepository>(
    task_repo: &T,
    team_id: uuid::Uuid,
    request: &TaskQueryRequestDto,
    limit: u32,
    offset: u32,
) -> Result<(Vec<crate::domain::models::Task>, u64), CrawlRsError> {
    task_repo
        .query_tasks(TaskQueryParams {
            team_id,
            task_ids: request.task_ids.clone(),
            task_types: request.task_types.clone(),
            statuses: request.statuses.clone(),
            created_after: request.created_after.map(|dt| dt.with_timezone(&Utc)),
            created_before: request.created_before.map(|dt| dt.with_timezone(&Utc)),
            crawl_id: request.crawl_id,
            limit,
            offset,
            cursor: None,
            cursor_id: None,
        })
        .await
        .map_err(|e| CrawlRsError::from(anyhow::anyhow!("Query failed: {:?}", e)))
}

/// 处理同步等待模式
pub(crate) async fn handle_sync_wait<T: TaskRepository>(
    task_repo: &T,
    tasks: &[crate::domain::models::Task],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
) -> Result<u64, CrawlRsError> {
    let task_ids = extract_task_ids(tasks);
    let wait_start = Instant::now();

    wait_for_tasks_completion(
        task_repo,
        &task_ids,
        team_id,
        sync_wait_ms,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await?;

    Ok(wait_start.elapsed().as_millis() as u64)
}

/// 同步等待结果
pub struct SyncWaitResult {
    /// 实际等待时间（毫秒）
    pub waited_time_ms: u64,
    /// 是否超时
    pub is_timeout: bool,
}

/// Orchestrate synchronous wait for a set of tasks and return the aggregate status.
///
/// This is the public entry-point used by handlers. When `sync_wait_ms` is
/// 0 or `task_ids` is empty the function returns immediately.
///
/// # Arguments
///
/// * `task_repo` - Task repository for polling.
/// * `task_ids` - IDs of tasks to monitor.
/// * `team_id` - Owning team.
/// * `sync_wait_ms` - Maximum wait time in milliseconds (0 = no wait).
///
/// # Errors
///
/// Returns `CrawlRsError` if the wait or repository call fails.
pub async fn handle_sync_wait_and_get_status(
    task_repo: &dyn TaskRepository,
    task_ids: &[uuid::Uuid],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
) -> Result<SyncWaitResult, CrawlRsError> {
    if sync_wait_ms == 0 || task_ids.is_empty() {
        return Ok(SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        });
    }

    let wait_start = Instant::now();

    match wait_for_tasks_completion(
        task_repo,
        task_ids,
        team_id,
        sync_wait_ms,
        crawl_task::BASE_POLL_INTERVAL_MS,
    )
    .await
    {
        Ok(_) => {
            let waited_time_ms = wait_start.elapsed().as_millis() as u64;
            Ok(SyncWaitResult {
                waited_time_ms,
                is_timeout: waited_time_ms >= sync_wait_ms as u64,
            })
        }
        Err(e) => {
            log::error!("Failed to wait for task completion: {:?}", e);
            // 即使等待失败，也返回已创建的任务信息
            let waited_time_ms = wait_start.elapsed().as_millis() as u64;
            Ok(SyncWaitResult {
                waited_time_ms,
                is_timeout: waited_time_ms >= sync_wait_ms as u64,
            })
        }
    }
}

/// 获取抓取结果
pub(crate) async fn fetch_scrape_results(
    scrape_result_repo: &ScrapeResultRepositoryImpl,
    tasks: &[crate::domain::models::Task],
) -> Result<
    Option<std::collections::HashMap<uuid::Uuid, crate::domain::models::ScrapeResult>>,
    CrawlRsError,
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
pub(crate) fn build_task_infos(
    tasks: &[crate::domain::models::Task],
    results_map: Option<
        &std::collections::HashMap<uuid::Uuid, crate::domain::models::ScrapeResult>,
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
                created_at: chrono::FixedOffset::east_opt(0)
                    .unwrap()
                    .from_utc_datetime(&task.created_at.naive_utc()),
                started_at: task.started_at.as_ref().map(|dt| {
                    chrono::FixedOffset::east_opt(0)
                        .unwrap()
                        .from_utc_datetime(&dt.naive_utc())
                }),
                completed_at: task.completed_at.as_ref().map(|dt| {
                    chrono::FixedOffset::east_opt(0)
                        .unwrap()
                        .from_utc_datetime(&dt.naive_utc())
                }),
                crawl_id: task.crawl_id,
                result,
            }
        })
        .collect()
}

/// 构建抓取结果信息
pub(crate) fn build_scrape_result_json(
    scrape_result: &crate::domain::models::ScrapeResult,
) -> ScrapeResultInfoDto {
    let escaped_content = html_escape::encode_text(&scrape_result.content);
    ScrapeResultInfoDto {
        id: scrape_result.id,
        status_code: scrape_result.status_code as u16,
        content: escaped_content.to_string(),
        metadata: Some(scrape_result.meta_data.clone()),
    }
}
