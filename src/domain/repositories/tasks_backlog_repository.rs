// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Tasks backlog repository trait and domain model
//!
//! This module defines the repository interface and domain model for task backlog.
//! The trait is defined in the domain layer, while implementations reside in the
//! infrastructure layer, following the Dependency Inversion Principle.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::repositories::task_repository::RepositoryError;

/// 任务积压状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TasksBacklogStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Expired,
}

impl std::fmt::Display for TasksBacklogStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TasksBacklogStatus::Pending => write!(f, "pending"),
            TasksBacklogStatus::Processing => write!(f, "processing"),
            TasksBacklogStatus::Completed => write!(f, "completed"),
            TasksBacklogStatus::Failed => write!(f, "failed"),
            TasksBacklogStatus::Expired => write!(f, "expired"),
        }
    }
}

impl std::str::FromStr for TasksBacklogStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(TasksBacklogStatus::Pending),
            "processing" => Ok(TasksBacklogStatus::Processing),
            "completed" => Ok(TasksBacklogStatus::Completed),
            "failed" => Ok(TasksBacklogStatus::Failed),
            "expired" => Ok(TasksBacklogStatus::Expired),
            _ => Err(format!("Invalid tasks backlog status: {}", s)),
        }
    }
}

/// 任务积压领域模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasksBacklog {
    pub id: Uuid,
    pub task_id: Uuid,
    pub team_id: Uuid,
    pub task_type: String,
    pub priority: i32,
    pub payload: serde_json::Value,
    pub max_retries: i32,
    pub retry_count: i32,
    pub status: TasksBacklogStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
}

