// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl query handlers — GET crawl operations.

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::application::use_cases::crawl_use_case::CrawlUseCaseError;
use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::response_builder::{
    error_response, errors_locale, success_response,
};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;

/// Retrieve crawl details by ID.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
/// * `crawl_id` - ID of the crawl to retrieve.
///
/// # Errors
///
/// Returns 403 if the caller does not own the crawl, 404 if not found.
pub async fn get_crawl(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

    match use_case.get_crawl(crawl_id, team_id).await {
        Ok(Some(crawl)) => success_response(StatusCode::OK, crawl),
        Ok(None) => errors_locale::not_found(&locale, &bundle, "api-crawl-not-found"),
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

/// Retrieve scrape results for a given crawl.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
/// * `crawl_id` - ID of the crawl whose results to fetch.
///
/// # Errors
///
/// Returns 403 if the caller does not own the crawl, 404 if not found.
pub async fn get_crawl_results(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

    match use_case.get_crawl_results(crawl_id, team_id).await {
        Ok(results) => success_response(StatusCode::OK, results),
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
