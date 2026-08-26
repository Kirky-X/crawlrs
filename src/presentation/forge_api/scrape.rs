// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape 路由的 sdforge 注册。
//!
//! - `POST /v1/scrape`：直注（design D1′，带 JSON body，handler 原样组装）
//! - `GET /v1/scrape/{id}`：`#[forge]` stream 透传 wrapper（无 body，Path+State
//!   参数模型与宏匹配；Ok/Err 两臂各做一次 into_response，响应零改写）

use axum::{Extension, response::Response};
use sdforge::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::scrape_handler;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::forge_api::route_metadata;
use inventory::submit;
use axum::routing::post;

fn create_scrape_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/scrape".to_string(),
        post(scrape_handler::create_scrape),
        route_metadata("scrape_create", "v1", "Create a scrape task"),
        None,
    )
}

fn create_scrape_metadata() -> ApiMetadata {
    route_metadata("scrape_create", "v1", "Create a scrape task")
}

submit!(RouteRegistration::new(
    "scrape_create",
    "v1",
    create_scrape_route,
    create_scrape_metadata,
));

#[forge(
    name = "scrape_status",
    version = "v1",
    path = "/v1/scrape/{id}",
    method = "GET",
    no_prefix = true,
    stream = true,
    description = "Get scrape task status"
)]
async fn scrape_status_route(
    id: Uuid,
    #[state] task_repo: Arc<dyn TaskRepository>,
    #[state] result_repo: Arc<dyn ScrapeResultRepository>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] bundle: Arc<I18nBundle>,
) -> Result<Response, Response> {
    Ok(
        scrape_handler::get_scrape_status(
            axum::extract::Path(id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth_state),
            Extension(locale),
            Extension(bundle),
        )
        .await
        .into_response(),
    )
}
