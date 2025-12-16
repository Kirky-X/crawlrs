// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use axum::{
    extract::{Extension, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::presentation::middleware::team_semaphore::TeamSemaphore;

use crate::presentation::middleware::auth_middleware::AuthState;

pub async fn team_semaphore_middleware(
    State(semaphore): State<Arc<TeamSemaphore>>,
    Extension(user): Extension<AuthState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let _permit = semaphore.acquire(user.team_id).await;
    Ok(next.run(request).await)
}
