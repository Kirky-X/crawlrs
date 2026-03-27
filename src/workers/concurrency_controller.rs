// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 并发控制模块
//!
//! 提供任务并发控制功能，包括信号量管理和并发限制

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::services::concurrency_controller::{
    ConcurrencyController as ConcurrencyControllerTrait, ConcurrencyResult,
};
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::workers::constants::CONCURRENCY_CONTROL_LUA;

/// 并发控制器
///
/// 负责管理团队级别的任务并发控制
#[derive(Clone)]
pub struct ConcurrencyController {
    redis: Arc<RedisClient>,
    default_concurrency_limit: usize,
}

impl ConcurrencyController {
    /// 创建新的并发控制器
    pub fn new(redis: RedisClient, default_concurrency_limit: usize) -> Self {
        Self {
            redis: Arc::new(redis),
            default_concurrency_limit,
        }
    }

    /// 从任务负载中提取并发限制
    pub fn extract_payload_limit(task: &crate::domain::models::Task) -> Option<usize> {
        if task.task_type == crate::domain::models::TaskType::Crawl {
            task.payload
                .get("config")
                .and_then(|c| c.get("max_concurrency"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
        } else {
            None
        }
    }

    /// 获取有效的并发限制
    pub fn get_effective_limit(&self, task: &crate::domain::models::Task) -> usize {
        Self::extract_payload_limit(task).unwrap_or(self.default_concurrency_limit)
    }

    /// 获取信号量许可
    ///
    /// # Arguments
    ///
    /// * `task` - 任务
    ///
    /// # Returns
    ///
    /// 如果获取成功返回 Ok(true)，如果达到限制返回 Ok(false)，错误返回 Err
    pub async fn acquire_permit(&self, task: &crate::domain::models::Task) -> Result<bool> {
        let team_id = task.team_id;
        // Pre-allocate String with capacity to avoid reallocations
        let mut task_id_str = String::with_capacity(64);
        task_id_str.push_str(&team_id.to_string());
        task_id_str.push(':');
        task_id_str.push_str(&task.id.to_string());

        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        let team_concurrency_limit_key = format!("team:{}:concurrency_limit", team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        let default_limit = self.get_effective_limit(task);

        // Execute atomic Lua script - reduces 4 Redis calls to 1
        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&team_active_tasks_key, &team_concurrency_limit_key],
                &[
                    &task_id_str,
                    &now.to_string(),
                    &stale_threshold.to_string(),
                    &default_limit.to_string(),
                ],
            )
            .await?;

        let granted = result == "1";
        Ok(granted)
    }

    /// 释放信号量许可
    pub async fn release_permit(&self, team_id: Uuid, task_id: Uuid) -> Result<()> {
        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        // Pre-allocate String for task_id
        let task_id_str = task_id.to_string();
        self.redis
            .zrem(&team_active_tasks_key, &task_id_str)
            .await?;
        Ok(())
    }
}

/// 生成任务标识键
fn generate_task_key(team_id: Uuid, task_id: Uuid) -> String {
    let mut key = String::with_capacity(64);
    key.push_str(&team_id.to_string());
    key.push(':');
    key.push_str(&task_id.to_string());
    key
}

#[async_trait]
impl ConcurrencyControllerTrait for ConcurrencyController {
    async fn check_team_concurrency(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<ConcurrencyResult> {
        let task_key = generate_task_key(team_id, task_id);
        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        let team_concurrency_limit_key = format!("team:{}:concurrency_limit", team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&team_active_tasks_key, &team_concurrency_limit_key],
                &[
                    &task_key,
                    &now.to_string(),
                    &stale_threshold.to_string(),
                    &self.default_concurrency_limit.to_string(),
                ],
            )
            .await?;

        let granted = result == "1";
        if granted {
            Ok(ConcurrencyResult::Allowed)
        } else {
            Ok(ConcurrencyResult::Denied {
                reason: "已达到团队并发限制".to_string(),
            })
        }
    }

    async fn acquire_semaphore(&self, team_id: Uuid, task_id: Uuid) -> Result<bool> {
        let task_key = generate_task_key(team_id, task_id);
        let team_active_tasks_key = format!("team:{}:active_tasks", team_id);
        let team_concurrency_limit_key = format!("team:{}:concurrency_limit", team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&team_active_tasks_key, &team_concurrency_limit_key],
                &[
                    &task_key,
                    &now.to_string(),
                    &stale_threshold.to_string(),
                    &self.default_concurrency_limit.to_string(),
                ],
            )
            .await?;

        let granted = result == "1";
        Ok(granted)
    }

    async fn release_semaphore(&self, team_id: Uuid, task_id: Uuid) -> Result<()> {
        self.release_permit(team_id, task_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_payload_limit_scrape_task() {
        use crate::domain::models::{Task, TaskType};
        use uuid::Uuid;

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {
                    "max_concurrency": 5
                }
            }),
        );

        // Scrape tasks don't check payload limit
        let limit = ConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task() {
        use crate::domain::models::{Task, TaskType};
        use uuid::Uuid;

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {
                    "max_concurrency": 10
                }
            }),
        );

        let limit = ConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, Some(10));
    }

    #[test]
    fn test_extract_payload_limit_no_config() {
        use crate::domain::models::{Task, TaskType};
        use uuid::Uuid;

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );

        let limit = ConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_non_crawl_task() {
        use crate::domain::models::{Task, TaskType};
        use uuid::Uuid;

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Extract,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {
                    "max_concurrency": 5
                }
            }),
        );

        // Non-Crawl tasks don't check payload limit
        let limit = ConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }
}
