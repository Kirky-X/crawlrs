// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// HTTP请求处理器模块
///
/// 包含各个API端点的具体处理逻辑
/// 每个处理器负责处理特定类型的HTTP请求并返回响应
pub mod audit_handler;
pub mod crawl_handler;
pub mod extract_handler;
pub mod metrics_handler;
pub mod response_builder;
pub mod scrape_handler;
pub mod search_handler;
pub mod task_handler;
pub mod team_handler;
pub mod webhook_handler;

use crate::domain::models::task::Task;
use std::collections::HashMap;
use uuid::Uuid;

/// 从任务列表中提取ID列表 - 消除重复代码
#[inline]
pub fn extract_task_ids(tasks: &[Task]) -> Vec<Uuid> {
    tasks.iter().map(|task| task.id).collect()
}

/// 将任务列表转换为ID到任务的映射 - 提高查找效率
#[inline]
pub fn tasks_to_id_map(tasks: Vec<Task>) -> HashMap<Uuid, Task> {
    tasks.into_iter().map(|task| (task.id, task)).collect()
}
