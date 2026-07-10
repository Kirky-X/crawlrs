// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一的任务队列客户端
//!
//! 提供统一的任务队列操作接口，支持单任务和批量操作、优先级、指标收集等

use crate::domain::models::Task;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub use super::task_queue::{QueueError as InnerQueueError, TaskQueue};

/// 队列客户端配置构建器
///
/// 用于安全地构建 QueueClient 实例
#[derive(Debug, Clone)]
pub struct QueueClientBuilder {
    /// 最大批量操作大小
    max_batch_size: u32,
    /// 默认重试次数
    default_max_retries: i32,
    /// 默认优先级 (1-10)
    default_priority: i32,
    /// 操作超时时间
    operation_timeout_ms: u64,
    /// 启用指标收集
    metrics_enabled: bool,
}

impl Default for QueueClientBuilder {
    fn default() -> Self {
        Self {
            max_batch_size: 10,
            default_max_retries: 3,
            default_priority: 5,
            operation_timeout_ms: 30000,
            metrics_enabled: true,
        }
    }
}

impl QueueClientBuilder {
    /// 创建新的构建器实例
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置最大批量操作大小
    pub fn with_max_batch_size(mut self, size: u32) -> Self {
        self.max_batch_size = size.clamp(1, 100);
        self
    }

    /// 设置默认重试次数
    pub fn with_default_max_retries(mut self, retries: i32) -> Self {
        self.default_max_retries = retries.clamp(0, 10);
        self
    }

    /// 设置默认优先级 (1-10)
    pub fn with_default_priority(mut self, priority: i32) -> Self {
        self.default_priority = priority.clamp(1, 10);
        self
    }

    /// 设置操作超时时间（毫秒）
    pub fn with_operation_timeout(mut self, timeout_ms: u64) -> Self {
        self.operation_timeout_ms = timeout_ms.clamp(1000, 300000);
        self
    }

    /// 设置是否启用指标收集
    pub fn with_metrics_enabled(mut self, enabled: bool) -> Self {
        self.metrics_enabled = enabled;
        self
    }

    /// 构建 QueueClient 实例
    ///
    /// # 参数
    ///
    /// * `queue` - 底层任务队列实现
    ///
    /// # 返回值
    ///
    /// 返回配置好的 QueueClient 实例
    pub fn build<T: TaskQueue>(self, queue: T) -> QueueClient<T> {
        QueueClient::new(
            queue,
            QueueClientConfig {
                max_batch_size: self.max_batch_size,
                default_max_retries: self.default_max_retries,
                default_priority: self.default_priority,
                operation_timeout_ms: self.operation_timeout_ms,
                metrics_enabled: self.metrics_enabled,
            },
        )
    }
}

/// 队列客户端配置
///
/// 内部配置结构，包含运行时参数
#[derive(Debug, Clone)]
pub struct QueueClientConfig {
    max_batch_size: u32,
    default_max_retries: i32,
    default_priority: i32,
    operation_timeout_ms: u64,
    metrics_enabled: bool,
}

impl QueueClientConfig {
    /// 获取最大批量操作大小
    pub fn max_batch_size(&self) -> u32 {
        self.max_batch_size
    }

    /// 获取默认最大重试次数
    pub fn default_max_retries(&self) -> i32 {
        self.default_max_retries
    }

    /// 获取默认优先级
    pub fn default_priority(&self) -> i32 {
        self.default_priority
    }

    /// 获取操作超时时间（毫秒）
    pub fn operation_timeout_ms(&self) -> u64 {
        self.operation_timeout_ms
    }

    /// 检查是否启用指标收集
    pub fn is_metrics_enabled(&self) -> bool {
        self.metrics_enabled
    }
}

/// 队列客户端错误类型
///
/// 统一所有队列操作可能返回的错误
#[derive(Error, Debug)]
pub enum QueueClientError {
    /// 队列为空
    #[error("Queue is empty")]
    EmptyQueue,

    /// 任务不存在
    #[error("Task not found: {0}")]
    TaskNotFound(Uuid),

    /// 任务已被锁定
    #[error("Task is locked by another worker: {0}")]
    TaskLocked(Uuid),

    /// 无效的状态转换
    #[error("Invalid state transition for task: {0}")]
    InvalidStateTransition(Uuid),

    /// 操作超时
    #[error("Operation timed out after {0}ms")]
    Timeout(u64),

    /// 优雅关闭中
    #[error("Queue is shutting down")]
    ShuttingDown,

    /// 批量操作部分失败
    #[error("Batch operation partially failed: {0} succeeded, {1} failed")]
    PartialBatchFailure(usize, usize),

    /// 优先级无效
    #[error("Invalid priority value: {0}")]
    InvalidPriority(i32),

    /// 无效的任务类型
    #[error("Invalid task type: {0}")]
    InvalidTaskType(String),

    /// 配置错误
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// 内部错误
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<InnerQueueError> for QueueClientError {
    fn from(e: InnerQueueError) -> Self {
        match e {
            InnerQueueError::Empty => Self::EmptyQueue,
            InnerQueueError::Repository(_) => Self::Internal(e.to_string()),
        }
    }
}

/// 任务入队请求
///
/// 统一的入队操作输入结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnqueueRequest {
    /// 任务类型
    pub task_type: String,
    /// 目标URL
    pub url: String,
    /// 任务负载
    pub payload: serde_json::Value,
    /// 优先级 (1-10)
    priority: Option<i32>,
    /// 所属团队ID
    team_id: Uuid,
    /// API密钥ID
    api_key_id: Uuid,
    /// 延迟执行时间（秒）
    delay_seconds: Option<u64>,
    /// 过期时间（秒）
    expire_seconds: Option<u64>,
    /// 最大重试次数
    max_retries: Option<i32>,
}

