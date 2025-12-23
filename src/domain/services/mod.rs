// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 领域服务模块
///
/// 该模块包含系统的核心业务逻辑服务，这些服务封装了复杂的
/// 业务规则和领域逻辑，协调多个领域对象来完成业务操作。
///
/// 包含的服务：
/// - 爬取服务（crawl_service）：处理爬取任务的调度和执行逻辑
/// - 提取服务（extraction_service）：处理内容提取和数据解析逻辑
/// - LLM服务（llm_service）：集成大语言模型进行智能处理
/// - 抓取服务（scrape_service）：处理单个网页的抓取逻辑
/// - 搜索服务（search_service）：处理内容搜索和索引逻辑
/// - 团队服务（team_service）：处理团队管理和地理限制验证逻辑
///
/// 领域服务与应用程序服务的区别在于：领域服务包含纯粹的业务逻辑，
/// 而应用程序服务负责协调和编排，可能包含技术实现细节。
pub mod crawl_service;
pub mod extraction_service;
pub mod llm_service;
pub mod rate_limiting_service;
pub mod relevance_scorer;
pub mod scrape_service;
pub mod search_service;
pub mod team_service;
pub mod webhook_service;
