// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 并发控制器实现
//!
//! 所有 impl 块从 mod.rs 拆出，mod.rs 仅保留 trait/struct/enum 定义。

use log::{debug, warn};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{
    AdaptiveConcurrencyController, ConcurrencyError, ConcurrencyGuard, ConcurrencyPermit,
    ConcurrencyStats, ConcurrencyStrategy, FineGrainedConcurrencyController,
};

impl Default for ConcurrencyStrategy {
    fn default() -> Self {
        ConcurrencyStrategy::Global {
            max_concurrent: 100,
            queue_size: 1000,
        }
    }
}

impl ConcurrencyPermit {
    /// 创建新的并发许可
    fn new(
        _permit: OwnedSemaphorePermit,
        active_count: Arc<AtomicU32>,
        global_counter: Arc<AtomicU32>,
    ) -> Self {
        Self {
            _permit,
            active_count,
            global_counter,
        }
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        // 当 guard 被 drop 时，减少计数器
        // 注意：permit 会自动释放（通过其自身的 Drop 实现）
        let prev_active = self.active_count.fetch_sub(1, Ordering::SeqCst);
        let prev_global = self.global_counter.fetch_sub(1, Ordering::SeqCst);
        debug!(
            "Released concurrency permit. Active: {} -> {}, Global: {} -> {}",
            prev_active,
            prev_active.saturating_sub(1),
            prev_global,
            prev_global.saturating_sub(1)
        );
    }
}

impl ConcurrencyGuard {
    /// 创建新的并发守卫
    fn new(
        _permit: tokio::sync::SemaphorePermit<'static>,
        active_count: Arc<AtomicU32>,
        global_counter: Arc<AtomicU32>,
    ) -> Self {
        Self {
            _permit,
            active_count,
            global_counter,
        }
    }
}

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        let prev_active = self.active_count.fetch_sub(1, Ordering::SeqCst);
        let prev_global = self.global_counter.fetch_sub(1, Ordering::SeqCst);
        debug!(
            "Released concurrency guard. Active: {} -> {}, Global: {} -> {}",
            prev_active,
            prev_active.saturating_sub(1),
            prev_global,
            prev_global.saturating_sub(1)
        );
    }
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
    ///
    /// 返回一个 RAII guard，当 guard 被 drop 时自动释放槽位。
    /// 如果当前已达到并发限制，返回 None。
    pub fn try_acquire(&self) -> Option<ConcurrencyGuard> {
        match self.global_semaphore.try_acquire() {
            Ok(permit) => {
                self.global_counter.fetch_add(1, Ordering::SeqCst);
                self.active_count.fetch_add(1, Ordering::SeqCst);
                debug!(
                    "Acquired concurrency slot via try_acquire. Active: {}",
                    self.active_count.load(Ordering::SeqCst)
                );
                // SAFETY: 我们将 permit 的生命周期转换为 'static，因为它被包装在 ConcurrencyGuard 中，
                // guard 持有 Semaphore 的 Arc 引用，确保 permit 在有效期内有效。
                // 这是一个安全的模式，类似于 OwnedSemaphorePermit 的工作方式。
                let static_permit = unsafe {
                    std::mem::transmute::<
                        tokio::sync::SemaphorePermit<'_>,
                        tokio::sync::SemaphorePermit<'static>,
                    >(permit)
                };
                Some(ConcurrencyGuard::new(
                    static_permit,
                    self.active_count.clone(),
                    self.global_counter.clone(),
                ))
            }
            Err(_) => {
                debug!(
                    "try_acquire failed - at limit. Active: {}",
                    self.active_count.load(Ordering::SeqCst)
                );
                None
            }
        }
    }

    /// 获取并发槽位（等待）
    ///
    /// 返回一个 RAII guard，当 guard 被 drop 时自动释放槽位。
    /// 如果指定了超时时间且超时，返回 Timeout 错误。
    pub async fn acquire(
        &self,
        timeout: Option<Duration>,
    ) -> Result<ConcurrencyPermit, ConcurrencyError> {
        let permit = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, self.global_semaphore.clone().acquire_owned())
                .await
            {
                Ok(result) => result.map_err(|_| ConcurrencyError::Closed)?,
                Err(_) => return Err(ConcurrencyError::Timeout),
            }
        } else {
            self.global_semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| ConcurrencyError::Closed)?
        };

        self.global_counter.fetch_add(1, Ordering::SeqCst);
        self.active_count.fetch_add(1, Ordering::SeqCst);

        debug!(
            "Acquired concurrency slot via acquire. Active: {}",
            self.active_count.load(Ordering::SeqCst)
        );

        Ok(ConcurrencyPermit::new(
            permit,
            self.active_count.clone(),
            self.global_counter.clone(),
        ))
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
        let adjustment_interval = self.adjustment_interval;
        let target_utilization = self.target_utilization;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(adjustment_interval);

            loop {
                interval.tick().await;

                let utilization = controller.utilization();

                if utilization < target_utilization * 0.8 {
                    // 低于目标利用率，增加并发
                    warn!(
                        "Low utilization detected: {:.1}%. Consider increasing concurrency.",
                        utilization * 100.0
                    );
                } else if utilization > target_utilization * 1.2 {
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