impl TasksBacklog {
    /// 创建新的任务积压项
    pub fn new(
        task_id: Uuid,
        team_id: Uuid,
        task_type: String,
        priority: i32,
        payload: serde_json::Value,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            task_id,
            team_id,
            task_type,
            priority,
            payload,
            max_retries: 3,
            retry_count: 0,
            status: TasksBacklogStatus::Pending,
            created_at: now,
            updated_at: now,
            scheduled_at: None,
            expires_at,
            processed_at: None,
        }
    }

    /// 标记为处理中
    pub fn mark_processing(&mut self) -> Result<(), String> {
        if self.status != TasksBacklogStatus::Pending {
            return Err("Only pending tasks can be marked as processing".to_string());
        }
        self.status = TasksBacklogStatus::Processing;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为已完成
    pub fn mark_completed(&mut self) -> Result<(), String> {
        if self.status != TasksBacklogStatus::Processing {
            return Err("Only processing tasks can be marked as completed".to_string());
        }
        self.status = TasksBacklogStatus::Completed;
        self.processed_at = Some(Utc::now());
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为失败
    pub fn mark_failed(&mut self) -> Result<(), String> {
        self.status = TasksBacklogStatus::Failed;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 标记为已过期
    pub fn mark_expired(&mut self) -> Result<(), String> {
        self.status = TasksBacklogStatus::Expired;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 增加重试次数
    pub fn increment_retry_count(&mut self) {
        self.retry_count += 1;
        self.updated_at = Utc::now();
    }

    /// 检查是否已过期
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            return Utc::now() >= expires_at;
        }
        false
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }
}

/// 任务积压仓储接口
#[async_trait]
pub trait TasksBacklogRepository: Send + Sync {
    /// 创建任务积压项
    async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError>;

    /// 根据ID查找任务积压项
    async fn find_by_id(&self, id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError>;

    /// 根据任务ID查找任务积压项
    async fn find_by_task_id(&self, task_id: Uuid)
        -> Result<Option<TasksBacklog>, RepositoryError>;

    /// 更新任务积压项
    async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError>;

    /// 删除任务积压项
    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// 获取待处理的任务积压项（按优先级排序）
    async fn get_pending_tasks(
        &self,
        team_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError>;

    /// 获取过期的任务积压项
    async fn get_expired_tasks(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<TasksBacklog>, RepositoryError>;

    /// 统计任务积压项数量
    async fn count_by_status(
        &self,
        team_id: Option<Uuid>,
        status: TasksBacklogStatus,
    ) -> Result<i64, RepositoryError>;

    /// 批量更新任务积压项状态
    async fn update_status_batch(
        &self,
        ids: &[Uuid],
        status: TasksBacklogStatus,
    ) -> Result<u64, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== TasksBacklogStatus Display tests ==========

    #[test]
    fn test_status_display_pending() {
        assert_eq!(TasksBacklogStatus::Pending.to_string(), "pending");
    }

    #[test]
    fn test_status_display_processing() {
        assert_eq!(TasksBacklogStatus::Processing.to_string(), "processing");
    }

    #[test]
    fn test_status_display_completed() {
        assert_eq!(TasksBacklogStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_status_display_failed() {
        assert_eq!(TasksBacklogStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_status_display_expired() {
        assert_eq!(TasksBacklogStatus::Expired.to_string(), "expired");
    }

    // ========== TasksBacklogStatus FromStr tests ==========

    #[test]
    fn test_status_from_str_pending() {
        let status: TasksBacklogStatus = "pending".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Pending);
    }

    #[test]
    fn test_status_from_str_processing() {
        let status: TasksBacklogStatus = "processing".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Processing);
    }

    #[test]
    fn test_status_from_str_completed() {
        let status: TasksBacklogStatus = "completed".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Completed);
    }

    #[test]
    fn test_status_from_str_failed() {
        let status: TasksBacklogStatus = "failed".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Failed);
    }

    #[test]
    fn test_status_from_str_expired() {
        let status: TasksBacklogStatus = "expired".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Expired);
    }

    #[test]
    fn test_status_from_str_case_insensitive() {
        let status: TasksBacklogStatus = "PENDING".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Pending);
    }

    #[test]
    fn test_status_from_str_mixed_case() {
        let status: TasksBacklogStatus = "CoMpLeTeD".parse().unwrap();
        assert_eq!(status, TasksBacklogStatus::Completed);
    }

    #[test]
    fn test_status_from_str_invalid() {
        let result: Result<TasksBacklogStatus, String> = "invalid_status".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid tasks backlog status"));
    }

    #[test]
    fn test_status_from_str_empty() {
        let result: Result<TasksBacklogStatus, String> = "".parse();
        assert!(result.is_err());
    }

    // ========== TasksBacklogStatus clone/serialize ==========

    #[test]
    fn test_status_copy() {
        let status = TasksBacklogStatus::Pending;
        let copied = status; // TasksBacklogStatus implements Copy
        assert_eq!(status, copied);
    }

    #[test]
    fn test_status_serde_roundtrip() {
        let status = TasksBacklogStatus::Processing;
        let json = serde_json::to_string(&status).unwrap();
        let back: TasksBacklogStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn test_status_copy_equality() {
        let a = TasksBacklogStatus::Completed;
        let b = a;
        assert_eq!(a, b);
    }

    // ========== TasksBacklog::new tests ==========

    #[test]
    fn test_tasks_backlog_new_defaults() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let payload = serde_json::json!({"key": "value"});
        let backlog = TasksBacklog::new(
            task_id,
            team_id,
            "scrape".to_string(),
            5,
            payload.clone(),
            None,
        );

        assert_eq!(backlog.task_id, task_id);
        assert_eq!(backlog.team_id, team_id);
        assert_eq!(backlog.task_type, "scrape");
        assert_eq!(backlog.priority, 5);
        assert_eq!(backlog.payload, payload);
        assert_eq!(backlog.max_retries, 3);
        assert_eq!(backlog.retry_count, 0);
        assert_eq!(backlog.status, TasksBacklogStatus::Pending);
        assert!(backlog.scheduled_at.is_none());
        assert!(backlog.expires_at.is_none());
        assert!(backlog.processed_at.is_none());
    }

    #[test]
    fn test_tasks_backlog_new_with_expires_at() {
        let expires = Utc::now() + chrono::Duration::hours(1);
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "crawl".to_string(),
            1,
            serde_json::json!({}),
            Some(expires),
        );
        assert_eq!(backlog.expires_at, Some(expires));
    }

    #[test]
    fn test_tasks_backlog_new_generates_unique_ids() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let b1 = TasksBacklog::new(
            task_id,
            team_id,
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        let b2 = TasksBacklog::new(
            task_id,
            team_id,
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert_ne!(b1.id, b2.id);
    }

    // ========== mark_processing tests ==========

    #[test]
    fn test_mark_processing_from_pending_succeeds() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert!(backlog.mark_processing().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Processing);
    }

    #[test]
    fn test_mark_processing_from_processing_fails() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        let result = backlog.mark_processing();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Only pending tasks"));
    }

    #[test]
    fn test_mark_processing_from_completed_fails() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        backlog.mark_completed().unwrap();
        assert!(backlog.mark_processing().is_err());
    }

    // ========== mark_completed tests ==========

    #[test]
    fn test_mark_completed_from_processing_succeeds() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        assert!(backlog.mark_completed().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Completed);
        assert!(backlog.processed_at.is_some());
    }

    #[test]
    fn test_mark_completed_from_pending_fails() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        let result = backlog.mark_completed();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Only processing tasks"));
    }

    #[test]
    fn test_mark_completed_from_completed_fails() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        backlog.mark_completed().unwrap();
        assert!(backlog.mark_completed().is_err());
    }

    // ========== mark_failed tests ==========

    #[test]
    fn test_mark_failed_from_any_state() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert!(backlog.mark_failed().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Failed);
    }

    #[test]
    fn test_mark_failed_from_processing() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        assert!(backlog.mark_failed().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Failed);
    }

    // ========== mark_expired tests ==========

    #[test]
    fn test_mark_expired_from_pending() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert!(backlog.mark_expired().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Expired);
    }

    #[test]
    fn test_mark_expired_from_processing() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        backlog.mark_processing().unwrap();
        assert!(backlog.mark_expired().is_ok());
        assert_eq!(backlog.status, TasksBacklogStatus::Expired);
    }

    // ========== increment_retry_count tests ==========

    #[test]
    fn test_increment_retry_count_increments() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert_eq!(backlog.retry_count, 0);
        backlog.increment_retry_count();
        assert_eq!(backlog.retry_count, 1);
        backlog.increment_retry_count();
        assert_eq!(backlog.retry_count, 2);
    }

    // ========== is_expired tests ==========

    #[test]
    fn test_is_expired_no_expiry_returns_false() {
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert!(!backlog.is_expired());
    }

    #[test]
    fn test_is_expired_future_expiry_returns_false() {
        let future = Utc::now() + chrono::Duration::hours(1);
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            Some(future),
        );
        assert!(!backlog.is_expired());
    }

    #[test]
    fn test_is_expired_past_expiry_returns_true() {
        let past = Utc::now() - chrono::Duration::hours(1);
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            Some(past),
        );
        assert!(backlog.is_expired());
    }

    // ========== can_retry tests ==========

    #[test]
    fn test_can_retry_below_max() {
        let backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        assert!(backlog.can_retry());
    }

    #[test]
    fn test_can_retry_at_max() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        for _ in 0..backlog.max_retries {
            backlog.increment_retry_count();
        }
        assert!(!backlog.can_retry());
    }

    #[test]
    fn test_can_retry_above_max() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "t".to_string(),
            1,
            serde_json::json!({}),
            None,
        );
        for _ in 0..(backlog.max_retries + 1) {
            backlog.increment_retry_count();
        }
        assert!(!backlog.can_retry());
    }

    // ========== full lifecycle test ==========

    #[test]
    fn test_full_lifecycle_pending_to_completed() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "scrape".to_string(),
            5,
            serde_json::json!({"url": "http://example.com"}),
            None,
        );

        assert_eq!(backlog.status, TasksBacklogStatus::Pending);
        assert!(backlog.can_retry());

        backlog.mark_processing().unwrap();
        assert_eq!(backlog.status, TasksBacklogStatus::Processing);

        backlog.mark_completed().unwrap();
        assert_eq!(backlog.status, TasksBacklogStatus::Completed);
        assert!(backlog.processed_at.is_some());
    }

    #[test]
    fn test_full_lifecycle_with_retries_and_failure() {
        let mut backlog = TasksBacklog::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "crawl".to_string(),
            3,
            serde_json::json!({}),
            None,
        );

        // First attempt fails
        backlog.mark_processing().unwrap();
        backlog.mark_failed().unwrap();
        assert_eq!(backlog.status, TasksBacklogStatus::Failed);

        // Retry - note: mark_failed doesn't reset to pending, so this tests the
        // state machine as implemented
        assert_eq!(backlog.retry_count, 0);
        backlog.increment_retry_count();
        assert!(backlog.can_retry());
    }
}
