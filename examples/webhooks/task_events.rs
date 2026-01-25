// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 任务事件订阅示例
//!
//! 演示如何订阅特定的任务事件。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin task_events
//! ```
//!
//! ## 核心功能
//!
//! - 订阅任务生命周期事件
//! - 处理事件负载
//! - 事件过滤和路由

use tracing::info;
use uuid::Uuid;
use std::collections::HashMap;

// 任务状态
#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Pending,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

// 任务事件类型
#[derive(Debug, Clone)]
enum TaskEvent {
    Created { task_id: Uuid, url: String },
    Started { task_id: Uuid },
    Progress { task_id: Uuid, pages_scraped: u32 },
    Completed { task_id: Uuid, pages_scraped: u32, success: bool },
    Failed { task_id: Uuid, error: String },
    Cancelled { task_id: Uuid, reason: String },
}

// 事件订阅配置
#[derive(Debug, Clone)]
struct EventSubscription {
    id: Uuid,
    name: String,
    webhook_url: String,
    events: Vec<TaskStatus>,
    filters: HashMap<String, String>,
}

impl EventSubscription {
    pub fn new(name: &str, webhook_url: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            webhook_url: webhook_url.to_string(),
            events: Vec::new(),
            filters: HashMap::new(),
        }
    }

    pub fn subscribe_to(&mut self, status: TaskStatus) {
        self.events.push(status);
    }

    pub fn add_filter(&mut self, key: &str, value: &str) {
        self.filters.insert(key.to_string(), value.to_string());
    }

    pub fn matches(&self, event: &TaskEvent) -> bool {
        // 检查事件类型是否匹配
        let event_status = match event {
            TaskEvent::Created { .. } => TaskStatus::Pending,
            TaskEvent::Started { .. } => TaskStatus::Running,
            TaskEvent::Progress { .. } => TaskStatus::Running,
            TaskEvent::Completed { .. } => TaskStatus::Completed,
            TaskEvent::Failed { .. } => TaskStatus::Failed,
            TaskEvent::Cancelled { .. } => TaskStatus::Cancelled,
        };

        if !self.events.contains(&event_status) {
            return false;
        }

        // 检查过滤器（简化版本）
        true
    }
}

// 事件路由器
#[derive(Debug)]
struct EventRouter {
    subscriptions: Vec<EventSubscription>,
}

impl EventRouter {
    pub fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
        }
    }

    pub fn add_subscription(&mut self, subscription: &EventSubscription) {
        self.subscriptions.push(subscription.clone());
        info!("Added subscription: {} -> {}", subscription.name, subscription.webhook_url);
    }

    pub fn route_event(&self, event: &TaskEvent) {
        for sub in &self.subscriptions {
            if sub.matches(event) {
                info!("📨 Routing event {:?} to {}", event, sub.webhook_url);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("=== 任务事件订阅示例 ===\n");

    // 创建事件路由器
    let mut router = EventRouter::new();

    // 创建订阅
    let mut completion_sub = EventSubscription::new(
        "Completion Notifications",
        "https://api.example.com/webhooks/completions"
    );
    completion_sub.subscribe_to(TaskStatus::Completed);
    completion_sub.subscribe_to(TaskStatus::Failed);

    let mut progress_sub = EventSubscription::new(
        "Progress Tracker",
        "https://api.example.com/webhooks/progress"
    );
    progress_sub.subscribe_to(TaskStatus::Running);
    progress_sub.add_filter("url_pattern", "https://example.com/*");

    // 添加订阅
    router.add_subscription(&completion_sub);
    router.add_subscription(&progress_sub);

    // 模拟任务事件
    info!("\n--- 模拟任务事件 ---");

    let task_id = Uuid::new_v4();

    router.route_event(&TaskEvent::Created {
        task_id,
        url: "https://example.com/page1".to_string(),
    });

    router.route_event(&TaskEvent::Started { task_id });

    router.route_event(&TaskEvent::Progress {
        task_id,
        pages_scraped: 10,
    });

    router.route_event(&TaskEvent::Progress {
        task_id,
        pages_scraped: 50,
    });

    router.route_event(&TaskEvent::Completed {
        task_id,
        pages_scraped: 100,
        success: true,
    });

    info!("\n=== 任务事件订阅示例完成 ===");
}
