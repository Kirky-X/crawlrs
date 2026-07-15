// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task handlers - HTTP request handlers for task operations

#![allow(unused_variables)]

use crate::application::dto::task_query_request::{
    CancelledTaskInfoDto, FailedTaskInfoDto, ScrapeResultInfoDto, TaskCancelDataDto,
    TaskCancelRequestDto, TaskInfoDto, TaskQueryDataDto, TaskQueryRequestDto,
};
use crate::common::constants::crawl_task;
use crate::common::constants::server_config;
use crate::domain::models::TaskStatus;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::presentation::errors::AppError;
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
/// - 最大轮询次数：由 crawl_task::MAX_POLL_COUNT 控制（防止过多数据库查询）
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
fn poll_count_exceeded(count: u32, max_count: u32) -> bool {
    if count >= max_count {
        log::debug!("Reached max poll count ({}) for task completion", max_count);
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
) -> Result<Vec<crate::domain::models::Task>, AppError> {
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

/// 统一任务查询处理器
pub async fn query_tasks<T: TaskRepository>(
    Extension(auth_state): Extension<AuthState>,
    Extension(task_repo): Extension<Arc<T>>,
    Extension(scrape_result_repo): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Json(request): Json<TaskQueryRequestDto>,
) -> Result<Json<ApiResponse<TaskQueryDataDto>>, AppError> {
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
async fn execute_task_query<T: TaskRepository>(
    task_repo: &T,
    team_id: uuid::Uuid,
    request: &TaskQueryRequestDto,
    limit: u32,
    offset: u32,
) -> Result<(Vec<crate::domain::models::Task>, u64), AppError> {
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
        .map_err(|e| AppError::from(anyhow::anyhow!("Query failed: {:?}", e)))
}

/// 处理同步等待模式
async fn handle_sync_wait<T: TaskRepository>(
    task_repo: &T,
    tasks: &[crate::domain::models::Task],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
) -> Result<u64, AppError> {
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

/// 处理同步等待并返回状态码
///
/// 此函数封装了同步等待的通用逻辑，消除 crawl_handler 和 scrape_handler 中的重复代码
///
/// # 参数
/// * `task_repo` - 任务仓库
/// * `task_ids` - 要等待的任务ID列表
/// * `team_id` - 团队ID
/// * `sync_wait_ms` - 同步等待时间（毫秒）
///
/// # 返回值
/// * `Ok(SyncWaitResult)` - 同步等待结果
/// * `Err(AppError)` - 等待失败
pub async fn handle_sync_wait_and_get_status(
    task_repo: &dyn TaskRepository,
    task_ids: &[uuid::Uuid],
    team_id: uuid::Uuid,
    sync_wait_ms: u32,
) -> Result<SyncWaitResult, AppError> {
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
async fn fetch_scrape_results(
    scrape_result_repo: &ScrapeResultRepositoryImpl,
    tasks: &[crate::domain::models::Task],
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
    tasks: &[crate::domain::models::Task],
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
fn build_scrape_result_json(
    scrape_result: &crate::domain::models::scrape_result::ScrapeResult,
) -> ScrapeResultInfoDto {
    let escaped_content = html_escape::encode_text(&scrape_result.content);
    ScrapeResultInfoDto {
        id: scrape_result.id,
        status_code: scrape_result.status_code as u16,
        content: escaped_content.to_string(),
        metadata: Some(scrape_result.meta_data.clone()),
    }
}

/// 确定同步状态
#[allow(dead_code)]
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
) -> Result<Json<ApiResponse<TaskCancelDataDto>>, AppError> {
    let team_id = auth_state.team_id;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::RepositoryError;
    use async_trait::async_trait;
    use dbnexus::{DbConfig, DbPool};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    // ========== Helper to create test Task ==========

    fn make_test_task(id: Uuid, status: TaskStatus) -> Task {
        let now = chrono::Utc::now();
        Task {
            id,
            task_type: TaskType::Scrape,
            status,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        }
    }

    // ========== poll_count_exceeded tests ==========

    #[test]
    fn test_poll_count_exceeded_true_when_count_equals_max() {
        assert!(poll_count_exceeded(60, 60));
    }

    #[test]
    fn test_poll_count_exceeded_true_when_count_exceeds_max() {
        assert!(poll_count_exceeded(100, 60));
    }

    #[test]
    fn test_poll_count_exceeded_false_when_count_below_max() {
        assert!(!poll_count_exceeded(59, 60));
    }

    #[test]
    fn test_poll_count_exceeded_false_when_count_zero() {
        assert!(!poll_count_exceeded(0, 60));
    }

    #[test]
    fn test_poll_count_exceeded_with_max_one() {
        assert!(poll_count_exceeded(1, 1));
        assert!(!poll_count_exceeded(0, 1));
    }

    // ========== calculate_completion_rate tests ==========

    #[test]
    fn test_completion_rate_empty_task_ids_returns_one() {
        let tasks = vec![];
        let task_ids: Vec<Uuid> = vec![];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_all_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Completed),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_none_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Queued),
            make_test_task(id2, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.0);
    }

    #[test]
    fn test_completion_rate_half_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.5);
    }

    #[test]
    fn test_completion_rate_counts_failed_as_completed() {
        let id1 = Uuid::new_v4();
        let tasks = vec![make_test_task(id1, TaskStatus::Failed)];
        let task_ids = vec![id1];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_counts_cancelled_as_completed() {
        let id1 = Uuid::new_v4();
        let tasks = vec![make_test_task(id1, TaskStatus::Cancelled)];
        let task_ids = vec![id1];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_mixed_statuses() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let id4 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Failed),
            make_test_task(id3, TaskStatus::Cancelled),
            make_test_task(id4, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2, id3, id4];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.75);
    }

    // ========== calculate_next_interval tests ==========

    #[test]
    fn test_next_interval_no_progress_uses_rate_based() {
        let interval = calculate_next_interval(0.5, 0.5, 1000, 500, 2000);
        // rate_based = 500 + (1500 * 0.5) = 1250
        assert_eq!(interval, 1250);
    }

    #[test]
    fn test_next_interval_positive_progress_increases() {
        let interval = calculate_next_interval(0.6, 0.5, 1000, 500, 2000);
        // progress > 0: max(1000 * 1.2, rate_based) = max(1200, 500 + 1500*0.6) = max(1200, 1400) = 1400
        assert!(interval >= 1000);
        assert!(interval <= 2000);
    }

    #[test]
    fn test_next_interval_negative_progress_decreases() {
        let interval = calculate_next_interval(0.4, 0.5, 1500, 500, 2000);
        // progress < 0: min(1500 * 0.8, rate_based) = min(1200, 500 + 1500*0.4) = min(1200, 1100) = 1100
        assert!(interval <= 1500);
        assert!(interval >= 500);
    }

    #[test]
    fn test_next_interval_clamped_to_min() {
        let interval = calculate_next_interval(0.0, 0.0, 500, 500, 2000);
        assert!(interval >= 500);
    }

    #[test]
    fn test_next_interval_clamped_to_max() {
        let interval = calculate_next_interval(1.0, 1.0, 2000, 500, 2000);
        assert!(interval <= 2000);
    }

    #[test]
    fn test_next_interval_full_completion() {
        let interval = calculate_next_interval(1.0, 0.0, 500, 500, 2000);
        // rate_based = 500 + 1500*1.0 = 2000
        assert_eq!(interval, 2000);
    }

    // ========== apply_defaults tests ==========

    #[test]
    fn test_apply_defaults_all_none() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: None,
            offset: None,
            include_results: None,
            sync_wait_ms: None,
        };
        let (limit, offset, include_results, sync_wait_ms) = apply_defaults(&request);
        assert_eq!(limit, server_config::DEFAULT_PAGE_LIMIT);
        assert_eq!(offset, 0);
        assert!(!include_results);
        assert_eq!(sync_wait_ms, crawl_task::DEFAULT_TIMEOUT_MS as u32);
    }

    #[test]
    fn test_apply_defaults_with_values() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: Some(50),
            offset: Some(100),
            include_results: Some(true),
            sync_wait_ms: Some(10000),
        };
        let (limit, offset, include_results, sync_wait_ms) = apply_defaults(&request);
        assert_eq!(limit, 50);
        assert_eq!(offset, 100);
        assert!(include_results);
        assert_eq!(sync_wait_ms, 10000);
    }

    #[test]
    fn test_apply_defaults_limit_capped_at_max() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: Some(5000),
            offset: None,
            include_results: None,
            sync_wait_ms: None,
        };
        let (limit, _, _, _) = apply_defaults(&request);
        assert_eq!(limit, server_config::MAX_PAGE_LIMIT);
    }

    // ========== determine_sync_status tests ==========

    #[test]
    fn test_determine_sync_status_async_mode() {
        assert_eq!(determine_sync_status(false, 0, 5000), "async");
        assert_eq!(determine_sync_status(false, 100, 5000), "async");
    }

    #[test]
    fn test_determine_sync_status_timeout() {
        assert_eq!(determine_sync_status(true, 5000, 5000), "sync_timeout");
        assert_eq!(determine_sync_status(true, 6000, 5000), "sync_timeout");
    }

    #[test]
    fn test_determine_sync_status_completed() {
        assert_eq!(determine_sync_status(true, 3000, 5000), "sync_completed");
        assert_eq!(determine_sync_status(true, 0, 5000), "sync_completed");
        assert_eq!(determine_sync_status(true, 4999, 5000), "sync_completed");
    }

    // ========== build_task_infos tests ==========

    #[test]
    fn test_build_task_infos_empty() {
        let tasks: Vec<Task> = vec![];
        let result = build_task_infos(&tasks, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_task_infos_single_task_no_results() {
        let id = Uuid::new_v4();
        let tasks = vec![make_test_task(id, TaskStatus::Completed)];
        let result = build_task_infos(&tasks, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, id);
        assert_eq!(result[0].status, TaskStatus::Completed);
        assert_eq!(result[0].url, "https://example.com");
        assert!(result[0].result.is_none());
    }

    #[test]
    fn test_build_task_infos_multiple_tasks() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Queued),
            make_test_task(id2, TaskStatus::Failed),
        ];
        let result = build_task_infos(&tasks, None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, id1);
        assert_eq!(result[1].id, id2);
    }

    #[test]
    fn test_build_task_infos_preserves_task_type() {
        let id = Uuid::new_v4();
        let mut task = make_test_task(id, TaskStatus::Queued);
        task.task_type = TaskType::Crawl;
        let result = build_task_infos(&[task], None);
        assert_eq!(result[0].task_type, TaskType::Crawl);
    }

    // ========== SyncWaitResult struct tests ==========

    #[test]
    fn test_sync_wait_result_no_wait() {
        let result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    #[test]
    fn test_sync_wait_result_timeout() {
        let result = SyncWaitResult {
            waited_time_ms: 5000,
            is_timeout: true,
        };
        assert_eq!(result.waited_time_ms, 5000);
        assert!(result.is_timeout);
    }

    #[test]
    fn test_sync_wait_result_completed_before_timeout() {
        let result = SyncWaitResult {
            waited_time_ms: 3000,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 3000);
        assert!(!result.is_timeout);
    }

    // ========== TaskQueryResponseMeta serialization ==========

    #[test]
    fn test_task_query_response_meta_serialization() {
        let meta = TaskQueryResponseMeta {
            status: "sync_completed".to_string(),
            credits_used: 5,
            response_time_ms: 1234,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "sync_completed");
        assert_eq!(parsed["credits_used"], 5);
        assert_eq!(parsed["response_time_ms"], 1234);
    }

    #[test]
    fn test_task_query_response_meta_async_status() {
        let meta = TaskQueryResponseMeta {
            status: "async".to_string(),
            credits_used: 0,
            response_time_ms: 0,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "async");
    }

    #[test]
    fn test_task_query_response_meta_timeout_status() {
        let meta = TaskQueryResponseMeta {
            status: "sync_timeout".to_string(),
            credits_used: 10,
            response_time_ms: 30000,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "sync_timeout");
    }

    // ========== validate_request tests ==========

    #[test]
    fn test_validate_request_valid() {
        let request = TaskQueryRequestDto::default();
        assert!(validate_request(&request).is_ok());
    }

    #[test]
    fn test_validate_request_limit_too_small() {
        let request = TaskQueryRequestDto {
            limit: Some(0),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_limit_too_large() {
        let request = TaskQueryRequestDto {
            limit: Some(1001),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_exceeds_max() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30001),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_zero_ok() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_ok());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_at_max_ok() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30000),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_ok());
    }

    // ========== handle_sync_wait_and_get_status edge cases ==========

    #[tokio::test]
    async fn test_handle_sync_wait_zero_ms_returns_immediately() {
        // This test verifies that sync_wait_ms=0 returns immediately without calling the repo
        // We use a dummy that would fail if called, but since sync_wait_ms=0, it won't be called
        struct DummyRepo;
        #[async_trait::async_trait]
        impl TaskRepository for DummyRepo {
            async fn create(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_id(
                &self,
                _id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn update(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn acquire_next(
                &self,
                _worker_id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_completed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_failed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_cancelled(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn exists_by_url(
                &self,
                _url: &str,
            ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_existing_urls(
                &self,
                _urls: &[String],
            ) -> Result<
                std::collections::HashSet<String>,
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn reset_stuck_tasks(
                &self,
                _timeout: chrono::Duration,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn cancel_tasks_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn expire_tasks(
                &self,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<Vec<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn query_tasks(
                &self,
                _params: crate::domain::repositories::task_repository::TaskQueryParams,
            ) -> Result<
                (Vec<Task>, u64),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn batch_cancel(
                &self,
                _task_ids: Vec<Uuid>,
                _team_id: Uuid,
                _force: bool,
            ) -> Result<
                (Vec<Uuid>, Vec<(Uuid, String)>),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
        }

        let result = handle_sync_wait_and_get_status(&DummyRepo, &[], Uuid::nil(), 0).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    #[tokio::test]
    async fn test_handle_sync_wait_empty_task_ids_returns_immediately() {
        // Even with sync_wait_ms > 0, empty task_ids should return immediately
        struct DummyRepo;
        #[async_trait::async_trait]
        impl TaskRepository for DummyRepo {
            async fn create(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_id(
                &self,
                _id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn update(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn acquire_next(
                &self,
                _worker_id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_completed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_failed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_cancelled(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn exists_by_url(
                &self,
                _url: &str,
            ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_existing_urls(
                &self,
                _urls: &[String],
            ) -> Result<
                std::collections::HashSet<String>,
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn reset_stuck_tasks(
                &self,
                _timeout: chrono::Duration,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn cancel_tasks_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn expire_tasks(
                &self,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<Vec<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn query_tasks(
                &self,
                _params: crate::domain::repositories::task_repository::TaskQueryParams,
            ) -> Result<
                (Vec<Task>, u64),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn batch_cancel(
                &self,
                _task_ids: Vec<Uuid>,
                _team_id: Uuid,
                _force: bool,
            ) -> Result<
                (Vec<Uuid>, Vec<(Uuid, String)>),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
        }

        let result = handle_sync_wait_and_get_status(&DummyRepo, &[], Uuid::nil(), 5000).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    // ========== build_scrape_result_json tests ==========

    // 构造测试用 ScrapeResult
    fn make_test_scrape_result(
        task_id: Uuid,
    ) -> crate::domain::models::scrape_result::ScrapeResult {
        crate::domain::models::scrape_result_entity::Model {
            id: Uuid::new_v4(),
            task_id,
            url: "https://example.com".to_string(),
            status_code: 200,
            content: "<html><body>Hello</body></html>".to_string(),
            content_type: "text/html".to_string(),
            headers: serde_json::json!({"content-length": "100"}),
            meta_data: serde_json::json!({"key": "value"}),
            screenshot: None,
            response_time_ms: 150,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn test_build_scrape_result_json_maps_basic_fields() {
        let task_id = Uuid::new_v4();
        let result = make_test_scrape_result(task_id);
        let dto = build_scrape_result_json(&result);

        assert_eq!(dto.id, result.id);
        assert_eq!(dto.status_code, 200);
        // content 经过 html_escape::encode_text 转义
        assert_eq!(
            dto.content,
            "&lt;html&gt;&lt;body&gt;Hello&lt;/body&gt;&lt;/html&gt;"
        );
    }

    #[test]
    fn test_build_scrape_result_json_escapes_html_special_chars() {
        // html_escape::encode_text 应转义 < > & ' "
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = "<script>alert('xss')</script>".to_string();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.contains("&lt;script&gt;"));
        assert!(!dto.content.contains("<script>"));
    }

    #[test]
    fn test_build_scrape_result_json_escapes_ampersand() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = "Tom & Jerry".to_string();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.contains("&amp;"));
        assert!(!dto.content.contains(" & "));
    }

    #[test]
    fn test_build_scrape_result_json_clones_metadata() {
        let task_id = Uuid::new_v4();
        let result = make_test_scrape_result(task_id);
        let dto = build_scrape_result_json(&result);

        assert!(dto.metadata.is_some());
        let metadata = dto.metadata.unwrap();
        assert_eq!(metadata["key"], "value");
    }

    #[test]
    fn test_build_scrape_result_json_status_code_404() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.status_code = 404;
        let dto = build_scrape_result_json(&result);

        assert_eq!(dto.status_code, 404);
    }

    #[test]
    fn test_build_scrape_result_json_empty_content() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = String::new();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.is_empty());
    }

    #[test]
    fn test_build_scrape_result_json_null_metadata() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.meta_data = serde_json::Value::Null;
        let dto = build_scrape_result_json(&result);

        assert!(dto.metadata.is_some());
        assert!(dto.metadata.unwrap().is_null());
    }

    // ========== build_task_infos with results_map tests ==========

    #[test]
    fn test_build_task_infos_with_matching_result() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let scrape_result = make_test_scrape_result(task_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task_id, scrape_result.clone());

        let infos = build_task_infos(&[task], Some(&results_map));

        assert_eq!(infos.len(), 1);
        assert!(infos[0].result.is_some());
        let result_dto = infos[0].result.as_ref().unwrap();
        assert_eq!(result_dto.id, scrape_result.id);
        assert_eq!(result_dto.status_code, 200);
    }

    #[test]
    fn test_build_task_infos_with_results_map_no_match() {
        // Task has no corresponding result in the map
        let task_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let scrape_result = make_test_scrape_result(other_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(other_id, scrape_result);

        let infos = build_task_infos(&[task], Some(&results_map));

        assert_eq!(infos.len(), 1);
        assert!(infos[0].result.is_none());
    }

    #[test]
    fn test_build_task_infos_mixed_with_and_without_results() {
        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();
        let tasks = vec![
            make_test_task(task1_id, TaskStatus::Completed),
            make_test_task(task2_id, TaskStatus::Queued),
        ];
        let scrape_result = make_test_scrape_result(task1_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task1_id, scrape_result);

        let infos = build_task_infos(&tasks, Some(&results_map));

        assert_eq!(infos.len(), 2);
        assert!(infos[0].result.is_some());
        assert!(infos[1].result.is_none());
    }

    #[test]
    fn test_build_task_infos_empty_map_returns_none_for_all() {
        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();
        let tasks = vec![
            make_test_task(task1_id, TaskStatus::Completed),
            make_test_task(task2_id, TaskStatus::Failed),
        ];
        let results_map: std::collections::HashMap<
            Uuid,
            crate::domain::models::scrape_result::ScrapeResult,
        > = std::collections::HashMap::new();

        let infos = build_task_infos(&tasks, Some(&results_map));

        assert_eq!(infos.len(), 2);
        assert!(infos[0].result.is_none());
        assert!(infos[1].result.is_none());
    }

    #[test]
    fn test_build_task_infos_result_html_escaped_in_dto() {
        // 验证 results_map 中的 content 经过 HTML 转义后出现在 TaskInfoDto 中
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let mut scrape_result = make_test_scrape_result(task_id);
        scrape_result.content = "<b>bold</b>".to_string();
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task_id, scrape_result);

        let infos = build_task_infos(&[task], Some(&results_map));

        let result_dto = infos[0].result.as_ref().expect("result should exist");
        assert!(result_dto.content.contains("&lt;b&gt;"));
        assert!(!result_dto.content.contains("<b>"));
    }

    // ========== Handler test infrastructure ==========

    /// Construct a lazy `DbPool` that does not connect to any database.
    ///
    /// `DbPool::try_from` is lazy: it builds the internal struct without opening
    /// a connection. The connection is only established on `get_session()`, which
    /// handlers under test never call (they only read `team_id` / `api_key_id`
    /// from `AuthState`). Since `try_from` internally calls
    /// `Handle::current().block_on(...)`, we construct the pool on a dedicated
    /// OS thread to avoid runtime-in-runtime panics.
    fn make_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                DbPool::try_from(&DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    /// Build an `AuthState` suitable for handler unit tests.
    fn make_test_auth_state() -> AuthState {
        AuthState::new(
            make_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    /// Build a `ScrapeResultRepositoryImpl` backed by a lazy (non-connecting) pool.
    fn make_test_scrape_result_repo() -> Arc<ScrapeResultRepositoryImpl> {
        Arc::new(ScrapeResultRepositoryImpl::new(make_test_db_pool()))
    }

    // ========== MockTaskRepository ==========

    /// Mock `TaskRepository` with configurable `query_tasks` and `batch_cancel`.
    ///
    /// All other trait methods return benign defaults. `query_tasks` returns the
    /// stored data on each call (cloned), or returns a stored error once (consumed).
    /// `batch_cancel` returns the stored result once (consumed), then empty.
    struct MockTaskRepository {
        query_error: Mutex<Option<RepositoryError>>,
        query_tasks_data: Mutex<Vec<Task>>,
        query_total: u64,
        batch_cancel_result:
            Mutex<Option<Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>>>,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_query_data(tasks: Vec<Task>, total: u64) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(tasks),
                query_total: total,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_query_error(err: RepositoryError) -> Self {
            Self {
                query_error: Mutex::new(Some(err)),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_batch_cancel_result(
            result: Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>,
        ) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(Some(result)),
            }
        }

        fn with_batch_cancel_result_and_query_data(
            result: Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>,
            query_tasks: Vec<Task>,
        ) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(query_tasks),
                query_total: 1,
                batch_cancel_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            unreachable!("create not expected in task_handler tests")
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            unreachable!("update not expected in task_handler tests")
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
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(Vec::new())
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            if let Some(err) = self.query_error.lock().unwrap().take() {
                return Err(err);
            }
            Ok((
                self.query_tasks_data.lock().unwrap().clone(),
                self.query_total,
            ))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            match self.batch_cancel_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok((Vec::new(), Vec::new())),
            }
        }
    }

    // ========== query_tasks handler tests ==========

    #[tokio::test]
    async fn test_query_tasks_handler_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_ok(), "query_tasks should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.total, 1);
        assert!(!data.has_more);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_empty_result() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "query_tasks should succeed with empty results"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 0);
        assert_eq!(data.total, 0);
        assert!(!data.has_more);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_has_more() {
        let task1 = make_test_task(Uuid::new_v4(), TaskStatus::Completed);
        let task2 = make_test_task(Uuid::new_v4(), TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task1, task2], 10));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            limit: Some(2),
            offset: Some(0),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 2);
        assert_eq!(data.total, 10);
        assert!(
            data.has_more,
            "has_more should be true when total > offset+limit"
        );
    }

    #[tokio::test]
    async fn test_query_tasks_handler_validation_error_limit() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            limit: Some(0),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "limit=0 should fail validation");
    }

    #[tokio::test]
    async fn test_query_tasks_handler_validation_error_sync_wait() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30001),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "sync_wait_ms=30001 should fail validation");
    }

    #[tokio::test]
    async fn test_query_tasks_handler_repo_error() {
        let repo = Arc::new(MockTaskRepository::with_query_error(
            RepositoryError::Database(anyhow::anyhow!("query failed")),
        ));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "repo error should propagate");
        match result.unwrap_err() {
            AppError::Other(msg) => assert!(msg.contains("Query failed")),
            other => panic!("expected AppError::Other, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_query_tasks_handler_sync_wait_completed() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(100),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "sync wait with completed tasks should succeed"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_include_results_db_error() {
        // With include_results=true and a lazy (non-connecting) pool,
        // fetch_scrape_results should fail because the pool cannot connect.
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            include_results: Some(true),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_err(),
            "include_results with lazy pool should fail"
        );
    }

    // ========== cancel_tasks handler tests ==========

    #[tokio::test]
    async fn test_cancel_tasks_handler_success() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id],
            vec![],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "cancel_tasks should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
        assert_eq!(data.total_failed, 0);
        assert_eq!(data.cancelled_tasks.len(), 1);
        assert_eq!(data.cancelled_tasks[0].task_id, task_id);
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_empty_task_ids() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "empty task_ids should fail");
        match result.unwrap_err() {
            AppError::Validation(msg) => {
                assert!(msg.contains("Task IDs cannot be empty"), "got: {}", msg);
            }
            other => panic!("expected AppError::Validation, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_validation_error_sync_wait() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![Uuid::new_v4()],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(30001),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "sync_wait_ms=30001 should fail validation");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_repo_error() {
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Err(
            RepositoryError::Database(anyhow::anyhow!("batch_cancel failed")),
        )));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![Uuid::new_v4()],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "repo error should propagate");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_with_failed_tasks() {
        let task_id1 = Uuid::new_v4();
        let task_id2 = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id1],
            vec![(task_id2, "Already completed".to_string())],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id1, task_id2],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
        assert_eq!(data.total_failed, 1);
        assert_eq!(data.failed_tasks[0].task_id, task_id2);
        assert_eq!(data.failed_tasks[0].reason, "Already completed");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_sync_wait() {
        let task_id = Uuid::new_v4();
        let cancelled_task = make_test_task(task_id, TaskStatus::Cancelled);
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result_and_query_data(
            Ok((vec![task_id], vec![])),
            vec![cancelled_task],
        ));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(100),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "cancel with sync wait should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
    }

    // ========== wait_for_tasks_completion tests ==========

    #[tokio::test]
    async fn test_wait_for_tasks_completion_already_completed() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 100, 500).await;

        assert!(
            result.is_ok(),
            "should complete immediately when tasks are done"
        );
    }

    #[tokio::test]
    async fn test_wait_for_tasks_completion_timeout() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Active);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 50, 500).await;

        assert!(result.is_ok(), "should return Ok on timeout");
    }

    #[tokio::test]
    async fn test_wait_for_tasks_completion_query_error() {
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll query failed"),
        ));

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 100, 500).await;

        assert!(result.is_err(), "query error should propagate");
    }

    // ========== handle_sync_wait_and_get_status tests ==========

    #[tokio::test]
    async fn test_handle_sync_wait_and_get_status_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = handle_sync_wait_and_get_status(&repo, &[task_id], Uuid::nil(), 200).await;

        assert!(result.is_ok());
        let sync_result = result.unwrap();
        assert!(!sync_result.is_timeout, "should complete before timeout");
    }

    #[tokio::test]
    async fn test_handle_sync_wait_and_get_status_error_continues() {
        // Even when wait_for_tasks_completion returns an error,
        // handle_sync_wait_and_get_status catches it and returns Ok.
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll failed"),
        ));

        let result = handle_sync_wait_and_get_status(&repo, &[task_id], Uuid::nil(), 200).await;

        assert!(result.is_ok(), "should return Ok even on wait error");
    }

    // ========== Direct function tests ==========

    #[tokio::test]
    async fn test_query_tasks_for_poll_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = query_tasks_for_poll(&repo, Uuid::nil(), &[task_id]).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_query_tasks_for_poll_error() {
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll failed"),
        ));

        let result = query_tasks_for_poll(&repo, Uuid::nil(), &[task_id]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_task_query_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);
        let request = TaskQueryRequestDto::default();

        let result = execute_task_query(&repo, Uuid::nil(), &request, 100, 0).await;

        assert!(result.is_ok());
        let (tasks, total) = result.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(total, 1);
    }

    #[tokio::test]
    async fn test_execute_task_query_error() {
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("exec failed"),
        ));
        let request = TaskQueryRequestDto::default();

        let result = execute_task_query(&repo, Uuid::nil(), &request, 100, 0).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::Other(msg) => assert!(msg.contains("Query failed")),
            other => panic!("expected AppError::Other, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handle_sync_wait_direct() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task.clone()], 1);

        let result = handle_sync_wait(&repo, &[task], Uuid::nil(), 100).await;

        assert!(result.is_ok());
        let waited = result.unwrap();
        assert!(
            waited < 1000,
            "waited_time_ms should be small, got {}",
            waited
        );
    }

    #[tokio::test]
    async fn test_fetch_scrape_results_empty_tasks() {
        let repo = make_test_scrape_result_repo();
        let tasks: Vec<Task> = vec![];

        let result = fetch_scrape_results(repo.as_ref(), &tasks).await;

        assert!(result.is_ok());
        let map = result.unwrap();
        assert!(map.is_some());
        assert!(map.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_fetch_scrape_results_non_empty_no_db() {
        // With a lazy (non-connecting) pool, calling find_by_task_ids on
        // non-empty task IDs should fail because the pool cannot connect.
        let repo = make_test_scrape_result_repo();
        let task_id = Uuid::new_v4();
        let tasks = vec![make_test_task(task_id, TaskStatus::Completed)];

        let result = fetch_scrape_results(repo.as_ref(), &tasks).await;

        assert!(result.is_err(), "should fail without a real DB connection");
    }
}
