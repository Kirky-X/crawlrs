// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

pub mod crawl_repository;
/// 仓库接口模块
///
/// 该模块定义了领域层的仓库接口，遵循依赖倒置原则。
/// 仓库接口定义了数据持久化的抽象契约，具体实现由基础设施层提供。
///
/// 包含的仓库接口：
/// - 积分仓库（credits_repository）：管理团队的积分余额和交易记录
/// - 爬取任务仓库（crawl_repository）：管理爬取任务的持久化
/// - 爬取结果仓库（scrape_result_repository）：管理爬取结果的存储
/// - 地理限制仓库（geo_restriction_repository）：管理团队的地理限制配置
/// - 存储仓库（storage_repository）：管理文件和对象的存储
/// - 任务仓库（task_repository）：管理任务的调度和执行
/// - Webhook事件仓库（webhook_event_repository）：管理Webhook事件的发送
/// - Webhook仓库（webhook_repository）：管理Webhook配置
///
/// 这些接口确保了领域层不依赖于具体的数据存储技术，
/// 提高了系统的可测试性和可维护性.
pub mod credits_repository;
pub mod geo_restriction_repository;
pub mod scrape_result_repository;
pub mod storage_repository;
pub mod task_repository;
pub mod tasks_backlog_repository;
pub mod webhook_event_repository;
pub mod webhook_repository;