impl EnqueueRequest {
    /// 创建新的入队请求
    pub fn new(
        task_type: &str,
        url: &str,
        payload: serde_json::Value,
        team_id: Uuid,
        api_key_id: Uuid,
    ) -> Self {
        Self {
            task_type: task_type.to_string(),
            url: url.to_string(),
            payload,
            priority: None,
            team_id,
            api_key_id,
            delay_seconds: None,
            expire_seconds: None,
            max_retries: None,
        }
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority.clamp(1, 10));
        self
    }

    /// 设置延迟执行时间
    pub fn with_delay(mut self, seconds: u64) -> Self {
        self.delay_seconds = Some(seconds);
        self
    }

    /// 设置过期时间
    pub fn with_expire(mut self, seconds: u64) -> Self {
        self.expire_seconds = Some(seconds);
        self
    }

    /// 设置最大重试次数
    pub fn with_max_retries(mut self, retries: i32) -> Self {
        self.max_retries = Some(retries.clamp(0, 10));
        self
    }
}

/// 任务出队请求
///
/// 统一的出队操作输入结构
#[derive(Debug, Clone)]
pub struct DequeueRequest {
    /// 工作节点ID
    worker_id: Uuid,
    /// 批量大小
    batch_size: Option<u32>,
    /// 阻塞等待超时（毫秒），0 表示非阻塞
    poll_timeout_ms: u64,
}

impl DequeueRequest {
    /// 创建新的出队请求
    pub fn new(worker_id: Uuid) -> Self {
        Self {
            worker_id,
            batch_size: None,
            poll_timeout_ms: 0,
        }
    }

    /// 设置批量大小
    pub fn with_batch_size(mut self, size: u32) -> Self {
        self.batch_size = Some(size.clamp(1, 100));
        self
    }

    /// 设置阻塞等待超时
    pub fn with_poll_timeout(mut self, timeout_ms: u64) -> Self {
        self.poll_timeout_ms = timeout_ms.clamp(0, 60000);
        self
    }
}

/// 任务状态更新请求
///
/// 统一的状态更新操作输入结构
#[derive(Debug, Clone)]
pub struct StatusUpdateRequest {
    /// 任务ID
    task_id: Uuid,
    /// 目标状态
    status: StatusUpdateType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusUpdateType {
    Completed,
    Failed,
    Cancelled,
}

impl StatusUpdateRequest {
    /// 创建完成状态更新请求
    pub fn complete(task_id: Uuid) -> Self {
        Self {
            task_id,
            status: StatusUpdateType::Completed,
        }
    }

    /// 创建失败状态更新请求
    pub fn fail(task_id: Uuid, _error: &str) -> Self {
        Self {
            task_id,
            status: StatusUpdateType::Failed,
        }
    }

    /// 创建取消状态更新请求
    pub fn cancel(task_id: Uuid) -> Self {
        Self {
            task_id,
            status: StatusUpdateType::Cancelled,
        }
    }
}

/// 队列操作类型
///
/// 用于指标收集和追踪
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOperation {
    Enqueue,
    Dequeue,
    Complete,
    Fail,
    Cancel,
    BatchEnqueue,
    BatchDequeue,
    BatchComplete,
    BatchFail,
}

/// 队列指标数据
///
/// 统一的指标输出结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMetricsData {
    /// 当前队列深度
    pub queue_depth: u64,
    /// 已处理任务总数
    pub tasks_processed: u64,
    /// 失败任务总数
    pub tasks_failed: u64,
    /// 平均处理时间（毫秒）
    pub avg_processing_time_ms: f64,
    /// 各操作的成功率
    pub operation_success_rates: std::collections::HashMap<String, f64>,
}

/// 队列客户端
///
/// 提供统一的任务队列操作接口，支持：
/// - 单任务和批量操作
/// - 优先级支持
/// - 指标收集
/// - 优雅关闭
///
/// # 示例
///
/// ```ignore
/// use crawlrs::queue::{QueueClient, QueueClientBuilder, EnqueueRequest};
///
/// let client = QueueClientBuilder::new()
///     .with_default_priority(5)
///     .build(postgres_queue);
///
/// let task = client.enqueue(EnqueueRequest::new(
///     "scrape",
///     "https://example.com",
///     serde_json::json!({"depth": 1}),
///     team_id,
/// )).await?;
/// ```
pub struct QueueClient<T: TaskQueue> {
    inner: T,
    config: QueueClientConfig,
    metrics: Option<Box<dyn QueueMetrics>>,
}

impl<T: TaskQueue> QueueClient<T> {
    /// 创建新的队列客户端
    ///
    /// # 参数
    ///
    /// * `inner` - 底层任务队列实现
    /// * `config` - 客户端配置
    ///
    /// # 返回值
    ///
    /// 返回新的队列客户端实例
    pub fn new(inner: T, config: QueueClientConfig) -> Self {
        let metrics = if config.metrics_enabled {
            Some(Box::new(QueueMetricsImpl::new()) as Box<dyn QueueMetrics>)
        } else {
            None
        };

        Self {
            inner,
            config,
            metrics,
        }
    }

