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

use crate::domain::services::concurrency_controller::{ConcurrencyController, ConcurrencyResult};
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
    pub fn get_effective_limit(&self, task: &crate::domain::models::Task) -> usize {
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
        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task() {
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

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, Some(10));
    }

    #[test]
    fn test_extract_payload_limit_no_config() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_non_crawl_task() {
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
        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task_no_max_concurrency() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {}
            }),
        );

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task_config_not_object() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": "not-an-object"
            }),
        );

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task_max_concurrency_not_u64() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {
                    "max_concurrency": "not-a-number"
                }
            }),
        );

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    #[test]
    fn test_extract_payload_limit_crawl_task_no_payload_config_key() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );

        let limit = RedisConcurrencyController::extract_payload_limit(&task);
        assert_eq!(limit, None);
    }

    fn make_controller(limit: usize) -> Option<RedisConcurrencyController> {
        let redis = RedisClient::new("redis://127.0.0.1:1/").ok()?;
        Some(RedisConcurrencyController::new(redis, limit))
    }

    #[test]
    fn test_new_constructor_stores_default_limit() {
        let controller = make_controller(50);
        assert!(controller.is_some());

        // Verify default limit is used when task has no payload limit
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );

        let controller = controller.unwrap();
        assert_eq!(controller.get_effective_limit(&task), 50);
    }

    #[test]
    fn test_get_effective_limit_uses_payload_limit_for_crawl() {
        let controller = make_controller(100);
        assert!(controller.is_some());

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({
                "config": {
                    "max_concurrency": 25
                }
            }),
        );

        let controller = controller.unwrap();
        assert_eq!(controller.get_effective_limit(&task), 25);
    }

    #[test]
    fn test_get_effective_limit_falls_back_to_default_for_crawl_without_config() {
        let controller = make_controller(30);
        assert!(controller.is_some());

        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );

        let controller = controller.unwrap();
        assert_eq!(controller.get_effective_limit(&task), 30);
    }

    #[test]
    fn test_get_effective_limit_uses_default_for_non_crawl() {
        let controller = make_controller(40);
        assert!(controller.is_some());

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

        let controller = controller.unwrap();
        assert_eq!(controller.get_effective_limit(&task), 40);
    }

    #[tokio::test]
    async fn test_check_team_concurrency_returns_error_without_redis() {
        let controller = make_controller(10);
        assert!(controller.is_some());
        let controller = controller.unwrap();

        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let result = controller.check_team_concurrency(team_id, task_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_acquire_semaphore_returns_error_without_redis() {
        let controller = make_controller(10);
        assert!(controller.is_some());
        let controller = controller.unwrap();

        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let result = controller.acquire_semaphore(team_id, task_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_release_semaphore_returns_error_without_redis() {
        let controller = make_controller(10);
        assert!(controller.is_some());
        let controller = controller.unwrap();

        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let result = controller.release_semaphore(team_id, task_id).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_controller_is_cloneable() {
        let controller = make_controller(10);
        assert!(controller.is_some());
        let controller = controller.unwrap();

        let cloned = controller.clone();
        // Verify both controllers have the same default limit behavior
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com".to_string(),
            serde_json::json!({}),
        );
        assert_eq!(controller.get_effective_limit(&task), 10);
        assert_eq!(cloned.get_effective_limit(&task), 10);
    }
}
