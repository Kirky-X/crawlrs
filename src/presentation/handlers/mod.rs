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

use crate::domain::models::Task;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskStatus, TaskType};
    use chrono::Utc;
    use uuid::Uuid;

    /// 构造一个用于测试的 Task 实例，id 由参数指定，其余字段使用合理默认值。
    fn make_task(id: Uuid) -> Task {
        let now = Utc::now();
        Task {
            id,
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        }
    }

    /// extract_task_ids 应按输入顺序返回所有任务的 id。
    /// 验证：空切片返回空 Vec；非空切片返回长度相等且顺序一致的 id 列表。
    #[test]
    fn test_extract_task_ids_returns_ids_in_order() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let tasks = vec![make_task(id1), make_task(id2), make_task(id3)];

        let ids = extract_task_ids(&tasks);

        assert_eq!(ids, vec![id1, id2, id3]);
    }

    /// extract_task_ids 对空切片应返回空 Vec（不 panic）。
    #[test]
    fn test_extract_task_ids_empty_slice_returns_empty_vec() {
        let tasks: Vec<Task> = vec![];
        let ids = extract_task_ids(&tasks);
        assert!(ids.is_empty());
    }

    /// extract_task_ids 对单元素切片应返回单元素 Vec。
    #[test]
    fn test_extract_task_ids_single_element() {
        let id = Uuid::new_v4();
        let tasks = vec![make_task(id)];
        let ids = extract_task_ids(&tasks);
        assert_eq!(ids, vec![id]);
    }

    /// extract_task_ids 不应消耗输入（接收 &[Task]），调用后原 Vec 仍可用。
    #[test]
    fn test_extract_task_ids_does_not_consume_input() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![make_task(id1), make_task(id2)];

        let ids = extract_task_ids(&tasks);
        assert_eq!(ids.len(), 2);

        // 原始 Vec 仍然可用（未被消耗）
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, id1);
        assert_eq!(tasks[1].id, id2);
    }

    /// tasks_to_id_map 应将任务列表转换为 id -> task 的映射。
    /// 验证：映射大小与输入一致；通过 id 可取回对应任务。
    #[test]
    fn test_tasks_to_id_map_builds_correct_mapping() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![make_task(id1), make_task(id2)];

        let map = tasks_to_id_map(tasks);

        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&id1));
        assert!(map.contains_key(&id2));
        assert_eq!(map.get(&id1).unwrap().id, id1);
        assert_eq!(map.get(&id2).unwrap().id, id2);
    }

    /// tasks_to_id_map 对空 Vec 应返回空 HashMap（不 panic）。
    #[test]
    fn test_tasks_to_id_map_empty_input_returns_empty_map() {
        let tasks: Vec<Task> = vec![];
        let map = tasks_to_id_map(tasks);
        assert!(map.is_empty());
    }

    /// tasks_to_id_map 应消耗输入 Vec（接收 Vec<Task> 而非 &）。
    /// 验证：调用后原 Vec 不可再用（编译期保证，此处仅验证返回值正确）。
    #[test]
    fn test_tasks_to_id_map_single_element() {
        let id = Uuid::new_v4();
        let tasks = vec![make_task(id)];

        let map = tasks_to_id_map(tasks);

        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&id).unwrap().id, id);
    }

    /// tasks_to_id_map 保留最后一个任务当出现重复 id（后入覆盖）。
    /// 虽然 Task.id 通常是唯一的，但函数语义是 HashMap insert，后入覆盖前入。
    #[test]
    fn test_tasks_to_id_map_duplicate_ids_last_wins() {
        let id = Uuid::new_v4();
        let mut task1 = make_task(id);
        task1.url = "https://first.com".to_string();
        let mut task2 = make_task(id);
        task2.url = "https://second.com".to_string();

        let tasks = vec![task1, task2];
        let map = tasks_to_id_map(tasks);

        // 只有一个条目（id 相同），后插入的覆盖前一个
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&id).unwrap().url, "https://second.com");
    }
}
