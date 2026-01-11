// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::presentation::middleware::team_semaphore::TeamSemaphore;

/// 从请求扩展中提取team_id
/// 认证中间件会将team_id注入到请求扩展中
fn extract_team_id(request: &Request) -> Option<uuid::Uuid> {
    request.extensions().get::<uuid::Uuid>().copied()
}

pub async fn team_semaphore_middleware(
    State(semaphore): State<Arc<TeamSemaphore>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 从请求扩展中获取team_id（由认证中间件注入）
    let team_id = extract_team_id(&request).ok_or_else(|| {
        tracing::warn!("No team_id found in request extensions - authentication may have failed");
        StatusCode::UNAUTHORIZED
    })?;

    // 使用真实的team_id获取并发许可
    let _permit = semaphore.acquire(team_id).await;
    Ok(next.run(request).await)
}
