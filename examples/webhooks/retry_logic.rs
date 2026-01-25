// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 重试逻辑示例
//!
//! 演示如何配置Webhook重试策略。
//!
//! ## 运行
//!
//! ```bash
//! cargo run --bin retry_logic
//! ```
//!
//! ## 核心功能
//!
//! - 指数退避重试策略
//! - 最大重试次数配置
//! - 重试条件判断

use tracing::{info, warn};
use std::time::Duration;
use rand::Rng;

// 重试策略配置
#[derive(Debug, Clone)]
struct RetryPolicy {
    max_retries: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    backoff_multiplier: f64,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, initial_delay_ms: u64) -> Self {
        Self {
            max_retries,
            initial_delay_ms,
            max_delay_ms: 60000, // 60秒最大延迟
            backoff_multiplier: 2.0,
        }
    }

    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay = self.initial_delay_ms as f64 * self.backoff_multiplier.powf(attempt as f64);
        let delay = delay.min(self.max_delay_ms as f64);
        Duration::from_millis(delay as u64)
    }

    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }
}

// Webhook 发送结果
#[derive(Debug, Clone)]
enum DeliveryResult {
    Success,
    TransientFailure { reason: String }, // 可重试
    PermanentFailure { reason: String }, // 不可重试
}

// 模拟的 Webhook 发送器
struct WebhookDelivery {
    url: String,
    policy: RetryPolicy,
    total_attempts: u32,
    successful_attempts: u32,
    failed_attempts: u32,
}

impl WebhookDelivery {
    pub fn new(url: &str, policy: RetryPolicy) -> Self {
        Self {
            url: url.to_string(),
            policy,
            total_attempts: 0,
            successful_attempts: 0,
            failed_attempts: 0,
        }
    }

    async fn send_with_retry(&mut self, _payload: &str) -> DeliveryResult {
        let mut attempt = 0;

        loop {
            attempt += 1;
            self.total_attempts += 1;

            info!("Attempt {} for {}", attempt, self.url);

            // 模拟发送结果（随机）
            let result = self.simulate_send();

            match result {
                DeliveryResult::Success => {
                    self.successful_attempts += 1;
                    info!("✅ Successfully delivered to {}", self.url);
                    return DeliveryResult::Success;
                }
                DeliveryResult::TransientFailure { reason } => {
                    if self.policy.should_retry(attempt) {
                        let delay = self.policy.calculate_delay(attempt);
                        warn!("Transient failure: {}. Retrying in {}ms", reason, delay.as_millis());
                        tokio::time::sleep(delay).await;
                        continue;
                    } else {
                        self.failed_attempts += 1;
                        warn!("Max retries exceeded for {}", self.url);
                        return result;
                    }
                }
                DeliveryResult::PermanentFailure { ref reason } => {
                    self.failed_attempts += 1;
                    warn!("Permanent failure: {}. No retry.", reason);
                    return result;
                }
            }
        }
    }

    fn simulate_send(&self) -> DeliveryResult {
        let mut rng = rand::thread_rng();
        let roll: f32 = rng.gen();

        match roll {
            r if r < 0.7 => DeliveryResult::Success,
            r if r < 0.85 => DeliveryResult::TransientFailure {
                reason: "Connection timeout".to_string()
            },
            _ => DeliveryResult::TransientFailure {
                reason: "Server busy (503)".to_string()
            },
        }
    }

    fn get_stats(&self) -> (u32, u32, u32) {
        (self.total_attempts, self.successful_attempts, self.failed_attempts)
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("=== Webhook 重试逻辑示例 ===\n");

    // 创建重试策略（最多3次重试，初始延迟1秒）
    let policy = RetryPolicy::new(3, 1000);

    info!("Retry policy:");
    info!("  Max retries: {}", policy.max_retries);
    info!("  Initial delay: {}ms", policy.initial_delay_ms);
    info!("  Max delay: {}ms", policy.max_delay_ms);
    info!("  Backoff multiplier: {}", policy.backoff_multiplier);

    // 演示延迟计算
    info!("\n--- 延迟计算演示 ---");
    for i in 0..5 {
        let delay = policy.calculate_delay(i);
        info!("  Attempt {}: {}ms", i + 1, delay.as_millis());
    }

    // 模拟发送
    let mut delivery = WebhookDelivery::new(
        "https://api.example.com/webhook",
        policy
    );

    info!("\n--- 模拟发送测试 ---");
    let _ = delivery.send_with_retry(r#"{"event": "task.completed"}"#).await;

    // 显示统计
    let (total, success, failed) = delivery.get_stats();
    info!("\n--- 发送统计 ---");
    info!("Total attempts: {}", total);
    info!("Successful: {}", success);
    info!("Failed: {}", failed);

    info!("\n=== 重试逻辑示例完成 ===");
}
