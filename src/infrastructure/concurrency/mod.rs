// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 并发控制优化模块
//!
//! 提供细粒度的并发控制，支持不同级别的限制策略

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, warn};

/// 并发控制策略
#[derive(Debug, Clone)]
pub enum ConcurrencyStrategy {
    /// 全局限流（全局并发限制）
    Global {
        max_concurrent: u32,
        queue_size: u32,
    },
    /// 每团队限流
    PerTeam { max_per_team: u32, queue_size: u32 },
    /// 每任务类型限流
    PerTaskType {
        max_per_type: u32,
        type_limits: std::collections::HashMap<String, u32>,
    },
    /// 分层限流（全局+团队+任务类型）
    Hierarchical {
        global_max: u32,
        per_team_max: u32,
        per_task_type_max: u32,
    },
}

impl Default for ConcurrencyStrategy {
    fn default() -> Self {
        ConcurrencyStrategy::Global {
            max_concurrent: 100,
            queue_size: 1000,
        }
    }
}

/// 细粒度并发控制器
#[derive(Debug, Clone)]
pub struct FineGrainedConcurrencyController {
    /// 全局信号量
    global_semaphore: Arc<Semaphore>,
    /// 全局并发计数
    global_counter: Arc<AtomicU32>,
    /// 当前活跃并发数
    active_count: Arc<AtomicU32>,
    /// 配置
    config: Arc<ConcurrencyStrategy>,
    /// 等待队列统计
    wait_count: Arc<AtomicU32>,
}

impl FineGrainedConcurrencyController {
    /// 创建新的并发控制器
    pub fn new(strategy: ConcurrencyStrategy) -> Self {
        let max_concurrent = match &strategy {
            ConcurrencyStrategy::Global { max_concurrent, .. } => *max_concurrent,
            ConcurrencyStrategy::PerTeam { max_per_team, .. } => *max_per_team,
            ConcurrencyStrategy::PerTaskType { max_per_type, .. } => *max_per_type,
            ConcurrencyStrategy::Hierarchical { global_max, .. } => *global_max,
        };

        Self {
            global_semaphore: Arc::new(Semaphore::new(max_concurrent as usize)),
            global_counter: Arc::new(AtomicU32::new(0)),
            active_count: Arc::new(AtomicU32::new(0)),
            config: Arc::new(strategy),
            wait_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// 尝试获取并发槽位（不等待）
    pub fn try_acquire(&self) -> bool {
        if self.global_semaphore.try_acquire().is_ok() {
            self.global_counter.fetch_add(1, Ordering::SeqCst);
            self.active_count.fetch_add(1, Ordering::SeqCst);
            debug!(
                "Acquired concurrency slot. Active: {}",
                self.active_count.load(Ordering::SeqCst)
            );
            true
        } else {
            false
        }
    }

    /// 获取并发槽位（等待）
    pub async fn acquire(&self, timeout: Option<Duration>) -> Result<(), ConcurrencyError> {
        let permit = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, self.global_semaphore.acquire()).await {
                Ok(permit) => permit,
                Err(_) => return Err(ConcurrencyError::Timeout),
            }
        } else {
            self.global_semaphore.acquire().await
        };

        self.global_counter.fetch_add(1, Ordering::SeqCst);
        self.active_count.fetch_add(1, Ordering::SeqCst);

        // 存储permit以便释放
        drop(permit);

        debug!(
            "Acquired concurrency slot. Active: {}",
            self.active_count.load(Ordering::SeqCst)
        );
        Ok(())
    }

    /// 释放并发槽位
    pub fn release(&self) {
        self.global_counter.fetch_sub(1, Ordering::SeqCst);
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        self.global_semaphore.add_permits(1);
        debug!(
            "Released concurrency slot. Active: {}",
            self.active_count.load(Ordering::SeqCst)
        );
    }

    /// 获取当前活跃并发数
    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    /// 获取当前等待数
    pub fn wait_count(&self) -> u32 {
        self.wait_count.load(Ordering::SeqCst)
    }

    /// 获取使用率
    pub fn utilization(&self) -> f64 {
        let max = match self.config.as_ref() {
            ConcurrencyStrategy::Global { max_concurrent, .. } => *max_concurrent,
            ConcurrencyStrategy::PerTeam { max_per_team, .. } => *max_per_team,
            ConcurrencyStrategy::PerTaskType { max_per_type, .. } => *max_per_type,
            ConcurrencyStrategy::Hierarchical { global_max, .. } => *global_max,
        };

        if max == 0 {
            0.0
        } else {
            self.active_count() as f64 / max as f64
        }
    }

