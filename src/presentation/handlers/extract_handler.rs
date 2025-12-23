// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{ConnectInfo, Extension},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::net::SocketAddr;
use tracing::error;
use uuid::Uuid;

use crate::application::dto::extract_request::ExtractRequestDto;
use crate::config::settings::Settings;
use crate::domain::models::task::{Task, TaskType};
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::services::team_service::TeamService;
use crate::infrastructure::geolocation::GeoLocationService;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::handlers::task_handler::wait_for_tasks_completion;
use crate::queue::task_queue::TaskQueue;
use std::sync::Arc;

pub async fn extract<GR>(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_id): Extension<Uuid>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<ExtractRequestDto>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    // Validate the request
    if payload.urls.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "At least one URL is required"
            })),
        )
            .into_response();
    }

    if payload.prompt.is_none() && payload.schema.is_none() && payload.rules.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Either prompt, schema, or rules is required"
            })),
        )
            .into_response();
    }

    // 检查地理限制
    let client_ip = addr.ip().to_string();

    // 获取团队地理限制配置
    let restrictions = match geo_restriction_repo.get_team_restrictions(team_id).await {
        Ok(restrictions) => restrictions,
        Err(e) => {
            error!("Failed to get team restrictions: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to validate geographic access"
                })),
            )
                .into_response();
        }
    };

    // 使用团队服务验证地理限制
    let team_service = TeamService::new(GeoLocationService::new(), geo_restriction_repo.clone());
    match team_service
        .validate_geographic_restriction(team_id, &client_ip, &restrictions)
        .await
    {
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed) => {
            // 记录允许的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "ALLOWED",
                    "Extract request - Geographic restriction check passed",
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }
        }
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
            // 记录拒绝的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "DENIED",
                    &format!("Extract request - {}", reason),
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }

            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "success": false,
                    "error": format!("Access denied due to geographic restrictions: {}", reason)
                })),
            )
                .into_response();
        }
        Err(e) => {
            error!("Failed to validate geographic restrictions: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to validate geographic access"
                })),
            )
                .into_response();
        }
    }

    // Create a task for async extraction
    let task = Task::new(
        TaskType::Extract,
        team_id,
        payload.urls.first().unwrap().clone(), // Use first URL as primary
        serde_json::to_value(&payload).unwrap_or_default(),
    );

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 处理同步等待逻辑
            let sync_wait_ms = payload.sync_wait_ms.unwrap_or(5000);
            let mut waited_time_ms = 0u64;

            if sync_wait_ms > 0 {
                let wait_start = std::time::Instant::now();

                // 调用智能轮询等待函数
                match wait_for_tasks_completion(
                    task_repository.as_ref(),
                    &[task.id],
                    team_id,
                    sync_wait_ms,
                    1000, // 基础轮询间隔1秒
                )
                .await
                {
                    Ok(_) => {
                        waited_time_ms = wait_start.elapsed().as_millis() as u64;
                    }
                    Err(e) => {
                        error!("Failed to wait for task completion: {:?}", e);
                        // 即使等待失败，也返回已创建的任务信息
                    }
                }
            }

            let response = json!({
                "success": true,
                "id": task.id,
                "status": "pending"
            });

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 && waited_time_ms >= sync_wait_ms as u64 {
                StatusCode::ACCEPTED // 同步等待超时，任务已接受但可能未完成
            } else {
                StatusCode::CREATED // 任务已创建（可能已完成）
            };

            (status_code, Json(response)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": e.to_string()
            })),
        )
            .into_response(),
    }
}
