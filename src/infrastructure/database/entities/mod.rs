// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 数据库实体模块
///
/// 定义数据库表对应的实体结构
/// 使用SeaORM框架进行对象关系映射
/// 包含所有业务实体的数据库表示
pub mod api_key;
pub mod crawl;
pub mod credits;
pub mod credits_transactions;
pub mod scrape_result;
pub mod task;
pub mod tasks_backlog;
pub mod webhook;
pub mod webhook_event;
