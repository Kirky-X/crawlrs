// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use log::{debug, error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

/// Worker trait定义
///
/// 所有后台工作器都必须实现此trait
#[async_trait]
pub trait Worker: Send + Sync {
    /// 运行工作器
    async fn run(&self);

    /// 获取工作器名称
    fn name(&self) -> &str;
}

/// 处理结果枚举
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    /// 处理成功完成
    Completed,
    /// 发生错误
    Error(String),
    /// 无任务需要处理（空闲状态）
    Empty,
}

/// Worker 处理 trait
///
/// 定义单个处理周期的工作逻辑，配合 AbstractWorker 使用
#[async_trait]
pub trait WorkerProcess: Send + Sync {
    /// 获取工作器名称
    fn name(&self) -> &str;

    /// 处理单个周期
    async fn process(&self) -> ProcessResult;
}

/// 模板工作器骨架
///
/// 封装通用的循环逻辑：定时周期 + 错误处理 + 日志记录
pub struct AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync,
{
    processor: Arc<P>,
    interval: Duration,
}

impl<P> AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync,
{
    /// 创建新的模板工作器
    pub fn new(processor: Arc<P>, interval: Duration) -> Self {
        Self {
            processor,
            interval,
        }
    }
}

#[async_trait]
impl<P> Worker for AbstractWorker<P>
where
    P: WorkerProcess + Send + Sync + 'static,
{
    /// 运行工作器（模板方法）
    async fn run(&self) {
        info!("Worker '{}' started", self.processor.name());
        let mut interval = interval(self.interval);

        loop {
            interval.tick().await;

            match self.processor.process().await {
                ProcessResult::Completed => {
                    debug!("Worker '{}' completed one cycle", self.processor.name());
                }
                ProcessResult::Error(e) => {
                    error!("Worker '{}' error: {}", self.processor.name(), e);
                }
                ProcessResult::Empty => {
                    debug!("Worker '{}' no work to do", self.processor.name());
                }
            }
        }
    }

    fn name(&self) -> &str {
        self.processor.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ========== ProcessResult tests ==========

    #[test]
    fn test_process_result_completed_equality() {
        assert_eq!(ProcessResult::Completed, ProcessResult::Completed);
    }

    #[test]
    fn test_process_result_empty_equality() {
        assert_eq!(ProcessResult::Empty, ProcessResult::Empty);
    }

    #[test]
    fn test_process_result_error_equality_same_message() {
        assert_eq!(
            ProcessResult::Error("msg".to_string()),
            ProcessResult::Error("msg".to_string())
        );
    }

    #[test]
    fn test_process_result_error_inequality_different_message() {
        assert_ne!(
            ProcessResult::Error("a".to_string()),
            ProcessResult::Error("b".to_string())
        );
    }

    #[test]
    fn test_process_result_completed_neq_empty() {
        assert_ne!(ProcessResult::Completed, ProcessResult::Empty);
    }

    #[test]
    fn test_process_result_clone_preserves_variant() {
        let original = ProcessResult::Error("clone-me".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_process_result_debug_contains_variant_name() {
        let r = ProcessResult::Completed;
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("Completed"));
    }

    // ========== MockProcessor for WorkerProcess tests ==========

    struct MockProcessor {
        name: &'static str,
        result: ProcessResult,
        call_count: AtomicU32,
    }

    impl MockProcessor {
        fn new(name: &'static str, result: ProcessResult) -> Self {
            Self {
                name,
                result,
                call_count: AtomicU32::new(0),
            }
        }

        fn calls(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WorkerProcess for MockProcessor {
        fn name(&self) -> &str {
            self.name
        }

        async fn process(&self) -> ProcessResult {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    // ========== WorkerProcess trait tests ==========

    #[tokio::test]
    async fn test_mock_processor_returns_completed() {
        let p = MockProcessor::new("mp", ProcessResult::Completed);
        let result = p.process().await;
        assert_eq!(result, ProcessResult::Completed);
        assert_eq!(p.calls(), 1);
    }

    #[tokio::test]
    async fn test_mock_processor_returns_error() {
        let p = MockProcessor::new("mp", ProcessResult::Error("fail".to_string()));
        let result = p.process().await;
        assert_eq!(result, ProcessResult::Error("fail".to_string()));
    }

    #[tokio::test]
    async fn test_mock_processor_returns_empty() {
        let p = MockProcessor::new("mp", ProcessResult::Empty);
        let result = p.process().await;
        assert_eq!(result, ProcessResult::Empty);
    }

    #[tokio::test]
    async fn test_mock_processor_increments_call_count() {
        let p = MockProcessor::new("mp", ProcessResult::Completed);
        assert_eq!(p.calls(), 0);
        let _ = p.process().await;
        let _ = p.process().await;
        let _ = p.process().await;
        assert_eq!(p.calls(), 3);
    }

    #[test]
    fn test_mock_processor_name() {
        let p = MockProcessor::new("named-processor", ProcessResult::Completed);
        assert_eq!(p.name(), "named-processor");
    }

    // ========== AbstractWorker tests ==========

    #[test]
    fn test_abstract_worker_name_returns_processor_name() {
        let processor = Arc::new(MockProcessor::new("ab-worker", ProcessResult::Completed));
        let worker = AbstractWorker::new(processor, Duration::from_millis(10));
        assert_eq!(worker.name(), "ab-worker");
    }

    #[tokio::test]
    async fn test_abstract_worker_run_processes_one_cycle_with_timeout() {
        let processor = Arc::new(MockProcessor::new("run-worker", ProcessResult::Completed));
        let worker = AbstractWorker::new(processor.clone(), Duration::from_millis(10));
        // Run the worker briefly; it loops forever, so we use a timeout.
        let _ = tokio::time::timeout(Duration::from_millis(100), worker.run()).await;
        // At least one process() call should have happened after the first tick.
        assert!(
            processor.calls() > 0,
            "worker should have processed at least one cycle"
        );
    }

    #[tokio::test]
    async fn test_abstract_worker_run_handles_error_result() {
        let processor = Arc::new(MockProcessor::new(
            "err-worker",
            ProcessResult::Error("boom".to_string()),
        ));
        let worker = AbstractWorker::new(processor.clone(), Duration::from_millis(10));
        let _ = tokio::time::timeout(Duration::from_millis(100), worker.run()).await;
        assert!(
            processor.calls() > 0,
            "worker should process even when result is Error"
        );
    }

    #[tokio::test]
    async fn test_abstract_worker_run_handles_empty_result() {
        let processor = Arc::new(MockProcessor::new("empty-worker", ProcessResult::Empty));
        let worker = AbstractWorker::new(processor.clone(), Duration::from_millis(10));
        let _ = tokio::time::timeout(Duration::from_millis(100), worker.run()).await;
        assert!(
            processor.calls() > 0,
            "worker should process even when result is Empty"
        );
    }
}
