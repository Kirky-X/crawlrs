// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 领域事件模型
//!
//! 定义系统中所有重要的领域事件类型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 领域事件trait
///
/// 所有领域事件都应实现此trait。
pub trait DomainEvent: Send + Sync {
    /// 获取事件类型名称
    fn event_type(&self) -> &'static str;
    /// 获取事件ID
    fn event_id(&self) -> Uuid;
    /// 获取事件发生时间
    fn occurred_at(&self) -> DateTime<Utc>;
    /// 获取聚合根ID
    fn aggregate_id(&self) -> Uuid;
    /// 获取聚合根类型
    fn aggregate_type(&self) -> &'static str;
}

/// 事件元数据
///
/// 包含事件的附加信息，用于事件追踪和调试。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// 事件ID
    pub event_id: Uuid,
    /// 事件类型
    pub event_type: String,
    /// 聚合根ID
    pub aggregate_id: Uuid,
    /// 聚合根类型
    pub aggregate_type: String,
    /// 事件发生时间
    pub occurred_at: DateTime<Utc>,
    /// 追踪ID（用于分布式追踪）
    pub trace_id: Option<Uuid>,
    /// 用户/团队ID
    pub tenant_id: Option<Uuid>,
    /// 附加数据
    #[serde(default)]
    pub additional_data: serde_json::Value,
}

impl EventMetadata {
    /// 创建新的事件元数据
    pub fn new(
        event_type: &'static str,
        aggregate_id: Uuid,
        aggregate_type: &'static str,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            aggregate_id,
            aggregate_type: aggregate_type.to_string(),
            occurred_at: Utc::now(),
            trace_id: None,
            tenant_id: None,
            additional_data: serde_json::json!({}),
        }
    }

    /// 添加追踪ID
    pub fn with_trace_id(mut self, trace_id: Uuid) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    /// 添加租户ID
    pub fn with_tenant_id(mut self, tenant_id: Uuid) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// 添加附加数据
    pub fn with_additional_data(mut self, data: serde_json::Value) -> Self {
        self.additional_data = data;
        self
    }
}

/// 任务相关事件
pub mod task {
    use super::*;

    /// 任务创建事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TaskCreatedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 任务ID
        pub task_id: Uuid,
        /// 任务类型
        pub task_type: String,
        /// 目标URL
        pub url: String,
        /// 团队ID
        pub team_id: Uuid,
        /// 优先级
        pub priority: i32,
    }

    impl TaskCreatedEvent {
        /// 创建新的任务创建事件
        pub fn new(
            task_id: Uuid,
            task_type: String,
            url: String,
            team_id: Uuid,
            priority: i32,
        ) -> Self {
            let mut metadata = EventMetadata::new("TaskCreated", task_id, "Task");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                task_id,
                task_type,
                url,
                team_id,
                priority,
            }
        }
    }

    impl super::DomainEvent for TaskCreatedEvent {
        fn event_type(&self) -> &'static str {
            "TaskCreated"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.task_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Task"
        }
    }

    /// 任务完成事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TaskCompletedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 任务ID
        pub task_id: Uuid,
        /// 任务类型
        pub task_type: String,
        /// 团队ID
        pub team_id: Uuid,
        /// 执行耗时（毫秒）
        pub duration_ms: u64,
        /// 结果摘要
        pub result_summary: serde_json::Value,
    }

    impl TaskCompletedEvent {
        /// 创建新的任务完成事件
        pub fn new(
            task_id: Uuid,
            task_type: String,
            team_id: Uuid,
            duration_ms: u64,
            result_summary: serde_json::Value,
        ) -> Self {
            let mut metadata = EventMetadata::new("TaskCompleted", task_id, "Task");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                task_id,
                task_type,
                team_id,
                duration_ms,
                result_summary,
            }
        }
    }

    impl super::DomainEvent for TaskCompletedEvent {
        fn event_type(&self) -> &'static str {
            "TaskCompleted"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.task_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Task"
        }
    }

    /// 任务失败事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TaskFailedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 任务ID
        pub task_id: Uuid,
        /// 任务类型
        pub task_type: String,
        /// 团队ID
        pub team_id: Uuid,
        /// 错误信息
        pub error_message: String,
        /// 重试次数
        pub retry_count: u32,
    }

    impl TaskFailedEvent {
        /// 创建新的任务失败事件
        pub fn new(
            task_id: Uuid,
            task_type: String,
            team_id: Uuid,
            error_message: String,
            retry_count: u32,
        ) -> Self {
            let mut metadata = EventMetadata::new("TaskFailed", task_id, "Task");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                task_id,
                task_type,
                team_id,
                error_message,
                retry_count,
            }
        }
    }

    impl super::DomainEvent for TaskFailedEvent {
        fn event_type(&self) -> &'static str {
            "TaskFailed"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.task_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Task"
        }
    }
}

/// 爬取相关事件
pub mod crawl {
    use super::*;

