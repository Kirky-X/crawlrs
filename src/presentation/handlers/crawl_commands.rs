// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Crawl command handlers — POST/DELETE crawl mutation operations.

use axum::{
    extract::{ConnectInfo, Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::application::use_cases::crawl_use_case::CrawlUseCaseError;
use crate::common::constants::crawl_task::DEFAULT_TIMEOUT_MS;
use crate::config::settings::Settings;
use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::credits::{CreditCharge, CreditFlow};
use crate::presentation::handlers::response_builder::errors;
use crate::presentation::handlers::response_builder::{
    error_response, error_response_locale, errors_locale, json_rejection_response, success_response,
};
use crate::presentation::handlers::task_queries::handle_sync_wait_and_get_status;
use crate::presentation::handlers::task_queries::SyncWaitResult;
use crate::presentation::handlers::{check_ssrf_url, extract_task_ids, sync_wait_status_code};
use crate::presentation::helpers::rate_limit_helper::check_rate_limit;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;
use log::error;

/// F11：sitemap 流式请求源主开关校验
///
/// 请求携带 `sitemap_seed=true` 而能力不可用时必须**显式拒绝**（禁止静默
/// 忽略降级为普通 crawl）：
/// - sitemap feature 未编译：能力不存在（需 `--features sitemap` 重新构建）
/// - 主开关关闭（`CRAWLRS__SITEMAP__ENABLED=false`，默认）
///
/// 返回 `Some(FTL key)` 表示拒绝（文案经请求 locale 翻译，见 api.ftl），
/// `None` 表示放行（或请求未启用 sitemap 源）。
fn sitemap_seed_rejection(payload: &CrawlRequestDto, settings: &Settings) -> Option<&'static str> {
    // sitemap-off 组合下 settings 不参与判断（disabled_reason 恒为 feature-required）
    #[cfg(not(feature = "sitemap"))]
    let _ = settings;
    if payload.sitemap_seed != Some(true) {
        return None;
    }
    #[cfg(not(feature = "sitemap"))]
    let disabled_reason: Option<&'static str> = Some("api-sitemap-feature-required");
    #[cfg(feature = "sitemap")]
    let disabled_reason: Option<&'static str> = if settings.sitemap.enabled {
        None
    } else {
        Some("api-sitemap-source-disabled")
    };
    disabled_reason
}

/// Create a new crawl task.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state (use-case factory).
/// * `settings` - Application settings（sitemap 请求源主开关）.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources for error messages.
/// * `addr` - Client socket address (logging).
/// * `payload` - Crawl request DTO.
///
/// # Errors
///
/// Returns 400 for validation errors, 402 for insufficient credits,
/// 500 for internal errors.
pub async fn create_crawl(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(settings): Extension<Arc<Settings>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    payload: Result<Json<CrawlRequestDto>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    // 提取器级拒绝（非法 JSON / 字段缺失）映射为统一包封，避免纯文本响应
    let Json(payload) = match payload {
        Ok(parsed) => parsed,
        Err(ref rej) => return json_rejection_response(rej),
    };
    let team_id = auth_state.team_id;
    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);

    if let Some(response) =
        preflight_crawl(&state, &payload, &settings, &auth_state, &locale, &bundle).await
    {
        return response;
    }

    let use_case = state.create_use_case();

    let client_ip = addr.ip().to_string();
    // 先取出 URL：payload 会在 create_crawl 中被 move，退款日志还需要它
    let crawl_url = payload.url.clone();
    match use_case
        .create_crawl(team_id, auth_state.api_key_id, payload, &client_ip)
        .await
    {
        Ok(crawl) => {
            let wait_result =
                await_crawl_sync_wait(state.as_ref(), crawl.id, team_id, sync_wait_ms).await;

            let status_code = sync_wait_status_code(sync_wait_ms, wait_result.is_timeout);

            success_response(status_code, crawl)
        }
        Err(e) => {
            CreditFlow::new(state.rate_limiting_service.as_ref(), team_id)
                .refund(CreditCharge::Crawl { url: &crawl_url }, None)
                .await;
            crawl_error_response(e, &locale, &bundle)
        }
    }
}

/// 前置校验与计费：深度/sitemap 校验 → 限流 → SSRF → 配额扣减。
///
/// 返回 `Some(response)` 表示请求必须中止。
async fn preflight_crawl(
    state: &CrawlHandlerState,
    payload: &CrawlRequestDto,
    settings: &Settings,
    auth_state: &AuthState,
    locale: &Locale,
    bundle: &Arc<I18nBundle>,
) -> Option<Response> {
    if let Some(response) = validate_crawl_request(payload, settings, locale, bundle) {
        return Some(response);
    }

    // 1. 检查限流（架构限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    if let Err(response) = check_rate_limit(
        state.rate_limiting_service.as_ref(),
        auth_state.api_key_id,
        "/v1/crawl",
    )
    .await
    {
        return Some(response);
    }

    // 2. SSRF 验证 - 使用完整的异步 DNS 验证（目标 URL / proxy / sitemap_url）
    if let Some(response) =
        check_crawl_ssrf_targets(payload, auth_state.team_id, auth_state.api_key_id).await
    {
        return Some(response);
    }

    if let Some(response) =
        CreditFlow::new(state.rate_limiting_service.as_ref(), auth_state.team_id)
            .deduct(CreditCharge::Crawl { url: &payload.url })
            .await
    {
        return Some(response);
    }

    None
}

