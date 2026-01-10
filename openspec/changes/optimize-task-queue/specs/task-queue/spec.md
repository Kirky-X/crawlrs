## ADDED Requirements

### Requirement: Extended Error Types

系统 SHALL 提供更丰富的错误类型以支持更好的错误处理和调试。

#### Scenario: Repository error propagation
- **WHEN** a repository operation fails
- **THEN** the error SHALL be wrapped in `QueueError::Repository` with the original error message

#### Scenario: Empty queue handling
- **WHEN** dequeue is called on an empty queue
- **THEN** return `QueueError::Empty`

#### Scenario: Timeout handling
- **WHEN** a queue operation exceeds the configured timeout
- **THEN** return `QueueError::Timeout` with duration information

#### Scenario: Shutdown in progress
- **WHEN** a queue operation is called during shutdown
- **THEN** return `QueueError::ShuttingDown`

---

### Requirement: Batch Dequeue

系统 SHALL 支持批量出队操作以提高吞吐量。

#### Scenario: Successful batch dequeue
- **WHEN** `dequeue_batch` is called with size=5
- **THEN** return up to 5 pending tasks ordered by priority
- **AND** all returned tasks SHALL be marked as processing

#### Scenario: Partial batch availability
- **WHEN** `dequeue_batch` is called with size=10 but only 3 tasks available
- **THEN** return all 3 available tasks
- **AND** not return more than available

#### Scenario: Empty queue batch dequeue
- **WHEN** `dequeue_batch` is called on an empty queue
- **THEN** return an empty vector

---

### Requirement: Priority Queue Support

系统 SHALL 支持基于优先级的任务调度。

#### Scenario: High priority task processing first
- **WHEN** multiple tasks with different priorities exist
- **THEN** dequeue SHALL return highest priority tasks first

#### Scenario: Priority levels
- **GIVEN** task priorities: Critical, High, Normal, Low
- **WHEN** acquiring next task
- **THEN** tasks with higher priority SHALL be acquired first
- **AND** tasks with same priority SHALL follow FIFO order

#### Scenario: Priority validation
- **WHEN** enqueueing a task with invalid priority value
- **THEN** reject the task with `QueueError::InvalidPriority`

---

### Requirement: Graceful Shutdown

系统 SHALL 支持优雅关闭，确保正在执行的任务完成。

#### Scenario: Shutdown signal received
- **WHEN** shutdown signal (SIGTERM/SIGINT) is received
- **THEN** queue SHALL stop accepting new tasks
- **AND** running tasks SHALL continue to completion

#### Scenario: Shutdown completion
- **WHEN** all running tasks have completed
- **THEN** shutdown SHALL complete successfully
- **AND** return `Ok(())`

#### Scenario: Shutdown timeout
- **WHEN** shutdown does not complete within configured timeout
- **THEN** force close pending operations
- **AND** return `QueueError::ShutdownTimeout`

#### Scenario: Enqueue during shutdown
- **WHEN** attempting to enqueue after shutdown initiated
- **THEN** return `QueueError::ShuttingDown`

---

### Requirement: Monitoring Metrics

系统 SHALL 导出监控指标以支持生产环境运维。

#### Scenario: Queue depth metric
- **WHEN** metrics are collected
- **THEN** expose current queue depth as a gauge metric

#### Scenario: Tasks processed counter
- **WHEN** a task completes
- **THEN** increment tasks_processed counter
- **AND** record task priority as a label

#### Scenario: Processing duration histogram
- **WHEN** a task completes
- **THEN** record processing duration in histogram
- **AND** support percentile queries

#### Scenario: Failed tasks counter
- **WHEN** a task fails
- **THEN** increment failed_tasks counter
- **AND** record failure reason as label

---

### Requirement: Redis Delayed Queue

系统 SHALL 支持基于 Redis 的延迟任务队列。

#### Scenario: Delayed task enqueue
- **WHEN** enqueue is called with execute_after timestamp
- **THEN** store task in Redis sorted set with score = execute_at timestamp
- **AND** task SHALL not be available for dequeue before execute_at

#### Scenario: Delayed task scan
- **WHEN** scan is called
- **THEN** return all tasks where execute_at <= current time
- **AND** move tasks to available queue

#### Scenario: Delayed task cancellation
- **WHEN** cancel is called for a delayed task
- **THEN** remove task from Redis sorted set
- **AND** return success if task existed

#### Scenario: Delayed task execution
- **WHEN** a delayed task's execute_at time is reached
- **THEN** task SHALL become available for dequeue
- **AND** preserve original task priority

---

### Requirement: Batch Completion

系统 SHALL 支持批量完成任务以提高效率。

#### Scenario: Successful batch completion
- **WHEN** `complete_batch` is called with task IDs [id1, id2, id3]
- **THEN** mark all specified tasks as completed
- **AND** return count of successfully completed tasks

#### Scenario: Partial batch completion
- **WHEN** `complete_batch` is called but some tasks don't exist
- **THEN** complete existing tasks
- **AND** return count of completed tasks
- **AND** not fail the entire operation
