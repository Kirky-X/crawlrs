// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 统一的任务队列客户端
//!
//! 提供统一的任务队列操作接口，支持单任务和批量操作、优先级、指标收集等

use crate::domain::models::task::Task;
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
    /// 延迟执行时间（秒）
    delay_seconds: Option<u64>,
    /// 过期时间（秒）
    expire_seconds: Option<u64>,
    /// 最大重试次数
    max_retries: Option<i32>,
}

impl EnqueueRequest {
    /// 创建新的入队请求
    pub fn new(task_type: &str, url: &str, payload: serde_json::Value, team_id: Uuid) -> Self {
        Self {
            task_type: task_type.to_string(),
            url: url.to_string(),
            payload,
            priority: None,
            team_id,
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

        let mut task = Task::new(task_type, request.team_id, request.url, request.payload);

        task.priority = priority;
        task.max_retries = request
            .max_retries
            .unwrap_or(self.config.default_max_retries);

        if let Some(delay) = request.delay_seconds {
            task.scheduled_at =
                Some((chrono::Utc::now() + chrono::Duration::seconds(delay as i64)).into());
        }

        if let Some(expire) = request.expire_seconds {
            task.expires_at =
                Some((chrono::Utc::now() + chrono::Duration::seconds(expire as i64)).into());
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
                task_type,
                request.team_id,
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
        let request = EnqueueRequest::new(
            "scrape",
            "https://example.com",
            serde_json::json!({}),
            team_id,
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
        let request = EnqueueRequest::new(
            "scrape",
            "https://example.com",
            serde_json::json!({}),
            team_id,
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