/// 将 create_crawl 错误映射为统一响应。
fn crawl_error_response(
    e: CrawlUseCaseError,
    locale: &Locale,
    bundle: &Arc<I18nBundle>,
) -> Response {
    match e {
        CrawlUseCaseError::NotFound => {
            errors_locale::not_found(locale, bundle, "api-crawl-not-found")
        }
        e => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 创建请求的前置校验：深度上限与 sitemap 请求源主开关。
///
/// F11：请求携带 `sitemap_seed=true` 而能力不可用时必须**显式拒绝**
/// （禁止静默忽略降级为普通 crawl）。
fn validate_crawl_request(
    payload: &CrawlRequestDto,
    settings: &Settings,
    locale: &Locale,
    bundle: &Arc<I18nBundle>,
) -> Option<Response> {
    // 审查 HIGH：tasks.url 的普通 btree 索引（idx_tasks_url）仍有行上限，
    // DTO 的 2048 字符对多字节 URL 可达 8 KiB。入队前按字节收敛。
    if payload.url.len() > 2048 {
        return Some(errors::unprocessable_entity(
            "url must be at most 2048 bytes",
        ));
    }

    if payload.config.max_depth > crate::application::dto::crawl_request::MAX_CRAWL_DEPTH {
        return Some(errors::unprocessable_entity(
            "max_depth must be between 0 and 5",
        ));
    }

    // D6 修复：max_concurrency 契约上限 50（此前 handler 层无校验，51 放行
    // 到 use_case 才以 400 报 100 上限——三层不一致，E2E SC-04 实证）。
    if let Some(concurrency) = payload.config.max_concurrency {
        if concurrency > crate::application::dto::crawl_request::MAX_CONCURRENCY {
            return Some(errors::unprocessable_entity(
                "max_concurrency must be between 1 and 50",
            ));
        }
    }

    // F11：sitemap 请求源主开关校验（能力不可用时显式拒绝，禁止静默忽略）
    if let Some(key) = sitemap_seed_rejection(payload, settings) {
        return Some(error_response_locale(
            StatusCode::UNPROCESSABLE_ENTITY,
            locale,
            bundle,
            key,
        ));
    }

    None
}

/// 2. SSRF 防护 (CWE-918)：目标 URL、config.proxy、显式 sitemap_url 依次校验。
async fn check_crawl_ssrf_targets(
    payload: &CrawlRequestDto,
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
) -> Option<Response> {
    if let Some(response) = check_ssrf_url(&payload.url, team_id, api_key_id).await {
        return Some(response);
    }

    // 2.5 SSRF 防护 (CWE-918)：验证 config.proxy 不指向内部网络
    if let Some(ref proxy_url) = payload.config.proxy {
        if let Some(response) = check_ssrf_url(proxy_url, team_id, api_key_id).await {
            return Some(response);
        }
    }

    // 2.6 SSRF 防护 (CWE-918)：显式 sitemap_url 是 worker 侧的拉取目标，
    // 与种子 URL 同级验证（纵深防御；引擎 router 侧另有执行前校验）
    if let Some(ref sitemap_url) = payload.sitemap_url {
        if let Some(response) = check_ssrf_url(sitemap_url, team_id, api_key_id).await {
            return Some(response);
        }
    }

    None
}

/// 创建成功后按 sync_wait_ms 轮询 crawl 子任务状态。
async fn await_crawl_sync_wait(
    state: &CrawlHandlerState,
    crawl_id: crate::domain::id::Id,
    team_id: crate::domain::id::Id,
    sync_wait_ms: u32,
) -> SyncWaitResult {
    if sync_wait_ms == 0 {
        return SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
    }

    match state.task_repo.find_by_crawl_id(crawl_id).await {
        Ok(tasks) => {
            if tasks.is_empty() {
                SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            } else {
                let task_ids = extract_task_ids(&tasks);
                handle_sync_wait_and_get_status(
                    state.task_repo.as_ref(),
                    &task_ids,
                    team_id,
                    sync_wait_ms,
                )
                .await
                .unwrap_or(SyncWaitResult {
                    waited_time_ms: sync_wait_ms as u64,
                    is_timeout: true,
                })
            }
        }
        Err(e) => {
            error!("Failed to find tasks for crawl {}: {:?}", crawl_id, e);
            SyncWaitResult {
                waited_time_ms: 0,
                is_timeout: false,
            }
        }
    }
}

/// Cancel an in-progress crawl task.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
/// * `crawl_id` - ID of the crawl to cancel.
///
/// # Errors
///
/// Returns 403 if the caller does not own the crawl, 404 if not found,
/// 500 for internal errors.
pub async fn cancel_crawl(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    Path(crawl_id): Path<crate::domain::id::Id>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => match e {
            CrawlUseCaseError::NotFound => {
                errors_locale::not_found(&locale, &bundle, "api-crawl-not-found")
            }
            e => {
                let (status, msg): (StatusCode, String) = e.into();
                error_response(status, msg)
            }
        },
    }
}

impl From<CrawlUseCaseError> for (StatusCode, String) {
    fn from(err: CrawlUseCaseError) -> Self {
        match err {
            CrawlUseCaseError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg),
            CrawlUseCaseError::Repository(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
            CrawlUseCaseError::NotFound => (StatusCode::NOT_FOUND, "Crawl not found".to_string()),
            CrawlUseCaseError::Anyhow(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
        }
    }
}
