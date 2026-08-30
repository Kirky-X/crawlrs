// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Map 路由的 sdforge 直注注册（bdd-acceptance-hardening R-map-004）。
//!
//! 形态与 `search.rs` 逐字节对齐：`HttpRoute` + `inventory::submit!`。

use axum::routing::post;
use inventory::submit;
use sdforge::prelude::{ApiMetadata, HttpRoute};

use crate::presentation::forge_api::route_metadata;
use crate::presentation::handlers::map_handler;

fn map_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/map".to_string(),
        post(map_handler::map),
        route_metadata("map", "v1", "Discover URLs from a site's sitemap"),
        None,
    )
}

fn map_metadata() -> ApiMetadata {
    route_metadata("map", "v1", "Discover URLs from a site's sitemap")
}

submit!(sdforge::prelude::RouteRegistration::new(
    "map",
    "v1",
    map_route,
    map_metadata,
));
