// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 集成测试模块
///
/// 包含系统的端到端集成测试
/// 测试各个组件之间的交互和整体功能
pub mod api;
pub mod api_tests;
pub mod crawl_service_test;
pub mod health_check;
pub mod health_monitor_test;
pub mod helpers;
pub mod real_components_test;
pub mod real_interactions_test;
pub mod real_world_test;
pub mod repositories;
pub mod scheduler_test;
pub mod scrape_handler_test;
pub mod scrape_test;
pub mod search_engines_simple_test;
pub mod search_engines_test;
pub mod search_uat_test;
pub mod uat_scenarios_test;
pub mod webhook_test;