    /// 获取客户端名称
    ///
    /// 返回客户端的类型名称，用于标识和日志
    pub fn name(&self) -> &'static str {
        std::any::type_name::<T>()
    }

    /// 获取当前配置
    pub fn config(&self) -> &QueueClientConfig {
        &self.config
    }

    /// 入队单个任务
    ///
    /// # 参数
    ///
    /// * `request` - 入队请求
    ///
    /// # 返回值
    ///
    /// * `Ok(Task)` - 成功入队的任务
    /// * `Err(Error)` - 入队失败
    pub async fn enqueue(&self, request: EnqueueRequest) -> Result<Task, QueueClientError> {
        let priority = request.priority.unwrap_or(self.config.default_priority);

        let task_type = request
            .task_type
            .parse()
            .map_err(|_| QueueClientError::InvalidTaskType(request.task_type.clone()))?;

        let mut task = Task::new(
            Uuid::new_v4(),
            task_type,
            request.team_id,
            request.api_key_id,
            request.url,
            request.payload,
        );

        task.priority = priority;
        task.max_retries = request
            .max_retries
            .unwrap_or(self.config.default_max_retries);

        if let Some(delay) = request.delay_seconds {
            task.scheduled_at = Some(chrono::Utc::now() + chrono::Duration::seconds(delay as i64));
        }

        if let Some(expire) = request.expire_seconds {
            task.expires_at = Some(chrono::Utc::now() + chrono::Duration::seconds(expire as i64));
        }

        let result = self.inner.enqueue(task).await?;
        self.record_operation(QueueOperation::Enqueue, true);
        Ok(result)
    }

    /// 批量入队任务
    ///
    /// # 参数
    ///
    /// * `requests` - 入队请求列表
    ///
    /// # 返回值
    ///
    /// * `Ok(BatchEnqueueResult)` - 批量入队结果
    /// * `Err(Error)` - 入队失败
    pub async fn enqueue_batch(
        &self,
        requests: &[EnqueueRequest],
    ) -> Result<BatchEnqueueResult, QueueClientError> {
        let max_batch = self.config.max_batch_size as usize;
        let requests = requests.iter().take(max_batch).collect::<Vec<_>>();

        let mut tasks = Vec::with_capacity(requests.len());
        for request in &requests {
            let priority = request.priority.unwrap_or(self.config.default_priority);

            let task_type = request
                .task_type
                .parse()
                .map_err(|_| QueueClientError::InvalidTaskType(request.task_type.clone()))?;

            let mut task = Task::new(
                Uuid::new_v4(),
                task_type,
                request.team_id,
                request.api_key_id,
                request.url.clone(),
                request.payload.clone(),
            );

            task.priority = priority;
            task.max_retries = request
                .max_retries
                .unwrap_or(self.config.default_max_retries);

            tasks.push(task);
        }

        let mut success_count = 0;
        let mut failed_count = 0;
        let mut tasks_result = Vec::new();
        let mut errors = Vec::new();

        for task in tasks {
            match self.inner.enqueue(task).await {
                Ok(task) => {
                    success_count += 1;
                    tasks_result.push(task);
                }
                Err(e) => {
                    failed_count += 1;
                    errors.push(e.to_string());
                }
            }
        }

        self.record_operation(QueueOperation::BatchEnqueue, failed_count == 0);

        if failed_count > 0 && success_count > 0 {
            return Err(QueueClientError::PartialBatchFailure(
                success_count,
                failed_count,
            ));
        } else if failed_count > 0 {
            return Err(QueueClientError::Internal(errors.join("; ")));
        }

        Ok(BatchEnqueueResult {
            tasks: tasks_result,
            success_count,
            failed_count,
        })
    }

    /// 出队任务
    ///
    /// # 参数
    ///
    /// * `request` - 出队请求
    ///
    /// # 返回值
    ///
    /// * `Ok(Option<Task>)` - 成功出队的任务或 None
    /// * `Err(Error)` - 出队失败
    pub async fn dequeue(&self, request: DequeueRequest) -> Result<Option<Task>, QueueClientError> {
        if let Some(batch_size) = request.batch_size {
            self.dequeue_batch(request.worker_id, batch_size)
                .await
                .map(|r| r.tasks.into_iter().next())
        } else {
            let task = self.inner.dequeue(request.worker_id).await?;
            self.record_operation(QueueOperation::Dequeue, true);
            Ok(task)
        }
    }

    /// 批量出队任务
    ///
    /// # 参数
    ///
    /// * `worker_id` - 工作节点ID
    /// * `size` - 批量大小
    ///
    /// # 返回值
    ///
    /// * `Ok(BatchDequeueResult)` - 批量出队结果
    /// * `Err(Error)` - 出队失败
    pub async fn dequeue_batch(
        &self,
        worker_id: Uuid,
        size: u32,
    ) -> Result<BatchDequeueResult, QueueClientError> {
        let size = size.clamp(1, self.config.max_batch_size);
        let mut tasks = Vec::new();

        for _ in 0..size {
            match self.inner.dequeue(worker_id).await {
                Ok(Some(task)) => tasks.push(task),
                Ok(None) => break,
                Err(e) => {
                    self.record_operation(QueueOperation::BatchDequeue, false);
                    return Err(e.into());
                }
            }
        }

        self.record_operation(QueueOperation::BatchDequeue, true);
        Ok(BatchDequeueResult {
            tasks,
            worker_id,
            dequeued_at: chrono::Utc::now(),
        })
    }

    /// 更新任务状态
    ///
    /// # 参数
    ///
    /// * `request` - 状态更新请求
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 更新成功
    /// * `Err(Error)` - 更新失败
    pub async fn update_status(
        &self,
        request: StatusUpdateRequest,
    ) -> Result<(), QueueClientError> {
        let result = match request.status {
            StatusUpdateType::Completed => self.inner.complete(request.task_id).await,
            StatusUpdateType::Failed => self.inner.fail(request.task_id).await,
            StatusUpdateType::Cancelled => self.inner.cancel(request.task_id).await,
        };

        match result {
            Ok(_) => {
                self.record_operation(
                    match request.status {
                        StatusUpdateType::Completed => QueueOperation::Complete,
                        StatusUpdateType::Failed => QueueOperation::Fail,
                        StatusUpdateType::Cancelled => QueueOperation::Cancel,
                    },
                    true,
                );
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// 获取队列指标
    ///
    /// # 返回值
    ///
    /// * `Some(QueueMetricsData)` - 指标数据
    /// * `None` - 指标收集未启用
    pub fn get_metrics(&self) -> Option<QueueMetricsData> {
        self.metrics.as_ref().map(|m| m.collect())
    }

    /// 记录操作指标
    fn record_operation(&self, operation: QueueOperation, success: bool) {
        if let Some(metrics) = &self.metrics {
            metrics.record(operation, success);
        }
    }
}

/// 批量入队结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEnqueueResult {
    /// 成功入队的任务列表
    pub tasks: Vec<Task>,
    /// 成功数量
    pub success_count: usize,
    /// 失败数量
    pub failed_count: usize,
}

/// 批量出队结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchDequeueResult {
    /// 出队的任务列表
    pub tasks: Vec<Task>,
    /// 工作节点ID
    pub worker_id: Uuid,
    /// 出队时间
    pub dequeued_at: chrono::DateTime<chrono::Utc>,
}

/// 队列指标 trait
///
/// 定义指标收集接口
pub trait QueueMetrics: Send + Sync + 'static {
    /// 记录操作
    fn record(&self, operation: QueueOperation, success: bool);

    /// 收集指标数据
    fn collect(&self) -> QueueMetricsData;
}

/// 内部指标实现
#[derive(Debug)]
struct QueueMetricsImpl {
    tasks_processed: std::sync::atomic::AtomicU64,
    tasks_failed: std::sync::atomic::AtomicU64,
    operation_counts: std::sync::atomic::AtomicU64,
}

