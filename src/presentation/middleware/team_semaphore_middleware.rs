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

pub async fn team_semaphore_middleware(
    State(semaphore): State<Arc<TeamSemaphore>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // ⚠️ SECURITY WARNING: Using nil UUID - all requests share the same concurrency slot
    // In production, extract team_id from JWT token or API key
    let team_id = uuid::Uuid::nil();
    let _permit = semaphore.acquire(team_id).await;
    Ok(next.run(request).await)
}
