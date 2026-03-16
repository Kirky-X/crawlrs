// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod audit_service;
/// 领域服务模块
///
/// 该模块包含系统的核心业务逻辑服务，这些服务封装了复杂的
/// 业务规则和领域逻辑，协调多个领域对象来完成业务操作。
///
/// 包含的服务：
/// - 认证范围服务（auth_scope_service）：处理 API Key 权限范围管理
/// - 审计服务（audit_service）：处理认证和授权决策的审计日志
/// - 并发控制器（concurrency_controller）：提供统一的并发控制接口
/// - 爬取服务（crawl_service）：处理爬取任务的调度和执行逻辑
/// - 积分服务（credits_service）：处理爬取积分的扣减和管理逻辑
/// - 提取服务（extraction_service）：处理内容提取和数据解析逻辑
/// - 提取工具（extraction_utils）：消除提取逻辑重复的共享工具函数
/// - 功能标志服务（feature_flag_service）：处理运行时功能开关
/// - 地理位置服务（geo_location）：提供IP地址地理位置查询的抽象接口
/// - LLM服务（llm_service）：集成大语言模型进行智能处理
/// - 重试处理器（retry_handler）：处理任务失败的重试逻辑
/// - 抓取服务（scrape_service）：处理单个网页的抓取逻辑
/// - 搜索服务（search_service）：处理内容搜索和索引逻辑
/// - 团队服务（team_service）：处理团队管理和地理限制验证逻辑
/// - 限流服务（rate_limiting_service）：处理请求限流逻辑
/// - Webhook服务（webhook_service）：处理 Webhook 通知逻辑
/// - 测试工具（test_helpers）：提供可复用的测试夹具和模拟对象
///
/// 领域服务与应用程序服务的区别在于：领域服务包含纯粹的业务逻辑，
/// 而应用程序服务负责协调和编排，可能包含技术实现细节。
pub mod auth_scope_service;
pub mod concurrency_controller;
pub mod crawl_service;
pub mod credits_service;
pub mod extraction_service;
pub mod extraction_utils;
pub mod feature_flag_service;
pub mod geo_location;
pub mod llm_service;
pub mod rate_limiting_service;
pub mod relevance_scorer;
pub mod retry_handler;
pub mod scrape_service;
pub mod search_service;
pub mod team_service;
/// Test utilities for domain services
///
/// This module provides reusable test fixtures and mock objects
/// to eliminate code duplication across service tests.
#[cfg(test)]
pub mod test_helpers;
pub mod webhook_sender;
pub mod webhook_service;