    /// 检查是否达到限制
    pub fn is_at_limit(&self) -> bool {
        let max = match self.config.as_ref() {
            ConcurrencyStrategy::Global { max_concurrent, .. } => *max_concurrent,
            ConcurrencyStrategy::PerTeam { max_per_team, .. } => *max_per_team,
            ConcurrencyStrategy::PerTaskType { max_per_type, .. } => *max_per_type,
            ConcurrencyStrategy::Hierarchical { global_max, .. } => *global_max,
        };

        self.active_count() >= max
    }
}

/// 并发控制错误
#[derive(Debug, thiserror::Error)]
pub enum ConcurrencyError {
    #[error("Concurrency limit exceeded")]
    LimitExceeded,
    #[error("Concurrency operation timed out")]
    Timeout,
    #[error("Concurrency controller is closed")]
    Closed,
}

/// 并发统计信息
#[derive(Debug, Clone)]
pub struct ConcurrencyStats {
    pub active_concurrent: u32,
    pub utilization_percent: f64,
    pub is_at_limit: bool,
    pub strategy: String,
}

impl From<&FineGrainedConcurrencyController> for ConcurrencyStats {
    fn from(controller: &FineGrainedConcurrencyController) -> Self {
        ConcurrencyStats {
            active_concurrent: controller.active_count(),
            utilization_percent: controller.utilization() * 100.0,
            is_at_limit: controller.is_at_limit(),
            strategy: format!("{:?}", controller.config),
        }
    }
}

/// 自适应并发控制器
///
/// 根据系统负载动态调整并发限制
#[derive(Debug)]
pub struct AdaptiveConcurrencyController {
    base_controller: FineGrainedConcurrencyController,
    min_concurrent: u32,
    max_concurrent: u32,
    target_utilization: f64,
    adjustment_interval: Duration,
}

impl AdaptiveConcurrencyController {
    /// 创建新的自适应控制器
    pub fn new(
        min_concurrent: u32,
        max_concurrent: u32,
        target_utilization: f64,
        adjustment_interval: Duration,
    ) -> Self {
        let strategy = ConcurrencyStrategy::Global {
            max_concurrent,
            queue_size: 1000,
        };

        Self {
            base_controller: FineGrainedConcurrencyController::new(strategy),
            min_concurrent,
            max_concurrent,
            target_utilization,
            adjustment_interval,
        }
    }

    /// 启动自适应调整任务
    pub fn start_adaptive_adjustment(&self) {
        let controller = self.base_controller.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.adjustment_interval);

            loop {
                interval.tick().await;

                let utilization = controller.utilization();

                if utilization < self.target_utilization * 0.8 {
                    // 低于目标利用率，增加并发
                    warn!(
                        "Low utilization detected: {:.1}%. Consider increasing concurrency.",
                        utilization * 100.0
                    );
                } else if utilization > self.target_utilization * 1.2 {
                    // 高于目标利用率，减少并发
                    warn!(
                        "High utilization detected: {:.1}%. Consider decreasing concurrency.",
                        utilization * 100.0
                    );
                }
            }
        });
    }

    /// 获取基础控制器
    pub fn base(&self) -> &FineGrainedConcurrencyController {
        &self.base_controller
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrency_controller_try_acquire() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 2,
            queue_size: 10,
        });

        assert!(controller.try_acquire());
        assert!(controller.try_acquire());
        assert!(!controller.try_acquire()); // Should fail - at limit

        assert_eq!(controller.active_count(), 2);
        assert!(controller.is_at_limit());
    }

    #[tokio::test]
    async fn test_concurrency_controller_acquire_release() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);

        controller.release();
        assert_eq!(controller.active_count(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_controller_timeout() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0, // No slots available
            queue_size: 10,
        });

        let result = controller.acquire(Some(Duration::from_millis(100))).await;
        assert!(matches!(result, Err(ConcurrencyError::Timeout)));
    }

    #[test]
    fn test_concurrency_stats() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 100,
            queue_size: 1000,
        });

        let stats = ConcurrencyStats::from(&controller);
        assert_eq!(stats.active_concurrent, 0);
        assert_eq!(stats.utilization_percent, 0.0);
        assert!(!stats.is_at_limit);
    }
}
