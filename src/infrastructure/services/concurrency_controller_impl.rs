// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Redis 并发控制器实现
//!
//! 使用 Redis ZSET 实现分布式信号量，提供高性能的并发控制

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::services::concurrency_controller::{
    ConcurrencyController, ConcurrencyResult,
};
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::workers::constants::CONCURRENCY_CONTROL_LUA;

/// 基于 Redis 的并发控制器实现
///
/// 使用 Redis Sorted Set (ZSET) 实现分布式信号量：
/// - 使用 ZADD 添加任务到活跃任务集合
/// - 使用 ZCARD 统计当前并发数
/// - 使用 ZREMRANGEBYSCORE 清理过期任务
/// - 使用 Lua 脚本确保原子性操作
///
/// # 特点
///
/// - 高性能：Lua 脚本将多个 Redis 调用合并为一个原子操作
/// - 心跳机制：支持任务心跳更新，防止误判为过期
/// - 动态限制：支持从 Redis 读取或使用默认限制
#[derive(Clone)]
pub struct RedisConcurrencyController {
    redis: Arc<RedisClient>,
    default_concurrency_limit: usize,
}

impl RedisConcurrencyController {
    /// 创建新的 Redis 并发控制器
    ///
    /// # Arguments
    ///
    /// * `redis` - Redis 客户端
    /// * `default_concurrency_limit` - 默认并发限制
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
    pub fn get_effective_limit(
        &self,
        task: &crate::domain::models::Task,
    ) -> usize {
        Self::extract_payload_limit(task).unwrap_or(self.default_concurrency_limit)
    }

    /// 生成任务标识键
    ///
    /// 格式: `team_id:task_id`
    fn generate_task_key(&self, team_id: Uuid, task_id: Uuid) -> String {
        let mut key = String::with_capacity(64);
        key.push_str(&team_id.to_string());
        key.push(':');
        key.push_str(&task_id.to_string());
        key
    }

    /// 生成 Redis 键
    fn team_active_tasks_key(&self, team_id: Uuid) -> String {
        format!("team:{}:active_tasks", team_id)
    }

    fn team_concurrency_limit_key(&self, team_id: Uuid) -> String {
        format!("team:{}:concurrency_limit", team_id)
    }
}

#[async_trait]
impl ConcurrencyController for RedisConcurrencyController {
    async fn check_team_concurrency(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<ConcurrencyResult> {
        let task_key = self.generate_task_key(team_id, task_id);
        let active_key = self.team_active_tasks_key(team_id);
        let limit_key = self.team_concurrency_limit_key(team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        // 执行 Lua 脚本进行检查
        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&active_key, &limit_key],
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
        let task_key = self.generate_task_key(team_id, task_id);
        let active_key = self.team_active_tasks_key(team_id);
        let limit_key = self.team_concurrency_limit_key(team_id);
        let now = Utc::now().timestamp() as f64;
        let stale_threshold = now - 3600.0; // 1 hour stale

        // 执行原子 Lua 脚本
        let result = self
            .redis
            .eval(
                CONCURRENCY_CONTROL_LUA,
                &[&active_key, &limit_key],
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
        let active_key = self.team_active_tasks_key(team_id);
        let task_id_str = task_id.to_string();

        self.redis.zrem(&active_key, &task_id_str).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, TaskType};

    #[test]
    fn test_extract_payload_limit_scrape_task() {
        let task = Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            url: "http://example.com".to_string(),
            payload: serde_json::json!({
                "config": {
                    "max_concurrency": 5
                }
            }),
            ..Task::default()
        };

        // Scrape tasks don't check payload limit
        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task() {
        let task = Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            url: "http://example.com".to_string(),
            payload: serde_json::json!({
                "config": {
                    "max_concurrency": 10
                }
            }),
            ..Task::default()
        };

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, Some(10));
    }

    #[test]
    fn test_extract_payload_limit_no_config() {
        let task = Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            url: "http://example.com".to_string(),
            payload: serde_json::json!({}),
            ..Task::default()
        };

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_non_crawl_task() {
        let task = Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            task_type: TaskType::Extract,
            url: "http://example.com".to_string(),
            payload: serde_json::json!({
                "config": {
                    "max_concurrency": 5
                }
            }),
            ..Task::default()
        };

        // Non-Crawl tasks don't check payload limit
        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }
}
