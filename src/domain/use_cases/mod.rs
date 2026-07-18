// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub mod crawl_use_cases;
/// 领域用例模块
///
/// 该模块包含具体的业务用例实现，每个用例代表一个完整的业务流程。
/// 领域用例协调领域对象和服务来完成特定的业务目标。
///
/// 当前的用例：
/// - 创建Webhook（create_webhook）：处理Webhook配置的创建流程
/// - 任务用例（task_use_cases）：任务创建、查询、取消
/// - 爬取用例（crawl_use_cases）：异步和同步爬取操作
/// - 抓取用例（scrape_use_cases）：异步和同步抓取操作
///
/// 领域用例与应用程序用例的区别在于：领域用例包含纯粹的业务逻辑，
/// 关注业务规则的实现，而应用程序用例可能包含更多的技术细节和协调逻辑。
pub mod create_webhook;
pub mod scrape_use_cases;
pub mod task_use_cases;
