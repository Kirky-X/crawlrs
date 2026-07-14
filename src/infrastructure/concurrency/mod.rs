// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 并发控制优化模块
//!
//! 提供细粒度的并发控制，支持不同级别的限制策略

pub mod controller;

use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

/// 并发控制许可（RAII Guard）
///
/// 当此 guard 被 drop 时，自动释放并发槽位。
/// 这确保了并发控制正确执行，不会出现 permit 泄漏。
#[derive(Debug)]
pub struct ConcurrencyPermit {
    /// 底层的信号量许可
    _permit: OwnedSemaphorePermit,
    /// 活跃计数器的引用（用于统计）
    active_count: Arc<AtomicU32>,
    /// 全局计数器的引用（用于统计）
    global_counter: Arc<AtomicU32>,
}

/// 并发控制许可的 RAII 守卫（用于 try_acquire）
///
/// 此 guard 持有一个信号量许可，并在 drop 时自动释放。
#[derive(Debug)]
pub struct ConcurrencyGuard {
    /// 底层的信号量许可
    _permit: tokio::sync::SemaphorePermit<'static>,
    /// 活跃计数器的引用
    active_count: Arc<AtomicU32>,
    /// 全局计数器的引用
    global_counter: Arc<AtomicU32>,
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

/// 自适应并发控制器
///
/// 根据系统负载动态调整并发限制
#[derive(Debug)]
#[allow(dead_code)]
pub struct AdaptiveConcurrencyController {
    base_controller: FineGrainedConcurrencyController,
    min_concurrent: u32,
    max_concurrent: u32,
    target_utilization: f64,
    adjustment_interval: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_concurrency_controller_try_acquire() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 2,
            queue_size: 10,
        });

        // 获取两个许可
        let guard1 = controller.try_acquire();
        assert!(guard1.is_some());

        let guard2 = controller.try_acquire();
        assert!(guard2.is_some());

        // 第三个应该失败（已达到限制）
        let guard3 = controller.try_acquire();
        assert!(guard3.is_none());

        assert_eq!(controller.active_count(), 2);
        assert!(controller.is_at_limit());

        // 释放一个许可
        drop(guard1);
        tokio::task::yield_now().await; // 让 drop 完成

        assert_eq!(controller.active_count(), 1);
        assert!(!controller.is_at_limit());

        // 现在应该可以获取新的许可
        let guard4 = controller.try_acquire();
        assert!(guard4.is_some());
        assert_eq!(controller.active_count(), 2);
    }

    #[tokio::test]
    async fn test_concurrency_controller_acquire_release() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        {
            let _permit = controller.acquire(None).await.unwrap();
            assert_eq!(controller.active_count(), 1);
            // permit 在此作用域结束时自动释放
        }

        tokio::task::yield_now().await; // 让 drop 完成
        assert_eq!(controller.active_count(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_controller_timeout() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        // 先获取一个许可
        let _permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);

        // 尝试获取第二个许可，应该超时
        let result = controller.acquire(Some(Duration::from_millis(100))).await;
        assert!(matches!(result, Err(ConcurrencyError::Timeout)));
    }

    #[tokio::test]
    async fn test_concurrency_permit_drop_releases_slot() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        // 获取许可
        let permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);

        // 尝试获取第二个应该失败
        assert!(controller.try_acquire().is_none());

        // 释放许可
        drop(permit);
        tokio::task::yield_now().await;

        // 现在应该可以获取
        assert_eq!(controller.active_count(), 0);
        assert!(controller.try_acquire().is_some());
    }

    #[tokio::test]
    async fn test_concurrency_waits_when_at_limit() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        let permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);

        // 在另一个任务中尝试获取许可
        let controller_clone = controller.clone();
        let handle = tokio::spawn(async move {
            // 这应该等待直到有可用的许可
            let _p = controller_clone.acquire(None).await.unwrap();
            controller_clone.active_count()
        });

        // 等待一小段时间，确保任务已经开始等待
        sleep(Duration::from_millis(50)).await;

        // 释放许可，允许等待的任务获取
        drop(permit);

        // 等待任务完成
        let result = handle.await.unwrap();
        assert_eq!(result, 1);
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

    #[tokio::test]
    async fn test_concurrency_guard_drop_releases_slot() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 2,
            queue_size: 10,
        });

        // 使用 try_acquire 获取 guard
        {
            let guard1 = controller.try_acquire();
            assert!(guard1.is_some());
            assert_eq!(controller.active_count(), 1);

            let guard2 = controller.try_acquire();
            assert!(guard2.is_some());
            assert_eq!(controller.active_count(), 2);

            // guards 在作用域结束时自动释放
        }

        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_multiple_acquire_release() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 3,
            queue_size: 10,
        });

        // 获取多个许可
        let p1 = controller.acquire(None).await.unwrap();
        let p2 = controller.acquire(None).await.unwrap();
        let p3 = controller.acquire(None).await.unwrap();

        assert_eq!(controller.active_count(), 3);
        assert!(controller.is_at_limit());

        // 释放一个
        drop(p1);
        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 2);

        // 释放另一个
        drop(p2);
        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 1);

        // 释放最后一个
        drop(p3);
        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 0);
    }

    #[test]
    fn test_concurrency_strategy_default() {
        let strategy = ConcurrencyStrategy::default();
        match strategy {
            ConcurrencyStrategy::Global {
                max_concurrent,
                queue_size,
            } => {
                assert_eq!(max_concurrent, 100);
                assert_eq!(queue_size, 1000);
            }
            _ => panic!("Default strategy should be Global"),
        }
    }

    #[test]
    fn test_concurrency_controller_per_team_strategy() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTeam {
            max_per_team: 5,
            queue_size: 100,
        });

        assert_eq!(controller.active_count(), 0);
        assert!(!controller.is_at_limit());
        assert_eq!(controller.utilization(), 0.0);
        assert_eq!(controller.wait_count(), 0);
    }

    #[test]
    fn test_concurrency_controller_per_task_type_strategy() {
        let mut type_limits = std::collections::HashMap::new();
        type_limits.insert("scrape".to_string(), 10u32);
        type_limits.insert("crawl".to_string(), 5u32);
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTaskType {
            max_per_type: 8,
            type_limits,
        });

        assert_eq!(controller.active_count(), 0);
        assert!(!controller.is_at_limit());
        assert_eq!(controller.utilization(), 0.0);
    }

    #[test]
    fn test_concurrency_controller_hierarchical_strategy() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 50,
            per_team_max: 10,
            per_task_type_max: 5,
        });

        assert_eq!(controller.active_count(), 0);
        assert!(!controller.is_at_limit());
        assert_eq!(controller.utilization(), 0.0);
    }

    #[tokio::test]
    async fn test_concurrency_utilization_per_team_with_active() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTeam {
            max_per_team: 4,
            queue_size: 100,
        });

        let _permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);
        assert!((controller.utilization() - 0.25).abs() < 0.001);
        assert!(!controller.is_at_limit());
    }

    #[tokio::test]
    async fn test_concurrency_utilization_hierarchical_with_active() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 10,
            per_team_max: 5,
            per_task_type_max: 2,
        });

        let _permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);
        assert!((controller.utilization() - 0.1).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_concurrency_is_at_limit_hierarchical() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 2,
            per_team_max: 1,
            per_task_type_max: 1,
        });

        let _p1 = controller.acquire(None).await.unwrap();
        let _p2 = controller.acquire(None).await.unwrap();
        assert!(controller.is_at_limit());

        // try_acquire should fail at limit
        assert!(controller.try_acquire().is_none());
    }

    #[tokio::test]
    async fn test_concurrency_stats_with_active_count() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 4,
            queue_size: 100,
        });

        let _permit = controller.acquire(None).await.unwrap();
        let stats = ConcurrencyStats::from(&controller);
        assert_eq!(stats.active_concurrent, 1);
        assert!((stats.utilization_percent - 25.0).abs() < 0.001);
        assert!(!stats.is_at_limit);
        assert!(stats.strategy.contains("Global"));
    }

    #[tokio::test]
    async fn test_concurrency_stats_at_limit() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 100,
        });

        let _permit = controller.acquire(None).await.unwrap();
        let stats = ConcurrencyStats::from(&controller);
        assert_eq!(stats.active_concurrent, 1);
        assert!((stats.utilization_percent - 100.0).abs() < 0.001);
        assert!(stats.is_at_limit);
    }

    #[test]
    fn test_concurrency_stats_strategy_string_per_team() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTeam {
            max_per_team: 5,
            queue_size: 10,
        });
        let stats = ConcurrencyStats::from(&controller);
        assert!(stats.strategy.contains("PerTeam"));
    }

    #[test]
    fn test_concurrency_error_display_messages() {
        let err = ConcurrencyError::LimitExceeded;
        assert_eq!(err.to_string(), "Concurrency limit exceeded");

        let err = ConcurrencyError::Timeout;
        assert_eq!(err.to_string(), "Concurrency operation timed out");

        let err = ConcurrencyError::Closed;
        assert_eq!(err.to_string(), "Concurrency controller is closed");
    }

    #[tokio::test]
    async fn test_concurrency_acquire_with_timeout_succeeds() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        // Should succeed immediately with a timeout since slot is available
        let result = controller.acquire(Some(Duration::from_secs(1))).await;
        assert!(result.is_ok());
        assert_eq!(controller.active_count(), 1);
    }

    #[test]
    fn test_adaptive_concurrency_controller_new() {
        let controller = AdaptiveConcurrencyController::new(1, 100, 0.7, Duration::from_secs(60));

        assert_eq!(controller.base().active_count(), 0);
        assert_eq!(controller.base().wait_count(), 0);
        assert!(!controller.base().is_at_limit());
    }

    #[test]
    fn test_adaptive_concurrency_controller_base_acquires() {
        let controller = AdaptiveConcurrencyController::new(1, 10, 0.8, Duration::from_secs(30));

        let base = controller.base();
        assert_eq!(base.active_count(), 0);
        assert_eq!(base.utilization(), 0.0);
    }

    #[tokio::test]
    async fn test_adaptive_concurrency_controller_start_adjustment() {
        let controller = AdaptiveConcurrencyController::new(1, 100, 0.7, Duration::from_millis(10));

        // Start the adaptive adjustment task - it spawns a background task
        controller.start_adaptive_adjustment();

        // Give it a moment to run at least one tick
        tokio::time::sleep(Duration::from_millis(30)).await;

        // The base controller should still be accessible
        assert!(controller.base().active_count() < 100);
    }

    #[tokio::test]
    async fn test_concurrency_permit_drop_decrements_global_counter() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 5,
            queue_size: 10,
        });

        {
            let _permit1 = controller.acquire(None).await.unwrap();
            let _permit2 = controller.acquire(None).await.unwrap();
            assert_eq!(controller.active_count(), 2);
        }

        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_try_acquire_then_acquire_after_drop() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        // Use try_acquire first
        let guard = controller.try_acquire();
        assert!(guard.is_some());
        assert_eq!(controller.active_count(), 1);

        // Drop the guard
        drop(guard);
        tokio::task::yield_now().await;
        assert_eq!(controller.active_count(), 0);

        // Now acquire should work
        let _permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);
    }

    // ========== utilization() with max=0 edge cases ==========

    #[test]
    fn test_utilization_global_max_zero_returns_zero() {
        // max_concurrent = 0 → utilization() 的 max == 0 分支 → 返回 0.0
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0,
            queue_size: 10,
        });
        assert_eq!(controller.utilization(), 0.0);
    }

    #[test]
    fn test_utilization_per_team_max_zero_returns_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTeam {
            max_per_team: 0,
            queue_size: 10,
        });
        assert_eq!(controller.utilization(), 0.0);
    }

    #[test]
    fn test_utilization_per_task_type_max_zero_returns_zero() {
        let type_limits = std::collections::HashMap::new();
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTaskType {
            max_per_type: 0,
            type_limits,
        });
        assert_eq!(controller.utilization(), 0.0);
    }

    #[test]
    fn test_utilization_hierarchical_max_zero_returns_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 0,
            per_team_max: 0,
            per_task_type_max: 0,
        });
        assert_eq!(controller.utilization(), 0.0);
    }

    // ========== is_at_limit() with max=0 edge cases ==========

    #[test]
    fn test_is_at_limit_global_max_zero() {
        // max_concurrent = 0 → active_count(0) >= 0 → true
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0,
            queue_size: 10,
        });
        assert!(controller.is_at_limit(), "with max=0, should be at limit");
    }

    #[test]
    fn test_is_at_limit_per_team_max_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTeam {
            max_per_team: 0,
            queue_size: 10,
        });
        assert!(controller.is_at_limit());
    }

    #[test]
    fn test_is_at_limit_hierarchical_max_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 0,
            per_team_max: 0,
            per_task_type_max: 0,
        });
        assert!(controller.is_at_limit());
    }

    // ========== ConcurrencyStats with max=0 ==========

    #[test]
    fn test_concurrency_stats_with_max_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0,
            queue_size: 10,
        });
        let stats = ConcurrencyStats::from(&controller);
        assert_eq!(stats.active_concurrent, 0);
        assert_eq!(stats.utilization_percent, 0.0);
        assert!(stats.is_at_limit);
        assert!(stats.strategy.contains("Global"));
    }

    #[test]
    fn test_concurrency_stats_strategy_string_per_task_type() {
        let mut type_limits = std::collections::HashMap::new();
        type_limits.insert("scrape".to_string(), 10u32);
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::PerTaskType {
            max_per_type: 8,
            type_limits,
        });
        let stats = ConcurrencyStats::from(&controller);
        assert!(stats.strategy.contains("PerTaskType"));
    }

    #[test]
    fn test_concurrency_stats_strategy_string_hierarchical() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Hierarchical {
            global_max: 50,
            per_team_max: 10,
            per_task_type_max: 5,
        });
        let stats = ConcurrencyStats::from(&controller);
        assert!(stats.strategy.contains("Hierarchical"));
    }

    // ========== try_acquire with max=0 ==========

    #[tokio::test]
    async fn test_try_acquire_with_max_zero_returns_none() {
        // max_concurrent = 0 → semaphore 有 0 个许可 → try_acquire 总是失败
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0,
            queue_size: 10,
        });
        assert!(controller.try_acquire().is_none());
        assert_eq!(controller.active_count(), 0);
    }

    // ========== acquire with max=0 should timeout ==========

    #[tokio::test]
    async fn test_acquire_with_max_zero_times_out() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 0,
            queue_size: 10,
        });
        let result = controller.acquire(Some(Duration::from_millis(50))).await;
        assert!(
            matches!(result, Err(ConcurrencyError::Timeout)),
            "should timeout with max=0"
        );
    }

    // ========== wait_count ==========

    #[test]
    fn test_wait_count_starts_zero() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 5,
            queue_size: 10,
        });
        assert_eq!(controller.wait_count(), 0);
    }

    // ========== ConcurrencyPermit and ConcurrencyGuard Drop with cloned controller ==========

    #[tokio::test]
    async fn test_controller_clone_shares_state() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 2,
            queue_size: 10,
        });
        let controller_clone = controller.clone();

        // 使用 clone 获取 permit
        let _permit = controller_clone.acquire(None).await.unwrap();
        // 原始 controller 应看到 active_count = 1（共享状态）
        assert_eq!(controller.active_count(), 1);
        assert_eq!(controller_clone.active_count(), 1);
    }

    #[tokio::test]
    async fn test_controller_clone_try_acquire_shared() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });
        let controller_clone = controller.clone();

        // 原始获取 permit
        let _permit = controller.acquire(None).await.unwrap();
        assert_eq!(controller.active_count(), 1);

        // clone 的 try_acquire 应失败（共享信号量）
        assert!(controller_clone.try_acquire().is_none());
    }

    // ========== Multiple sequential operations ==========

    #[tokio::test]
    async fn test_sequential_acquire_release_cycles() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        for _ in 0..5 {
            {
                let _permit = controller.acquire(None).await.unwrap();
                assert_eq!(controller.active_count(), 1);
                assert!(controller.is_at_limit());
            }
            tokio::task::yield_now().await;
            assert_eq!(controller.active_count(), 0);
            assert!(!controller.is_at_limit());
        }
    }

    #[tokio::test]
    async fn test_try_acquire_sequential_after_each_drop() {
        let controller = FineGrainedConcurrencyController::new(ConcurrencyStrategy::Global {
            max_concurrent: 1,
            queue_size: 10,
        });

        for _ in 0..5 {
            let guard = controller.try_acquire();
            assert!(guard.is_some());
            assert_eq!(controller.active_count(), 1);
            drop(guard);
            tokio::task::yield_now().await;
            assert_eq!(controller.active_count(), 0);
        }
    }

    // ========== AdaptiveConcurrencyController additional tests ==========

    #[test]
    fn test_adaptive_concurrency_controller_new_with_min_max() {
        let controller = AdaptiveConcurrencyController::new(2, 50, 0.6, Duration::from_secs(30));
        assert_eq!(controller.base().active_count(), 0);
        assert!(!controller.base().is_at_limit());
    }

    #[test]
    fn test_adaptive_concurrency_controller_base_utilization_zero() {
        let controller = AdaptiveConcurrencyController::new(1, 100, 0.7, Duration::from_secs(60));
        assert_eq!(controller.base().utilization(), 0.0);
    }
}
