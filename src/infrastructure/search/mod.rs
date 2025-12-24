// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

pub mod ab_test;
pub mod aggregator;
pub mod baidu;
pub mod bing;
/// 搜索服务模块
///
/// 提供各种搜索引擎的集成实现
/// 包括Google、Bing、百度、搜狗等搜索引擎的API客户端
/// 以及搜索结果聚合器
pub mod enhanced_aggregator;
pub mod factory;
pub mod google;
pub mod search_engine_router;
pub mod smart_search;
pub mod sogou;

pub use factory::{create_default_router, SearchEngineFactory, SearchEngineFactoryConfig};
pub use search_engine_router::{
    SearchEngineRouter, SearchEngineRouterConfig, SmartSearchEngineWrapper,
};
