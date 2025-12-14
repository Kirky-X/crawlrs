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

use axum::{
    extract::{Extension, Request},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::{error, warn};

use crate::presentation::middleware::rate_limit_middleware::RateLimiter;

/// 分布式速率限制中间件
///
/// 基于API密钥应用分布式速率限制
///
/// # 参数
///
/// * `rate_limiter` - 速率限制器扩展
/// * `request` - HTTP请求
/// * `next` - 下一个中间件
///
/// # 返回值
///
/// * `Ok(impl IntoResponse)` - 处理成功的响应
/// * `Err(StatusCode)` - 处理失败的状态码
pub async fn distributed_rate_limit_middleware(
    Extension(rate_limiter): Extension<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    let api_key_id = request.extensions().get::<String>().cloned().ok_or_else(|| {
        error!("API Key not found in request extensions. Ensure AuthMiddleware is applied before DistributedRateLimitMiddleware.");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    rate_limiter.check(&api_key_id).await.map_err(|e| {
        warn!("Rate limit check failed for API Key {}: {}", api_key_id, e);
        StatusCode::TOO_MANY_REQUESTS
    })?;

    Ok(next.run(request).await)
}
