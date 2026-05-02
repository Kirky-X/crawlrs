// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Worker core logic tests
//!
//! Tests the Worker trait, WorkerProcess trait, ProcessResult, and AbstractWorker

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use crawlrs::workers::worker::{AbstractWorker, ProcessResult, Worker, WorkerProcess};

// === Test Processors ===

/// 成功的测试处理器 - 每次处理返回 Completed
struct SuccessfulProcessor {
    name: String,
    call_count: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl WorkerProcess for SuccessfulProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self) -> ProcessResult {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ProcessResult::Completed
    }
}

/// 失败的测试处理器 - 每次处理返回 Error
struct FailingProcessor {
    name: String,
    error_message: String,
}

#[async_trait::async_trait]
impl WorkerProcess for FailingProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self) -> ProcessResult {
        ProcessResult::Error(self.error_message.clone())
    }
}

/// 空闲处理器 - 每次处理返回 Empty
struct IdleProcessor {
    name: String,
}

#[async_trait::async_trait]
impl WorkerProcess for IdleProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self) -> ProcessResult {
        ProcessResult::Empty
    }
}

/// 条件处理器 - 根据状态返回不同结果
struct ConditionalProcessor {
    name: String,
    state: Arc<std::sync::Mutex<ProcessorState>>,
}

struct ProcessorState {
    call_count: usize,
    fail_after: usize,
}

#[async_trait::async_trait]
impl WorkerProcess for ConditionalProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self) -> ProcessResult {
        let mut state = self.state.lock().await;
        state.call_count += 1;

        if state.call_count >= state.fail_after {
            ProcessResult::Error("Failed after threshold".to_string())
        } else {
            ProcessResult::Completed
        }
    }
}

// === Unit Tests ===

#[test]
fn test_process_result_equality() {
    // 测试 ProcessResult 枚举的相等性
    assert_eq!(ProcessResult::Completed, ProcessResult::Completed);
    assert_eq!(
        ProcessResult::Error("test".to_string()),
        ProcessResult::Error("test".to_string())
    );
    assert_eq!(ProcessResult::Empty, ProcessResult::Empty);

    // 测试不等性
    assert_ne!(ProcessResult::Completed, ProcessResult::Empty);
    assert_ne!(
        ProcessResult::Error("a".to_string()),
        ProcessResult::Error("b".to_string())
    );
}