impl QueueMetricsImpl {
    fn new() -> Self {
        Self {
            tasks_processed: std::sync::atomic::AtomicU64::new(0),
            tasks_failed: std::sync::atomic::AtomicU64::new(0),
            operation_counts: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl QueueMetrics for QueueMetricsImpl {
    fn record(&self, operation: QueueOperation, _success: bool) {
        self.operation_counts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        match operation {
            QueueOperation::Complete | QueueOperation::BatchComplete => {
                self.tasks_processed
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            QueueOperation::Fail | QueueOperation::BatchFail => {
                self.tasks_failed
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            _ => {}
        }
    }

    fn collect(&self) -> QueueMetricsData {
        QueueMetricsData {
            queue_depth: 0, // Requires queue integration to track
            tasks_processed: self
                .tasks_processed
                .load(std::sync::atomic::Ordering::SeqCst),
            tasks_failed: self.tasks_failed.load(std::sync::atomic::Ordering::SeqCst),
            avg_processing_time_ms: 0.0, // Requires timing tracking
            operation_success_rates: std::collections::HashMap::with_capacity(16),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_client_builder_default() {
        let builder = QueueClientBuilder::new();
        assert_eq!(builder.max_batch_size, 10);
        assert_eq!(builder.default_max_retries, 3);
        assert_eq!(builder.default_priority, 5);
    }

    #[tokio::test]
    async fn test_queue_client_builder_chain() {
        let builder = QueueClientBuilder::new()
            .with_max_batch_size(20)
            .with_default_max_retries(5)
            .with_default_priority(8)
            .with_operation_timeout(60000)
            .with_metrics_enabled(false);

        assert_eq!(builder.max_batch_size, 20);
        assert_eq!(builder.default_max_retries, 5);
        assert_eq!(builder.default_priority, 8);
        assert_eq!(builder.operation_timeout_ms, 60000);
        assert!(!builder.metrics_enabled);
    }

    #[tokio::test]
    async fn test_enqueue_request_builder() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let request = EnqueueRequest::new(
            "scrape",
            "https://example.com",
            serde_json::json!({}),
            team_id,
            api_key_id,
        )
        .with_priority(7)
        .with_delay(60)
        .with_expire(3600)
        .with_max_retries(5);

        assert_eq!(request.task_type, "scrape");
        assert_eq!(request.url, "https://example.com");
        assert_eq!(request.priority, Some(7));
        assert_eq!(request.delay_seconds, Some(60));
        assert_eq!(request.expire_seconds, Some(3600));
        assert_eq!(request.max_retries, Some(5));
    }

    #[tokio::test]
    async fn test_enqueue_request_priority_clamping() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let request = EnqueueRequest::new(
            "scrape",
            "https://example.com",
            serde_json::json!({}),
            team_id,
            api_key_id,
        )
        .with_priority(15); // Should clamp to 10

        assert_eq!(request.priority, Some(10));
    }

    #[tokio::test]
    async fn test_dequeue_request_builder() {
        let worker_id = Uuid::new_v4();
        let request = DequeueRequest::new(worker_id)
            .with_batch_size(5)
            .with_poll_timeout(5000);

        assert_eq!(request.worker_id, worker_id);
        assert_eq!(request.batch_size, Some(5));
        assert_eq!(request.poll_timeout_ms, 5000);
    }

    #[tokio::test]
    async fn test_status_update_request() {
        let task_id = Uuid::new_v4();

        let complete = StatusUpdateRequest::complete(task_id);
        assert_eq!(complete.task_id, task_id);
        assert_eq!(complete.status, StatusUpdateType::Completed);

        let fail = StatusUpdateRequest::fail(task_id, "error");
        assert_eq!(fail.task_id, task_id);
        assert_eq!(fail.status, StatusUpdateType::Failed);

        let cancel = StatusUpdateRequest::cancel(task_id);
        assert_eq!(cancel.task_id, task_id);
        assert_eq!(cancel.status, StatusUpdateType::Cancelled);
    }

    #[tokio::test]
    async fn test_queue_operation_variants() {
        let variants = [
            QueueOperation::Enqueue,
            QueueOperation::Dequeue,
            QueueOperation::Complete,
            QueueOperation::Fail,
            QueueOperation::Cancel,
            QueueOperation::BatchEnqueue,
            QueueOperation::BatchDequeue,
            QueueOperation::BatchComplete,
            QueueOperation::BatchFail,
        ];

        assert_eq!(variants.len(), 9);
    }
}

// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests_ext {
    use super::*;
    use crate::domain::models::TaskStatus;
    use crate::domain::models::TaskType;
    use crate::queue::task_queue::QueueError;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    // ========== Mock TaskQueue implementation ==========

    struct MockTaskQueue {
        tasks: Arc<StdMutex<VecDeque<Task>>>,
        should_fail_enqueue: bool,
        should_fail_dequeue: bool,
        completed: Arc<StdMutex<Vec<Uuid>>>,
        failed: Arc<StdMutex<Vec<Uuid>>>,
        cancelled: Arc<StdMutex<Vec<Uuid>>>,
    }

    impl MockTaskQueue {
        fn new() -> Self {
            Self {
                tasks: Arc::new(StdMutex::new(VecDeque::new())),
                should_fail_enqueue: false,
                should_fail_dequeue: false,
                completed: Arc::new(StdMutex::new(Vec::new())),
                failed: Arc::new(StdMutex::new(Vec::new())),
                cancelled: Arc::new(StdMutex::new(Vec::new())),
            }
        }

        fn with_enqueue_failure() -> Self {
            let mut q = Self::new();
            q.should_fail_enqueue = true;
            q
        }

        #[allow(dead_code)]
        fn with_dequeue_failure() -> Self {
            let mut q = Self::new();
            q.should_fail_dequeue = true;
            q
        }

        #[allow(dead_code)]
        fn len(&self) -> usize {
            self.tasks.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            if self.should_fail_enqueue {
                return Err(QueueError::Repository(
                    crate::domain::repositories::task_repository::RepositoryError::Database(
                        anyhow::anyhow!("mock enqueue failure"),
                    ),
                ));
            }
            self.tasks.lock().unwrap().push_back(task.clone());
            Ok(task)
        }

        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            if self.should_fail_dequeue {
                return Err(QueueError::Repository(
                    crate::domain::repositories::task_repository::RepositoryError::Database(
                        anyhow::anyhow!("mock dequeue failure"),
                    ),
                ));
            }
            let task = self.tasks.lock().unwrap().pop_front();
            Ok(task)
        }

        async fn complete(&self, task_id: Uuid) -> Result<(), QueueError> {
            self.completed.lock().unwrap().push(task_id);
            Ok(())
        }

        async fn fail(&self, task_id: Uuid) -> Result<(), QueueError> {
            self.failed.lock().unwrap().push(task_id);
            Ok(())
        }

        async fn cancel(&self, task_id: Uuid) -> Result<(), QueueError> {
            self.cancelled.lock().unwrap().push(task_id);
            Ok(())
        }
    }

    fn make_enqueue_request(task_type: &str) -> EnqueueRequest {
        EnqueueRequest::new(
            task_type,
            "https://example.com",
            serde_json::json!({"key": "value"}),
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
    }

    fn build_client(queue: MockTaskQueue) -> QueueClient<MockTaskQueue> {
        QueueClientBuilder::new()
            .with_max_batch_size(10)
            .with_default_max_retries(3)
            .with_default_priority(5)
            .build(queue)
    }

    // ========== QueueClientBuilder clamping tests ==========

    #[test]
    fn test_builder_max_batch_size_clamps_below_min() {
        let builder = QueueClientBuilder::new().with_max_batch_size(0);
        assert_eq!(builder.max_batch_size, 1, "should clamp to 1");
    }

    #[test]
    fn test_builder_max_batch_size_clamps_above_max() {
        let builder = QueueClientBuilder::new().with_max_batch_size(200);
        assert_eq!(builder.max_batch_size, 100, "should clamp to 100");
    }

    #[test]
    fn test_builder_max_batch_size_accepts_valid() {
        let builder = QueueClientBuilder::new().with_max_batch_size(50);
        assert_eq!(builder.max_batch_size, 50);
    }

    #[test]
    fn test_builder_max_batch_size_accepts_boundary_min() {
        let builder = QueueClientBuilder::new().with_max_batch_size(1);
        assert_eq!(builder.max_batch_size, 1);
    }

    #[test]
    fn test_builder_max_batch_size_accepts_boundary_max() {
        let builder = QueueClientBuilder::new().with_max_batch_size(100);
        assert_eq!(builder.max_batch_size, 100);
    }

    #[test]
    fn test_builder_default_max_retries_clamps_below_min() {
        let builder = QueueClientBuilder::new().with_default_max_retries(-5);
        assert_eq!(builder.default_max_retries, 0, "should clamp to 0");
    }

    #[test]
    fn test_builder_default_max_retries_clamps_above_max() {
        let builder = QueueClientBuilder::new().with_default_max_retries(20);
        assert_eq!(builder.default_max_retries, 10, "should clamp to 10");
    }

    #[test]
    fn test_builder_default_max_retries_accepts_boundary_zero() {
        let builder = QueueClientBuilder::new().with_default_max_retries(0);
        assert_eq!(builder.default_max_retries, 0);
    }

    #[test]
    fn test_builder_default_max_retries_accepts_boundary_ten() {
        let builder = QueueClientBuilder::new().with_default_max_retries(10);
        assert_eq!(builder.default_max_retries, 10);
    }

    #[test]
    fn test_builder_default_priority_clamps_below_min() {
        let builder = QueueClientBuilder::new().with_default_priority(-1);
        assert_eq!(builder.default_priority, 1, "should clamp to 1");
    }

    #[test]
    fn test_builder_default_priority_clamps_above_max() {
        let builder = QueueClientBuilder::new().with_default_priority(15);
        assert_eq!(builder.default_priority, 10, "should clamp to 10");
    }

    #[test]
    fn test_builder_default_priority_accepts_boundary_one() {
        let builder = QueueClientBuilder::new().with_default_priority(1);
        assert_eq!(builder.default_priority, 1);
    }

    #[test]
    fn test_builder_default_priority_accepts_boundary_ten() {
        let builder = QueueClientBuilder::new().with_default_priority(10);
        assert_eq!(builder.default_priority, 10);
    }

    #[test]
    fn test_builder_operation_timeout_clamps_below_min() {
        let builder = QueueClientBuilder::new().with_operation_timeout(500);
        assert_eq!(builder.operation_timeout_ms, 1000, "should clamp to 1000");
    }

    #[test]
    fn test_builder_operation_timeout_clamps_above_max() {
        let builder = QueueClientBuilder::new().with_operation_timeout(500000);
        assert_eq!(
            builder.operation_timeout_ms, 300000,
            "should clamp to 300000"
        );
    }

    #[test]
    fn test_builder_operation_timeout_accepts_boundary_min() {
        let builder = QueueClientBuilder::new().with_operation_timeout(1000);
        assert_eq!(builder.operation_timeout_ms, 1000);
    }

    #[test]
    fn test_builder_operation_timeout_accepts_boundary_max() {
        let builder = QueueClientBuilder::new().with_operation_timeout(300000);
        assert_eq!(builder.operation_timeout_ms, 300000);
    }

    // ========== EnqueueRequest clamping tests ==========

    #[test]
    fn test_enqueue_request_max_retries_clamps_below_min() {
        let request = make_enqueue_request("scrape").with_max_retries(-3);
        assert_eq!(request.max_retries, Some(0), "should clamp to 0");
    }

    #[test]
    fn test_enqueue_request_max_retries_clamps_above_max() {
        let request = make_enqueue_request("scrape").with_max_retries(15);
        assert_eq!(request.max_retries, Some(10), "should clamp to 10");
    }

    #[test]
    fn test_enqueue_request_priority_clamps_below_min() {
        let request = make_enqueue_request("scrape").with_priority(0);
        assert_eq!(request.priority, Some(1), "should clamp to 1");
    }

    #[test]
    fn test_enqueue_request_priority_clamps_above_max() {
        let request = make_enqueue_request("scrape").with_priority(20);
        assert_eq!(request.priority, Some(10), "should clamp to 10");
    }

    // ========== DequeueRequest clamping tests ==========

    #[test]
    fn test_dequeue_request_batch_size_clamps_below_min() {
        let worker_id = Uuid::new_v4();
        let request = DequeueRequest::new(worker_id).with_batch_size(0);
        assert_eq!(request.batch_size, Some(1), "should clamp to 1");
    }

    #[test]
    fn test_dequeue_request_batch_size_clamps_above_max() {
        let worker_id = Uuid::new_v4();
        let request = DequeueRequest::new(worker_id).with_batch_size(200);
        assert_eq!(request.batch_size, Some(100), "should clamp to 100");
    }

    #[test]
    fn test_dequeue_request_poll_timeout_clamps_above_max() {
        let worker_id = Uuid::new_v4();
        let request = DequeueRequest::new(worker_id).with_poll_timeout(70000);
        assert_eq!(request.poll_timeout_ms, 60000, "should clamp to 60000");
    }

    #[test]
    fn test_dequeue_request_poll_timeout_accepts_zero() {
        let worker_id = Uuid::new_v4();
        let request = DequeueRequest::new(worker_id).with_poll_timeout(0);
        assert_eq!(request.poll_timeout_ms, 0);
    }

    // ========== QueueClientConfig accessor tests ==========

    #[test]
    fn test_config_accessors_return_builder_values() {
        let builder = QueueClientBuilder::new()
            .with_max_batch_size(25)
            .with_default_max_retries(7)
            .with_default_priority(3)
            .with_operation_timeout(45000)
            .with_metrics_enabled(true);
        let queue = MockTaskQueue::new();
        let client = builder.build(queue);
        let config = client.config();

        assert_eq!(config.max_batch_size(), 25);
        assert_eq!(config.default_max_retries(), 7);
        assert_eq!(config.default_priority(), 3);
        assert_eq!(config.operation_timeout_ms(), 45000);
        assert!(config.is_metrics_enabled());
    }

    #[test]
    fn test_config_accessors_with_metrics_disabled() {
        let builder = QueueClientBuilder::new().with_metrics_enabled(false);
        let queue = MockTaskQueue::new();
        let client = builder.build(queue);
        assert!(!client.config().is_metrics_enabled());
    }

    // ========== QueueClientError tests ==========

    #[test]
    fn test_queue_client_error_empty_queue_display() {
        let err = QueueClientError::EmptyQueue;
        assert!(format!("{}", err).contains("Queue is empty"));
    }

    #[test]
    fn test_queue_client_error_task_not_found_display() {
        let id = Uuid::new_v4();
        let err = QueueClientError::TaskNotFound(id);
        let msg = format!("{}", err);
        assert!(msg.contains("Task not found"));
        assert!(msg.contains(&id.to_string()));
    }

    #[test]
    fn test_queue_client_error_task_locked_display() {
        let id = Uuid::new_v4();
        let err = QueueClientError::TaskLocked(id);
        assert!(format!("{}", err).contains("locked"));
    }

    #[test]
    fn test_queue_client_error_invalid_state_transition_display() {
        let id = Uuid::new_v4();
        let err = QueueClientError::InvalidStateTransition(id);
        assert!(format!("{}", err).contains("Invalid state transition"));
    }

    #[test]
    fn test_queue_client_error_timeout_display() {
        let err = QueueClientError::Timeout(5000);
        let msg = format!("{}", err);
        assert!(msg.contains("timed out"));
        assert!(msg.contains("5000"));
    }

    #[test]
    fn test_queue_client_error_shutting_down_display() {
        let err = QueueClientError::ShuttingDown;
        assert!(format!("{}", err).contains("shutting down"));
    }

    #[test]
    fn test_queue_client_error_partial_batch_failure_display() {
        let err = QueueClientError::PartialBatchFailure(3, 2);
        let msg = format!("{}", err);
        assert!(msg.contains("3"));
        assert!(msg.contains("2"));
        assert!(msg.contains("partially failed"));
    }

    #[test]
    fn test_queue_client_error_invalid_priority_display() {
        let err = QueueClientError::InvalidPriority(15);
        assert!(format!("{}", err).contains("Invalid priority"));
    }

    #[test]
    fn test_queue_client_error_invalid_task_type_display() {
        let err = QueueClientError::InvalidTaskType("unknown".to_string());
        assert!(format!("{}", err).contains("Invalid task type"));
    }

    #[test]
    fn test_queue_client_error_config_error_display() {
        let err = QueueClientError::ConfigError("bad config".to_string());
        assert!(format!("{}", err).contains("Configuration error"));
    }

    #[test]
    fn test_queue_client_error_internal_display() {
        let err = QueueClientError::Internal("db error".to_string());
        assert!(format!("{}", err).contains("Internal error"));
    }

    #[test]
    fn test_queue_client_error_from_inner_empty() {
        let inner = InnerQueueError::Empty;
        let client_err: QueueClientError = inner.into();
        assert!(matches!(client_err, QueueClientError::EmptyQueue));
    }

    // ========== QueueClient enqueue tests ==========

    #[tokio::test]
    async fn test_client_enqueue_success() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape");
        let result = client.enqueue(request).await;
        assert!(result.is_ok());
        let task = result.unwrap();
        assert_eq!(task.task_type, TaskType::Scrape);
        assert_eq!(task.priority, 5); // default priority
        assert_eq!(task.max_retries, 3); // default max_retries
        assert_eq!(task.status, TaskStatus::Queued);
    }

    #[tokio::test]
    async fn test_client_enqueue_with_custom_priority() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("crawl").with_priority(8);
        let task = client.enqueue(request).await.unwrap();
        assert_eq!(task.priority, 8);
        assert_eq!(task.task_type, TaskType::Crawl);
    }

    #[tokio::test]
    async fn test_client_enqueue_with_max_retries() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("extract").with_max_retries(7);
        let task = client.enqueue(request).await.unwrap();
        assert_eq!(task.max_retries, 7);
        assert_eq!(task.task_type, TaskType::Extract);
    }

    #[tokio::test]
    async fn test_client_enqueue_with_delay_sets_scheduled_at() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape").with_delay(60);
        let task = client.enqueue(request).await.unwrap();
        assert!(
            task.scheduled_at.is_some(),
            "scheduled_at should be set with delay"
        );
    }

