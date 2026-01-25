// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 基础Webhook配置示例
//!
//! 演示如何配置Webhook通知。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin basic_webhook
//! ```
//!
//! ## 核心功能
//!
//! - Webhook端点注册
//! - 事件类型选择
//! - 负载配置

use tracing::{info, warn};
use uuid::Uuid;
use std::time::Duration;

// 支持的事件类型
#[derive(Debug, Clone)]
enum WebhookEvent {
    TaskCreated,
    TaskStarted,
    TaskProgress { progress: u8 },
    TaskCompleted,
    TaskFailed { error: String },
    TaskCancelled,
}

// Webhook 配置
#[derive(Debug, Clone)]
struct WebhookConfig {
    id: Uuid,
    url: String,
    secret: String,
    events: Vec<WebhookEvent>,
    timeout_ms: u64,
    enabled: bool,
}

impl WebhookConfig {
    pub fn new(url: &str, secret: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            url: url.to_string(),
            secret: secret.to_string(),
            events: Vec::new(),
            timeout_ms: 5000,
            enabled: true,
        }
    }

    pub fn add_event(&mut self, event: WebhookEvent) {
        self.events.push(event.clone());
        info!("Added event to webhook {}: {:?}", self.id, event);
    }

    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// 模拟的 Webhook 发送器
#[derive(Debug)]
struct WebhookSender {
    config: WebhookConfig,
    delivery_count: usize,
    success_count: usize,
}

impl WebhookSender {
    pub fn new(config: WebhookConfig) -> Self {
        Self {
            config,
            delivery_count: 0,
            success_count: 0,
        }
    }

    // 模拟发送 Webhook
    pub async fn send(&mut self, event: &WebhookEvent, payload: &str) -> bool {
        if !self.config.is_enabled() {
            warn!("Webhook {} is disabled, skipping delivery", self.config.id);
            return false;
        }

        self.delivery_count += 1;
        info!("Sending webhook to {} for event: {:?}", self.config.url, event);
        info!("  Payload: {} bytes", payload.len());
        info!("  Timeout: {}ms", self.config.timeout_ms);

        // 模拟成功（90% 成功率）
        let success = rand::random::<f32>() < 0.9;
        if success {
            self.success_count += 1;
            info!("  ✅ Delivery successful");
        } else {
            warn!("  ❌ Delivery failed");
        }

        success
    }

    pub fn get_stats(&self) -> (usize, usize) {
        (self.delivery_count, self.success_count)
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("=== 基础 Webhook 配置示例 ===\n");

    // 创建 Webhook 配置
    let mut webhook_config = WebhookConfig::new(
        "https://your-server.com/webhook/crawlrs",
        "whsec_secret_key_here"
    );

    info!("Created webhook: {}", webhook_config.id);
    info!("URL: {}", webhook_config.url);

    // 配置事件类型
    info!("\n--- 配置事件类型 ---");
    webhook_config.add_event(WebhookEvent::TaskCompleted);
    webhook_config.add_event(WebhookEvent::TaskFailed { error: String::new() });
    webhook_config.add_event(WebhookEvent::TaskProgress { progress: 50 });

    // 配置超时
    webhook_config.set_timeout(10000);

    // 创建发送器
    let mut sender = WebhookSender::new(webhook_config);

    // 模拟发送 Webhook
    info!("\n--- 模拟发送 Webhook ---");

    let _ = sender.send(
        &WebhookEvent::TaskCompleted,
        r#"{"task_id": "uuid", "status": "completed", "result_count": 100}"#
    ).await;

    let _ = sender.send(
        &WebhookEvent::TaskProgress { progress: 50 },
        r#"{"task_id": "uuid", "progress": 50}"#
    ).await;

    let _ = sender.send(
        &WebhookEvent::TaskFailed { error: "Timeout".to_string() },
        r#"{"task_id": "uuid", "status": "failed", "error": "Timeout"}"#
    ).await;

    // 显示统计
    let (delivered, success) = sender.get_stats();
    info!("\n--- 发送统计 ---");
    info!("Total deliveries: {}", delivered);
    info!("Successful: {}", success);
    info!("Failed: {}", delivered - success);

    info!("\n=== Webhook 示例完成 ===");
}
