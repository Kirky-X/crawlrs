// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 领域模型模块
///
/// 该模块定义了系统的核心业务实体，包括：
/// - 爬取任务（crawl）：表示一个完整的爬取任务
/// - 爬取结果（scrape_result）：存储爬取到的数据结果
/// - 任务（task）：表示爬取任务中的单个执行单元
/// - 网络钩子（webhook）：用于异步通知的外部接口
///
/// 这些模型构成了系统的数据基础，定义了业务概念的
/// 结构和行为，是领域驱动设计的核心组成部分。
pub mod crawl;
pub mod scrape_result;
pub mod task;
pub mod webhook;
