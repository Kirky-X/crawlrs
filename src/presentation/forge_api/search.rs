// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Search 路由的 sdforge 直注注册（design D1′）。
//!
//! handler 原样提交：URL、方法、提取器与迁移前的手写
//! `.route("/v1/search", post(search_handler::search))` 逐字节等价。

use axum::routing::post;
use inventory::submit;
use sdforge::prelude::{ApiMetadata, HttpRoute};

use crate::presentation::handlers::search_handler;
use crate::presentation::forge_api::route_metadata;

fn search_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/search".to_string(),
        post(search_handler::search),
        route_metadata("search", "v1", "Execute a search query"),
        None,
    )
}

fn search_metadata() -> ApiMetadata {
    route_metadata("search", "v1", "Execute a search query")
}

submit!(sdforge::prelude::RouteRegistration::new(
    "search",
    "v1",
    search_route,
    search_metadata,
));