    /// 爬取开始事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CrawlStartedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 爬取ID
        pub crawl_id: Uuid,
        /// 团队ID
        pub team_id: Uuid,
        /// 根URL
        pub root_url: String,
        /// 预估页面数
        pub estimated_pages: u64,
    }

    impl CrawlStartedEvent {
        /// 创建新的爬取开始事件
        pub fn new(crawl_id: Uuid, team_id: Uuid, root_url: String, estimated_pages: u64) -> Self {
            let mut metadata = EventMetadata::new("CrawlStarted", crawl_id, "Crawl");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                crawl_id,
                team_id,
                root_url,
                estimated_pages,
            }
        }
    }

    impl super::DomainEvent for CrawlStartedEvent {
        fn event_type(&self) -> &'static str {
            "CrawlStarted"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.crawl_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Crawl"
        }
    }

    /// 爬取完成事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CrawlCompletedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 爬取ID
        pub crawl_id: Uuid,
        /// 团队ID
        pub team_id: Uuid,
        /// 总任务数
        pub total_tasks: u64,
        /// 完成的任务数
        pub completed_tasks: u64,
        /// 失败的任务数
        pub failed_tasks: u64,
        /// 总耗时（毫秒）
        pub total_duration_ms: u64,
    }

    impl CrawlCompletedEvent {
        /// 创建新的爬取完成事件
        pub fn new(
            crawl_id: Uuid,
            team_id: Uuid,
            total_tasks: u64,
            completed_tasks: u64,
            failed_tasks: u64,
            total_duration_ms: u64,
        ) -> Self {
            let mut metadata = EventMetadata::new("CrawlCompleted", crawl_id, "Crawl");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                crawl_id,
                team_id,
                total_tasks,
                completed_tasks,
                failed_tasks,
                total_duration_ms,
            }
        }
    }

    impl super::DomainEvent for CrawlCompletedEvent {
        fn event_type(&self) -> &'static str {
            "CrawlCompleted"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.crawl_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Crawl"
        }
    }
}

/// 积分相关事件
pub mod credits {
    use super::*;

    /// 积分扣除事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CreditsDeductedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 团队ID
        pub team_id: Uuid,
        /// 扣除的积分数量
        pub amount: u32,
        /// 扣除后的剩余积分
        pub remaining_credits: u32,
        /// 操作类型
        pub operation_type: String,
        /// 关联的资源ID
        pub resource_id: Uuid,
        /// 资源类型
        pub resource_type: String,
    }

    impl CreditsDeductedEvent {
        /// 创建新的积分扣除事件
        pub fn new(
            team_id: Uuid,
            amount: u32,
            remaining_credits: u32,
            operation_type: String,
            resource_id: Uuid,
            resource_type: String,
        ) -> Self {
            let mut metadata = EventMetadata::new("CreditsDeducted", team_id, "Credits");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                team_id,
                amount,
                remaining_credits,
                operation_type,
                resource_id,
                resource_type,
            }
        }
    }

    impl super::DomainEvent for CreditsDeductedEvent {
        fn event_type(&self) -> &'static str {
            "CreditsDeducted"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.team_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Credits"
        }
    }

    /// 积分不足事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CreditsLowEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 团队ID
        pub team_id: Uuid,
        /// 当前积分
        pub current_credits: u32,
        /// 阈值
        pub threshold: u32,
    }

    impl CreditsLowEvent {
        /// 创建新的积分不足事件
        pub fn new(team_id: Uuid, current_credits: u32, threshold: u32) -> Self {
            let mut metadata = EventMetadata::new("CreditsLow", team_id, "Credits");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                team_id,
                current_credits,
                threshold,
            }
        }
    }

    impl super::DomainEvent for CreditsLowEvent {
        fn event_type(&self) -> &'static str {
            "CreditsLow"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.team_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Credits"
        }
    }
}

/// 搜索相关事件
pub mod search {
    use super::*;

    /// 搜索执行事件
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SearchCompletedEvent {
        /// 事件元数据
        pub metadata: EventMetadata,
        /// 查询ID
        pub query_id: Uuid,
        /// 团队ID
        pub team_id: Uuid,
        /// 搜索查询
        pub query: String,
        /// 搜索引擎
        pub engine: String,
        /// 结果数量
        pub results_count: u64,
        /// 执行耗时（毫秒）
        pub duration_ms: u64,
    }

    impl SearchCompletedEvent {
        /// 创建新的搜索完成事件
        pub fn new(
            query_id: Uuid,
            team_id: Uuid,
            query: String,
            engine: String,
            results_count: u64,
            duration_ms: u64,
        ) -> Self {
            let mut metadata = EventMetadata::new("SearchCompleted", query_id, "Search");
            metadata.tenant_id = Some(team_id);
            Self {
                metadata,
                query_id,
                team_id,
                query,
                engine,
                results_count,
                duration_ms,
            }
        }
    }

    impl super::DomainEvent for SearchCompletedEvent {
        fn event_type(&self) -> &'static str {
            "SearchCompleted"
        }

        fn event_id(&self) -> Uuid {
            self.metadata.event_id
        }

        fn occurred_at(&self) -> DateTime<Utc> {
            self.metadata.occurred_at
        }

        fn aggregate_id(&self) -> Uuid {
            self.query_id
        }

        fn aggregate_type(&self) -> &'static str {
            "Search"
        }
    }
}
