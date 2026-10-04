// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 路由分组模块

pub mod crawl_routes;
pub mod management_routes;
// map 路由组（F11 sitemap 流式站点图）：sitemap-off 时不编译不注册
#[cfg(feature = "sitemap")]
pub mod map_routes;
pub mod scrape_routes;
pub mod search_routes;
