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
    pub fn new(event_type: &'static str, aggregate_id: Uuid, aggregate_type: &'static str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::credits::*;
    use super::crawl::*;
    use super::search::*;
    use super::task::*;

    // ========== EventMetadata tests ==========

    #[test]
    fn test_event_metadata_new_sets_required_fields() {
        let aggregate_id = Uuid::new_v4();
        let before = Utc::now();
        let metadata = EventMetadata::new("TaskCreated", aggregate_id, "Task");
        let after = Utc::now();

        assert_eq!(metadata.event_type, "TaskCreated");
        assert_eq!(metadata.aggregate_id, aggregate_id);
        assert_eq!(metadata.aggregate_type, "Task");
        assert!(
            metadata.occurred_at >= before && metadata.occurred_at <= after,
            "occurred_at should be ~now"
        );
        assert_ne!(metadata.event_id, Uuid::nil());
    }

    #[test]
    fn test_event_metadata_new_defaults_optional_fields() {
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y");
        assert!(metadata.trace_id.is_none());
        assert!(metadata.tenant_id.is_none());
        assert_eq!(metadata.additional_data, serde_json::json!({}));
    }

    #[test]
    fn test_event_metadata_new_generates_unique_ids() {
        let m1 = EventMetadata::new("X", Uuid::new_v4(), "Y");
        let m2 = EventMetadata::new("X", Uuid::new_v4(), "Y");
        assert_ne!(m1.event_id, m2.event_id);
    }

    #[test]
    fn test_event_metadata_with_trace_id_sets_field() {
        let trace_id = Uuid::new_v4();
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y").with_trace_id(trace_id);
        assert_eq!(metadata.trace_id, Some(trace_id));
    }

    #[test]
    fn test_event_metadata_with_tenant_id_sets_field() {
        let tenant_id = Uuid::new_v4();
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y").with_tenant_id(tenant_id);
        assert_eq!(metadata.tenant_id, Some(tenant_id));
    }

    #[test]
    fn test_event_metadata_with_additional_data_replaces_default() {
        let data = serde_json::json!({"key": "value", "n": 42});
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y").with_additional_data(data.clone());
        assert_eq!(metadata.additional_data, data);
    }

    #[test]
    fn test_event_metadata_builder_chains_correctly() {
        let trace_id = Uuid::new_v4();
        let tenant_id = Uuid::new_v4();
        let data = serde_json::json!({"k": "v"});
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y")
            .with_trace_id(trace_id)
            .with_tenant_id(tenant_id)
            .with_additional_data(data.clone());

        assert_eq!(metadata.trace_id, Some(trace_id));
        assert_eq!(metadata.tenant_id, Some(tenant_id));
        assert_eq!(metadata.additional_data, data);
    }

    #[test]
    fn test_event_metadata_serde_roundtrip() {
        let metadata = EventMetadata::new("TaskCreated", Uuid::new_v4(), "Task")
            .with_trace_id(Uuid::new_v4())
            .with_tenant_id(Uuid::new_v4())
            .with_additional_data(serde_json::json!({"a": 1}));

        let json = serde_json::to_string(&metadata).expect("serialize");
        let back: EventMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.event_id, metadata.event_id);
        assert_eq!(back.event_type, metadata.event_type);
        assert_eq!(back.aggregate_id, metadata.aggregate_id);
        assert_eq!(back.aggregate_type, metadata.aggregate_type);
        assert_eq!(back.trace_id, metadata.trace_id);
        assert_eq!(back.tenant_id, metadata.tenant_id);
        assert_eq!(back.additional_data, metadata.additional_data);
    }

    #[test]
    fn test_event_metadata_serde_missing_additional_data_uses_default() {
        let id = Uuid::new_v4();
        let json = format!(
            "{{\"event_id\":\"{}\",\"event_type\":\"X\",\"aggregate_id\":\"{}\",\"aggregate_type\":\"Y\",\"occurred_at\":\"2024-01-01T00:00:00Z\"}}",
            id, id
        );
        let metadata: EventMetadata = serde_json::from_str(&json).expect("deserialize");
        // #[serde(default)] on serde_json::Value defaults to Value::Null
        assert_eq!(metadata.additional_data, serde_json::Value::Null);
    }

    #[test]
    fn test_event_metadata_clone_preserves_all_fields() {
        let metadata = EventMetadata::new("X", Uuid::new_v4(), "Y")
            .with_trace_id(Uuid::new_v4())
            .with_tenant_id(Uuid::new_v4())
            .with_additional_data(serde_json::json!({"k": "v"}));
        let cloned = metadata.clone();
        assert_eq!(metadata.event_id, cloned.event_id);
        assert_eq!(metadata.event_type, cloned.event_type);
        assert_eq!(metadata.aggregate_id, cloned.aggregate_id);
        assert_eq!(metadata.aggregate_type, cloned.aggregate_type);
        assert_eq!(metadata.trace_id, cloned.trace_id);
        assert_eq!(metadata.tenant_id, cloned.tenant_id);
        assert_eq!(metadata.additional_data, cloned.additional_data);
    }

    // ========== TaskCreatedEvent tests ==========

    #[test]
    fn test_task_created_event_new_populates_fields() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let before = Utc::now();
        let event = TaskCreatedEvent::new(
            task_id,
            "scrape".to_string(),
            "https://example.com".to_string(),
            team_id,
            5,
        );
        let after = Utc::now();

        assert_eq!(event.task_id, task_id);
        assert_eq!(event.task_type, "scrape");
        assert_eq!(event.url, "https://example.com");
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.priority, 5);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "TaskCreated");
        assert_eq!(event.metadata.aggregate_type, "Task");
        assert_eq!(event.metadata.aggregate_id, task_id);
        assert!(
            event.metadata.occurred_at >= before && event.metadata.occurred_at <= after,
            "occurred_at should be ~now"
        );
    }

    #[test]
    fn test_task_created_event_implements_domain_event() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = TaskCreatedEvent::new(
            task_id,
            "scrape".to_string(),
            "https://example.com".to_string(),
            team_id,
            1,
        );

        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "TaskCreated");
        assert_eq!(dyn_event.event_id(), event.metadata.event_id);
        assert_eq!(dyn_event.occurred_at(), event.metadata.occurred_at);
        assert_eq!(dyn_event.aggregate_id(), task_id);
        assert_eq!(dyn_event.aggregate_type(), "Task");
    }

    #[test]
    fn test_task_created_event_serde_roundtrip() {
        let event = TaskCreatedEvent::new(
            Uuid::new_v4(),
            "crawl".to_string(),
            "https://example.com/page".to_string(),
            Uuid::new_v4(),
            3,
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: TaskCreatedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.task_id, event.task_id);
        assert_eq!(back.task_type, event.task_type);
        assert_eq!(back.url, event.url);
        assert_eq!(back.team_id, event.team_id);
        assert_eq!(back.priority, event.priority);
        assert_eq!(back.metadata.event_id, event.metadata.event_id);
    }

    // ========== TaskCompletedEvent tests ==========

    #[test]
    fn test_task_completed_event_new_populates_fields() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let summary = serde_json::json!({"pages": 10});
        let event =
            TaskCompletedEvent::new(task_id, "crawl".to_string(), team_id, 1500, summary.clone());

        assert_eq!(event.task_id, task_id);
        assert_eq!(event.task_type, "crawl");
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.duration_ms, 1500);
        assert_eq!(event.result_summary, summary);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "TaskCompleted");
        assert_eq!(event.metadata.aggregate_id, task_id);
    }

    #[test]
    fn test_task_completed_event_domain_event_trait() {
        let task_id = Uuid::new_v4();
        let event = TaskCompletedEvent::new(
            task_id,
            "scrape".to_string(),
            Uuid::new_v4(),
            100,
            serde_json::json!({}),
        );
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "TaskCompleted");
        assert_eq!(dyn_event.aggregate_id(), task_id);
        assert_eq!(dyn_event.aggregate_type(), "Task");
    }

    #[test]
    fn test_task_completed_event_serde_roundtrip() {
        let event = TaskCompletedEvent::new(
            Uuid::new_v4(),
            "extract".to_string(),
            Uuid::new_v4(),
            250,
            serde_json::json!({"items": 5}),
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: TaskCompletedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.duration_ms, event.duration_ms);
        assert_eq!(back.result_summary, event.result_summary);
    }

    // ========== TaskFailedEvent tests ==========

    #[test]
    fn test_task_failed_event_new_populates_fields() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = TaskFailedEvent::new(
            task_id,
            "scrape".to_string(),
            team_id,
            "connection timeout".to_string(),
            3,
        );

        assert_eq!(event.task_id, task_id);
        assert_eq!(event.task_type, "scrape");
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.error_message, "connection timeout");
        assert_eq!(event.retry_count, 3);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "TaskFailed");
    }

    #[test]
    fn test_task_failed_event_domain_event_trait() {
        let task_id = Uuid::new_v4();
        let event = TaskFailedEvent::new(
            task_id,
            "scrape".to_string(),
            Uuid::new_v4(),
            "error".to_string(),
            1,
        );
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "TaskFailed");
        assert_eq!(dyn_event.aggregate_id(), task_id);
        assert_eq!(dyn_event.aggregate_type(), "Task");
    }

    #[test]
    fn test_task_failed_event_serde_roundtrip() {
        let event = TaskFailedEvent::new(
            Uuid::new_v4(),
            "crawl".to_string(),
            Uuid::new_v4(),
            "dns error".to_string(),
            2,
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: TaskFailedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.error_message, event.error_message);
        assert_eq!(back.retry_count, event.retry_count);
    }

    // ========== CrawlStartedEvent tests ==========

    #[test]
    fn test_crawl_started_event_new_populates_fields() {
        let crawl_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = CrawlStartedEvent::new(
            crawl_id,
            team_id,
            "https://example.com".to_string(),
            100,
        );

        assert_eq!(event.crawl_id, crawl_id);
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.root_url, "https://example.com");
        assert_eq!(event.estimated_pages, 100);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "CrawlStarted");
        assert_eq!(event.metadata.aggregate_type, "Crawl");
        assert_eq!(event.metadata.aggregate_id, crawl_id);
    }

    #[test]
    fn test_crawl_started_event_domain_event_trait() {
        let crawl_id = Uuid::new_v4();
        let event = CrawlStartedEvent::new(
            crawl_id,
            Uuid::new_v4(),
            "https://example.com".to_string(),
            50,
        );
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "CrawlStarted");
        assert_eq!(dyn_event.aggregate_id(), crawl_id);
        assert_eq!(dyn_event.aggregate_type(), "Crawl");
    }

    #[test]
    fn test_crawl_started_event_serde_roundtrip() {
        let event = CrawlStartedEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            200,
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: CrawlStartedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.root_url, event.root_url);
        assert_eq!(back.estimated_pages, event.estimated_pages);
    }

    // ========== CrawlCompletedEvent tests ==========

    #[test]
    fn test_crawl_completed_event_new_populates_fields() {
        let crawl_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = CrawlCompletedEvent::new(crawl_id, team_id, 100, 95, 5, 60000);

        assert_eq!(event.crawl_id, crawl_id);
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.total_tasks, 100);
        assert_eq!(event.completed_tasks, 95);
        assert_eq!(event.failed_tasks, 5);
        assert_eq!(event.total_duration_ms, 60000);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "CrawlCompleted");
    }

    #[test]
    fn test_crawl_completed_event_domain_event_trait() {
        let crawl_id = Uuid::new_v4();
        let event = CrawlCompletedEvent::new(crawl_id, Uuid::new_v4(), 10, 8, 2, 1000);
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "CrawlCompleted");
        assert_eq!(dyn_event.aggregate_id(), crawl_id);
        assert_eq!(dyn_event.aggregate_type(), "Crawl");
    }

    #[test]
    fn test_crawl_completed_event_serde_roundtrip() {
        let event = CrawlCompletedEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            50,
            45,
            5,
            30000,
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: CrawlCompletedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.total_tasks, event.total_tasks);
        assert_eq!(back.completed_tasks, event.completed_tasks);
        assert_eq!(back.failed_tasks, event.failed_tasks);
    }

    // ========== CreditsDeductedEvent tests ==========

    #[test]
    fn test_credits_deducted_event_new_populates_fields() {
        let team_id = Uuid::new_v4();
        let resource_id = Uuid::new_v4();
        let event = CreditsDeductedEvent::new(
            team_id,
            10,
            90,
            "scrape".to_string(),
            resource_id,
            "task".to_string(),
        );

        assert_eq!(event.team_id, team_id);
        assert_eq!(event.amount, 10);
        assert_eq!(event.remaining_credits, 90);
        assert_eq!(event.operation_type, "scrape");
        assert_eq!(event.resource_id, resource_id);
        assert_eq!(event.resource_type, "task");
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "CreditsDeducted");
        assert_eq!(event.metadata.aggregate_type, "Credits");
        assert_eq!(event.metadata.aggregate_id, team_id);
    }

    #[test]
    fn test_credits_deducted_event_domain_event_trait() {
        let team_id = Uuid::new_v4();
        let event = CreditsDeductedEvent::new(
            team_id,
            5,
            95,
            "search".to_string(),
            Uuid::new_v4(),
            "query".to_string(),
        );
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "CreditsDeducted");
        assert_eq!(dyn_event.aggregate_id(), team_id);
        assert_eq!(dyn_event.aggregate_type(), "Credits");
    }

    #[test]
    fn test_credits_deducted_event_serde_roundtrip() {
        let event = CreditsDeductedEvent::new(
            Uuid::new_v4(),
            15,
            85,
            "crawl".to_string(),
            Uuid::new_v4(),
            "crawl_run".to_string(),
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: CreditsDeductedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.amount, event.amount);
        assert_eq!(back.remaining_credits, event.remaining_credits);
        assert_eq!(back.operation_type, event.operation_type);
    }

    // ========== CreditsLowEvent tests ==========

    #[test]
    fn test_credits_low_event_new_populates_fields() {
        let team_id = Uuid::new_v4();
        let event = CreditsLowEvent::new(team_id, 50, 100);

        assert_eq!(event.team_id, team_id);
        assert_eq!(event.current_credits, 50);
        assert_eq!(event.threshold, 100);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "CreditsLow");
        assert_eq!(event.metadata.aggregate_type, "Credits");
        assert_eq!(event.metadata.aggregate_id, team_id);
    }

    #[test]
    fn test_credits_low_event_domain_event_trait() {
        let team_id = Uuid::new_v4();
        let event = CreditsLowEvent::new(team_id, 10, 50);
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "CreditsLow");
        assert_eq!(dyn_event.aggregate_id(), team_id);
        assert_eq!(dyn_event.aggregate_type(), "Credits");
    }

    #[test]
    fn test_credits_low_event_serde_roundtrip() {
        let event = CreditsLowEvent::new(Uuid::new_v4(), 20, 100);
        let json = serde_json::to_string(&event).expect("serialize");
        let back: CreditsLowEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.current_credits, event.current_credits);
        assert_eq!(back.threshold, event.threshold);
    }

    // ========== SearchCompletedEvent tests ==========

    #[test]
    fn test_search_completed_event_new_populates_fields() {
        let query_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let event = SearchCompletedEvent::new(
            query_id,
            team_id,
            "rust web scraping".to_string(),
            "google".to_string(),
            25,
            500,
        );

        assert_eq!(event.query_id, query_id);
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.query, "rust web scraping");
        assert_eq!(event.engine, "google");
        assert_eq!(event.results_count, 25);
        assert_eq!(event.duration_ms, 500);
        assert_eq!(event.metadata.tenant_id, Some(team_id));
        assert_eq!(event.metadata.event_type, "SearchCompleted");
        assert_eq!(event.metadata.aggregate_type, "Search");
        assert_eq!(event.metadata.aggregate_id, query_id);
    }

    #[test]
    fn test_search_completed_event_domain_event_trait() {
        let query_id = Uuid::new_v4();
        let event = SearchCompletedEvent::new(
            query_id,
            Uuid::new_v4(),
            "query".to_string(),
            "bing".to_string(),
            10,
            200,
        );
        let dyn_event: &dyn DomainEvent = &event;
        assert_eq!(dyn_event.event_type(), "SearchCompleted");
        assert_eq!(dyn_event.aggregate_id(), query_id);
        assert_eq!(dyn_event.aggregate_type(), "Search");
    }

    #[test]
    fn test_search_completed_event_serde_roundtrip() {
        let event = SearchCompletedEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test query".to_string(),
            "baidu".to_string(),
            5,
            100,
        );
        let json = serde_json::to_string(&event).expect("serialize");
        let back: SearchCompletedEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.query, event.query);
        assert_eq!(back.engine, event.engine);
        assert_eq!(back.results_count, event.results_count);
        assert_eq!(back.duration_ms, event.duration_ms);
    }

    // ========== Cross-event consistency tests ==========

    #[test]
    fn test_all_task_events_use_task_aggregate_type() {
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();

        let created = TaskCreatedEvent::new(
            task_id,
            "scrape".to_string(),
            "url".to_string(),
            team_id,
            1,
        );
        let completed = TaskCompletedEvent::new(
            task_id,
            "scrape".to_string(),
            team_id,
            100,
            serde_json::json!({}),
        );
        let failed = TaskFailedEvent::new(
            task_id,
            "scrape".to_string(),
            team_id,
            "err".to_string(),
            1,
        );

        for ev in [&created as &dyn DomainEvent, &completed, &failed] {
            assert_eq!(ev.aggregate_type(), "Task");
            assert_eq!(ev.aggregate_id(), task_id);
        }
        assert_eq!(created.event_type(), "TaskCreated");
        assert_eq!(completed.event_type(), "TaskCompleted");
        assert_eq!(failed.event_type(), "TaskFailed");
    }

    #[test]
    fn test_all_crawl_events_use_crawl_aggregate_type() {
        let crawl_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();

        let started = CrawlStartedEvent::new(crawl_id, team_id, "url".to_string(), 10);
        let completed = CrawlCompletedEvent::new(crawl_id, team_id, 10, 8, 2, 1000);

        for ev in [&started as &dyn DomainEvent, &completed] {
            assert_eq!(ev.aggregate_type(), "Crawl");
            assert_eq!(ev.aggregate_id(), crawl_id);
        }
    }

    #[test]
    fn test_all_credits_events_use_credits_aggregate_type() {
        let team_id = Uuid::new_v4();

        let deducted = CreditsDeductedEvent::new(
            team_id,
            5,
            95,
            "scrape".to_string(),
            Uuid::new_v4(),
            "task".to_string(),
        );
        let low = CreditsLowEvent::new(team_id, 10, 50);

        for ev in [&deducted as &dyn DomainEvent, &low] {
            assert_eq!(ev.aggregate_type(), "Credits");
            assert_eq!(ev.aggregate_id(), team_id);
        }
    }

    #[test]
    fn test_search_event_uses_search_aggregate_type() {
        let query_id = Uuid::new_v4();
        let event = SearchCompletedEvent::new(
            query_id,
            Uuid::new_v4(),
            "q".to_string(),
            "google".to_string(),
            1,
            10,
        );
        let ev: &dyn DomainEvent = &event;
        assert_eq!(ev.aggregate_type(), "Search");
        assert_eq!(ev.aggregate_id(), query_id);
    }
}