#[test]
fn test_abstract_worker_creation() {
    // 测试 AbstractWorker 的创建
    let processor = SuccessfulProcessor {
        name: "test_worker".to_string(),
        call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let worker = AbstractWorker::new(Arc::new(processor), Duration::from_millis(100));

    assert_eq!(worker.name(), "test_worker");
}

#[test]
fn test_abstract_worker_name() {
    // 测试 Worker trait 的 name 方法
    let processor = IdleProcessor {
        name: "idle_worker".to_string(),
    };

    let worker = AbstractWorker::new(Arc::new(processor), Duration::from_millis(100));

    assert_eq!(worker.name(), "idle_worker");
    assert!(!worker.name().is_empty());
}

#[test]
fn test_processor_name() {
    // 测试 WorkerProcess trait 的 name 方法
    let processor = SuccessfulProcessor {
        name: "success_processor".to_string(),
        call_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    assert_eq!(processor.name(), "success_processor");
}

#[test]
fn test_successful_processor_sync() {
    // 测试同步调用成功处理器
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let processor = SuccessfulProcessor {
        name: "sync_worker".to_string(),
        call_count: call_count.clone(),
    };

    // 使用 tokio::runtime 在同步测试中运行异步代码
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(processor.process());

    assert_eq!(result, ProcessResult::Completed);
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn test_failing_processor() {
    // 测试失败处理器
    let processor = FailingProcessor {
        name: "failing_worker".to_string(),
        error_message: "Test error".to_string(),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(processor.process());

    assert!(matches!(result, ProcessResult::Error(_)));
    if let ProcessResult::Error(msg) = result {
        assert_eq!(msg, "Test error");
    }
}

#[test]
fn test_idle_processor() {
    // 测试空闲处理器
    let processor = IdleProcessor {
        name: "idle_worker".to_string(),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(processor.process());

    assert_eq!(result, ProcessResult::Empty);
}

#[test]
fn test_conditional_processor() {
    // 测试条件处理器 - 在达到阈值前成功，之后失败
    let state = Arc::new(std::sync::Mutex::new(ProcessorState {
        call_count: 0,
        fail_after: 3,
    }));

    let processor = ConditionalProcessor {
        name: "conditional_worker".to_string(),
        state: state.clone(),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();

    // 前两次应该成功
    let result1 = rt.block_on(processor.process());
    assert_eq!(result1, ProcessResult::Completed);

    let result2 = rt.block_on(processor.process());
    assert_eq!(result2, ProcessResult::Completed);

    // 第三次应该失败
    let result3 = rt.block_on(processor.process());
    assert!(matches!(result3, ProcessResult::Error(_)));

    let state_guard = rt.block_on(state.lock());
    assert_eq!(state_guard.call_count, 3);
}

// === Integration Tests ===

#[tokio::test]
async fn test_abstract_worker_run_multiple_cycles() {
    // 测试 AbstractWorker 运行多个周期
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let processor = SuccessfulProcessor {
        name: "multi_cycle_worker".to_string(),
        call_count: call_count.clone(),
    };

    let worker = AbstractWorker::new(Arc::new(processor), Duration::from_millis(50));

    // 运行工作器一段时间
    let worker_handle = tokio::spawn(async move {
        let duration = Duration::from_millis(200);
        let start = std::time::Instant::now();

        // 手动运行几个周期
        for _ in 0..5 {
            worker.processor.process().await;
            sleep(Duration::from_millis(50)).await;
        }
    });

    timeout(Duration::from_secs(5), worker_handle)
        .await
        .expect("Worker test timed out")
        .expect("Worker task failed");

    // 验证处理器被调用了多次
    let count = call_count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(count >= 5, "Expected at least 5 calls, got {}", count);
}

#[tokio::test]
async fn test_abstract_worker_with_error_recovery() {
    // 测试工作器在错误后继续运行
    let state = Arc::new(std::sync::Mutex::new(ProcessorState {
        call_count: 0,
        fail_after: 2,
    }));

    let processor = ConditionalProcessor {
        name: "recovery_worker".to_string(),
        state: state.clone(),
    };

    let worker = AbstractWorker::new(Arc::new(processor), Duration::from_millis(50));

    // 运行几个周期
    for _ in 0..3 {
        let result = worker.processor.process().await;
        // 不管成功还是失败，都应该继续运行
        match result {
            ProcessResult::Completed => {}
            ProcessResult::Error(_) => {}
            ProcessResult::Empty => {}
        }
        sleep(Duration::from_millis(10)).await;
    }

    let state_guard = state.lock().await;
    assert_eq!(state_guard.call_count, 3);
}

#[tokio::test]
async fn test_multiple_workers_parallel() {
    // 测试多个工作器并行运行
    let call_count1 = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let call_count2 = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let call_count3 = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let processor1 = SuccessfulProcessor {
        name: "worker1".to_string(),
        call_count: call_count1.clone(),
    };

    let processor2 = SuccessfulProcessor {
        name: "worker2".to_string(),
        call_count: call_count2.clone(),
    };

    let processor3 = SuccessfulProcessor {
        name: "worker3".to_string(),
        call_count: call_count3.clone(),
    };

    // 并行运行三个工作器
    let handle1 = tokio::spawn(async move {
        for _ in 0..3 {
            processor1.process().await;
            sleep(Duration::from_millis(10)).await;
        }
    });

    let handle2 = tokio::spawn(async move {
        for _ in 0..3 {
            processor2.process().await;
            sleep(Duration::from_millis(10)).await;
        }
    });

    let handle3 = tokio::spawn(async move {
        for _ in 0..3 {
            processor3.process().await;
            sleep(Duration::from_millis(10)).await;
        }
    });

    // 等待所有工作器完成
    let results = tokio::join!(handle1, handle2, handle3);
    assert!(results.0.is_ok());
    assert!(results.1.is_ok());
    assert!(results.2.is_ok());

    // 验证每个工作器都被调用了
    assert_eq!(call_count1.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(call_count2.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(call_count3.load(std::sync::atomic::Ordering::SeqCst), 3);
}

// === Edge Cases and Error Handling ===

#[test]
fn test_process_result_clone() {
    // 测试 ProcessResult 的 Clone trait
    let result = ProcessResult::Error("test error".to_string());
    let cloned = result.clone();
    assert_eq!(result, cloned);
}

#[test]
fn test_process_result_debug() {
    // 测试 ProcessResult 的 Debug trait
    let result = ProcessResult::Completed;
    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("Completed"));

    let error_result = ProcessResult::Error("test".to_string());
    let error_debug = format!("{:?}", error_result);
    assert!(error_debug.contains("Error"));
}

#[tokio::test]
async fn test_worker_with_zero_interval() {
    // 测试零间隔工作器（边界条件）
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let processor = SuccessfulProcessor {
        name: "zero_interval_worker".to_string(),
        call_count: call_count.clone(),
    };

    let worker = AbstractWorker::new(Arc::new(processor), Duration::from_millis(0));

    // 快速运行几个周期
    for _ in 0..5 {
        worker.processor.process().await;
    }

    let count = call_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_worker_concurrent_access() {
    // 测试工作器的并发访问安全性
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let processor = SuccessfulProcessor {
        name: "concurrent_worker".to_string(),
        call_count: call_count.clone(),
    };

    let worker = Arc::new(AbstractWorker::new(processor, Duration::from_millis(10)));

    // 从多个任务并发访问
    let mut handles = vec![];

    for _ in 0..10 {
        let worker_clone = worker.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..5 {
                worker_clone.processor.process().await;
                sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }

    // 等待所有任务完成
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // 验证所有调用都成功执行
    let count = call_count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(count, 50); // 10 tasks * 5 calls each
}
