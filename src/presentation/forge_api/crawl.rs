// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl 路由的 sdforge 注册。
//!
//! - `POST /v1/crawl`：直注（带 JSON body + ConnectInfo，handler 原样组装；
//!   ConnectInfo 由 into_make_service_with_connect_info 在请求层供给）
//! - `GET /v1/crawl/{id}`：直注（多方法路径——与 DELETE 同路径，必须单条目注册）
//! - `GET /v1/crawl/{id}/results`：`#[forge]` stream 透传 wrapper（无 body）

use axum::{Extension, response::Response};
use sdforge::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::crawl_handler;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;
use crate::presentation::forge_api::route_metadata;
use inventory::submit;

fn create_crawl_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/crawl".to_string(),
        axum::routing::post(crawl_handler::create_crawl),
        route_metadata("crawl_create", "v1", "Create a crawl task"),
        None,
    )
}

fn create_crawl_metadata() -> ApiMetadata {
    route_metadata("crawl_create", "v1", "Create a crawl task")
}

submit!(RouteRegistration::new(
    "crawl_create",
    "v1",
    create_crawl_route,
    create_crawl_metadata,
));

// GET+DELETE 同路径多方法：单条 RouteRegistration 直注，
// 规避 build() 按路径去重的覆盖丢失问题（design D2）。
fn crawl_by_id_route() -> HttpRoute {
    let method_router = axum::routing::get(crawl_handler::get_crawl)
        .delete(crawl_handler::cancel_crawl);
    HttpRoute::new(
        "/v1/crawl/{id}".to_string(),
        method_router,
        route_metadata("crawl_by_id", "v1", "Get or cancel a crawl task"),
        None,
    )
}

fn crawl_by_id_metadata() -> ApiMetadata {
    route_metadata("crawl_by_id", "v1", "Get or cancel a crawl task")
}

submit!(RouteRegistration::new(
    "crawl_by_id",
    "v1",
    crawl_by_id_route,
    crawl_by_id_metadata,
));

#[forge(
    name = "crawl_results",
    version = "v1",
    path = "/v1/crawl/{id}/results",
    method = "GET",
    no_prefix = true,
    stream = true,
    description = "Get crawl task results"
)]
async fn crawl_results_route(
    crawl_id: Uuid,
    #[state] state: Arc<CrawlHandlerState>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] bundle: Arc<I18nBundle>,
) -> Result<Response, Response> {
    Ok(
        crawl_handler::get_crawl_results(
            Extension(state),
            Extension(auth_state),
            Extension(locale),
            Extension(bundle),
            axum::extract::Path(crawl_id),
        )
        .await
        .into_response(),
    )
}