    #[tokio::test]
    async fn test_client_enqueue_without_delay_has_no_scheduled_at() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape");
        let task = client.enqueue(request).await.unwrap();
        assert!(task.scheduled_at.is_none());
    }

    #[tokio::test]
    async fn test_client_enqueue_with_expire_sets_expires_at() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape").with_expire(3600);
        let task = client.enqueue(request).await.unwrap();
        assert!(
            task.expires_at.is_some(),
            "expires_at should be set with expire"
        );
    }

    #[tokio::test]
    async fn test_client_enqueue_without_expire_has_no_expires_at() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape");
        let task = client.enqueue(request).await.unwrap();
        assert!(task.expires_at.is_none());
    }

    #[tokio::test]
    async fn test_client_enqueue_invalid_task_type() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = make_enqueue_request("invalid_type");
        let result = client.enqueue(request).await;
        assert!(result.is_err());
        match result {
            Err(QueueClientError::InvalidTaskType(msg)) => {
                assert!(msg.contains("invalid_type"));
            }
            _ => panic!("Expected InvalidTaskType error"),
        }
    }

    #[tokio::test]
    async fn test_client_enqueue_failure_propagates_error() {
        let queue = MockTaskQueue::with_enqueue_failure();
        let client = build_client(queue);

        let request = make_enqueue_request("scrape");
        let result = client.enqueue(request).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(QueueClientError::Internal(_))));
    }

    // ========== QueueClient enqueue_batch tests ==========

    #[tokio::test]
    async fn test_client_enqueue_batch_success() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let requests = vec![
            make_enqueue_request("scrape"),
            make_enqueue_request("crawl"),
            make_enqueue_request("extract"),
        ];

        let result = client.enqueue_batch(&requests).await.unwrap();
        assert_eq!(result.success_count, 3);
        assert_eq!(result.failed_count, 0);
        assert_eq!(result.tasks.len(), 3);
    }

    #[tokio::test]
    async fn test_client_enqueue_batch_empty() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let requests: Vec<EnqueueRequest> = vec![];
        let result = client.enqueue_batch(&requests).await.unwrap();
        assert_eq!(result.success_count, 0);
        assert_eq!(result.failed_count, 0);
        assert!(result.tasks.is_empty());
    }

    #[tokio::test]
    async fn test_client_enqueue_batch_truncates_to_max_batch_size() {
        let queue = MockTaskQueue::new();
        let client = QueueClientBuilder::new()
            .with_max_batch_size(3)
            .build(queue);

        let requests: Vec<EnqueueRequest> =
            (0..10).map(|_| make_enqueue_request("scrape")).collect();

        let result = client.enqueue_batch(&requests).await.unwrap();
        assert_eq!(
            result.success_count, 3,
            "should only enqueue max_batch_size items"
        );
    }

    #[tokio::test]
    async fn test_client_enqueue_batch_all_fail() {
        let queue = MockTaskQueue::with_enqueue_failure();
        let client = build_client(queue);

        let requests = vec![
            make_enqueue_request("scrape"),
            make_enqueue_request("crawl"),
        ];

        let result = client.enqueue_batch(&requests).await;
        assert!(result.is_err());
        match result {
            Err(QueueClientError::Internal(_)) => {}
            _ => panic!("Expected Internal error for all failures"),
        }
    }

    // ========== QueueClient dequeue tests ==========

    #[tokio::test]
    async fn test_client_dequeue_single() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Enqueue a task first
        client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();

        // Dequeue it
        let request = DequeueRequest::new(Uuid::new_v4());
        let result = client.dequeue(request).await.unwrap();
        assert!(result.is_some());
        let task = result.unwrap();
        assert_eq!(task.task_type, TaskType::Scrape);
    }

    #[tokio::test]
    async fn test_client_dequeue_empty_returns_none() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let request = DequeueRequest::new(Uuid::new_v4());
        let result = client.dequeue(request).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_client_dequeue_with_batch_size_returns_first() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Enqueue multiple tasks
        client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();
        client.enqueue(make_enqueue_request("crawl")).await.unwrap();

        // Dequeue with batch_size - should return first task
        let request = DequeueRequest::new(Uuid::new_v4()).with_batch_size(5);
        let result = client.dequeue(request).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_client_dequeue_batch_multiple() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Enqueue 3 tasks
        client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();
        client.enqueue(make_enqueue_request("crawl")).await.unwrap();
        client
            .enqueue(make_enqueue_request("extract"))
            .await
            .unwrap();

        // Dequeue batch of 2
        let result = client.dequeue_batch(Uuid::new_v4(), 2).await.unwrap();
        assert_eq!(result.tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_client_dequeue_batch_clamps_to_max() {
        let queue = MockTaskQueue::new();
        let client = QueueClientBuilder::new()
            .with_max_batch_size(3)
            .build(queue);

        // Enqueue 5 tasks
        for _ in 0..5 {
            client
                .enqueue(make_enqueue_request("scrape"))
                .await
                .unwrap();
        }

        // Request batch of 10, should be clamped to max_batch_size=3
        let result = client.dequeue_batch(Uuid::new_v4(), 10).await.unwrap();
        assert_eq!(result.tasks.len(), 3, "should be clamped to max_batch_size");
    }

    #[tokio::test]
    async fn test_client_dequeue_batch_empty_queue() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let result = client.dequeue_batch(Uuid::new_v4(), 5).await.unwrap();
        assert!(result.tasks.is_empty());
    }

    #[tokio::test]
    async fn test_client_dequeue_batch_fewer_than_requested() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Enqueue 2 tasks
        client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();
        client.enqueue(make_enqueue_request("crawl")).await.unwrap();

        // Request batch of 5, should only get 2
        let result = client.dequeue_batch(Uuid::new_v4(), 5).await.unwrap();
        assert_eq!(result.tasks.len(), 2);
    }

    // ========== QueueClient update_status tests ==========

    #[tokio::test]
    async fn test_client_update_status_complete() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let task_id = Uuid::new_v4();
        let request = StatusUpdateRequest::complete(task_id);
        let result = client.update_status(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_client_update_status_fail() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let task_id = Uuid::new_v4();
        let request = StatusUpdateRequest::fail(task_id, "test error");
        let result = client.update_status(request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_client_update_status_cancel() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let task_id = Uuid::new_v4();
        let request = StatusUpdateRequest::cancel(task_id);
        let result = client.update_status(request).await;
        assert!(result.is_ok());
    }

    // ========== QueueClient name and config tests ==========

    #[test]
    fn test_client_name_returns_type_name() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);
        let name = client.name();
        assert!(
            name.contains("MockTaskQueue"),
            "name should contain type name: {}",
            name
        );
    }

    #[test]
    fn test_client_config_returns_reference() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);
        let config = client.config();
        assert_eq!(config.max_batch_size(), 10);
    }

    // ========== QueueClient metrics tests ==========

    #[tokio::test]
    async fn test_client_metrics_enabled_returns_data() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Initially, metrics should show zeros
        let metrics = client.get_metrics().unwrap();
        assert_eq!(metrics.tasks_processed, 0);
        assert_eq!(metrics.tasks_failed, 0);

        // Enqueue and complete a task
        let task = client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();
        client
            .update_status(StatusUpdateRequest::complete(task.id))
            .await
            .unwrap();

        let metrics = client.get_metrics().unwrap();
        assert_eq!(metrics.tasks_processed, 1, "should have 1 processed task");
    }

    #[tokio::test]
    async fn test_client_metrics_disabled_returns_none() {
        let queue = MockTaskQueue::new();
        let client = QueueClientBuilder::new()
            .with_metrics_enabled(false)
            .build(queue);

        assert!(client.get_metrics().is_none());
    }

    #[tokio::test]
    async fn test_client_metrics_tracks_failures() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        let task = client
            .enqueue(make_enqueue_request("scrape"))
            .await
            .unwrap();
        client
            .update_status(StatusUpdateRequest::fail(task.id, "error"))
            .await
            .unwrap();

        let metrics = client.get_metrics().unwrap();
        assert_eq!(metrics.tasks_failed, 1, "should have 1 failed task");
    }

    #[tokio::test]
    async fn test_client_metrics_tracks_batch_operations() {
        let queue = MockTaskQueue::new();
        let client = build_client(queue);

        // Batch enqueue
        let requests = vec![
            make_enqueue_request("scrape"),
            make_enqueue_request("crawl"),
        ];
        client.enqueue_batch(&requests).await.unwrap();

        // Batch dequeue
        client.dequeue_batch(Uuid::new_v4(), 2).await.unwrap();

        // Metrics should have recorded operations
        let metrics = client.get_metrics().unwrap();
        assert!(metrics.tasks_processed == 0, "no completions yet");
    }

    // ========== QueueMetricsData serialization tests ==========

    #[test]
    fn test_queue_metrics_data_serialization() {
        let data = QueueMetricsData {
            queue_depth: 100,
            tasks_processed: 50,
            tasks_failed: 5,
            avg_processing_time_ms: 123.45,
            operation_success_rates: std::collections::HashMap::new(),
        };

        let json = serde_json::to_string(&data).expect("serialize");
        let back: QueueMetricsData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.queue_depth, 100);
        assert_eq!(back.tasks_processed, 50);
        assert_eq!(back.tasks_failed, 5);
        assert!((back.avg_processing_time_ms - 123.45).abs() < f64::EPSILON);
    }

    // ========== BatchEnqueueResult / BatchDequeueResult tests ==========

    #[test]
    fn test_batch_enqueue_result_serialization() {
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );

        let result = BatchEnqueueResult {
            tasks: vec![task],
            success_count: 1,
            failed_count: 0,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let back: BatchEnqueueResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.success_count, 1);
        assert_eq!(back.failed_count, 0);
        assert_eq!(back.tasks.len(), 1);
    }

    // ========== StatusUpdateType tests ==========

    #[test]
    fn test_status_update_type_equality() {
        let id = Uuid::new_v4();
        assert_eq!(
            StatusUpdateRequest::complete(id).status,
            StatusUpdateType::Completed
        );
        assert_eq!(
            StatusUpdateRequest::fail(id, "e").status,
            StatusUpdateType::Failed
        );
        assert_eq!(
            StatusUpdateRequest::cancel(id).status,
            StatusUpdateType::Cancelled
        );
    }

    #[test]
    fn test_status_update_type_different_statuses_not_equal() {
        assert_ne!(StatusUpdateType::Completed, StatusUpdateType::Failed);
        assert_ne!(StatusUpdateType::Failed, StatusUpdateType::Cancelled);
        assert_ne!(StatusUpdateType::Completed, StatusUpdateType::Cancelled);
    }

    // ========== QueueOperation serialization tests ==========

    #[test]
    fn test_queue_operation_serde_snake_case() {
        let json = serde_json::to_string(&QueueOperation::BatchEnqueue).unwrap();
        assert_eq!(json, "\"batch_enqueue\"");

        let json = serde_json::to_string(&QueueOperation::Enqueue).unwrap();
        assert_eq!(json, "\"enqueue\"");

        let json = serde_json::to_string(&QueueOperation::BatchDequeue).unwrap();
        assert_eq!(json, "\"batch_dequeue\"");
    }

    #[test]
    fn test_queue_operation_deserialize() {
        let op: QueueOperation = serde_json::from_str("\"batch_complete\"").unwrap();
        assert_eq!(op, QueueOperation::BatchComplete);

        let op: QueueOperation = serde_json::from_str("\"batch_fail\"").unwrap();
        assert_eq!(op, QueueOperation::BatchFail);
    }

    #[test]
    fn test_queue_operation_all_variants_serialize() {
        let variants = [
            (QueueOperation::Enqueue, "enqueue"),
            (QueueOperation::Dequeue, "dequeue"),
            (QueueOperation::Complete, "complete"),
            (QueueOperation::Fail, "fail"),
            (QueueOperation::Cancel, "cancel"),
            (QueueOperation::BatchEnqueue, "batch_enqueue"),
            (QueueOperation::BatchDequeue, "batch_dequeue"),
            (QueueOperation::BatchComplete, "batch_complete"),
            (QueueOperation::BatchFail, "batch_fail"),
        ];

        for (op, expected) in &variants {
            let json = serde_json::to_string(op).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
        }
    }
}
