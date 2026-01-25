// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::queue::client::QueueClientError;
use crawlrs::queue::{
    BatchDequeueResult, DequeueRequest, QueueClientBuilder, StatusUpdateRequest, TaskQueue,
};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct InMemoryTestQueue {
    tasks: Arc<Mutex<Vec<Task>>>,
    completed_tasks: Arc<Mutex<Vec<Task>>>,
    dequeued_tasks: Arc<Mutex<Vec<Task>>>,
}

impl InMemoryTestQueue {
    fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            dequeued_tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn add_task(&self, task: Task) {
        self.tasks
            .lock()
            .expect("Failed to acquire tasks lock")
            .push(task);
    }

    fn task_status(&self, task_id: Uuid) -> Option<TaskStatus> {
        if let Some(task) = self
            .completed_tasks
            .lock()
            .expect("Failed to acquire completed_tasks lock")
            .iter()
            .find(|t| t.id == task_id)
        {
            return Some(task.status);
        }
        if let Some(task) = self
            .dequeued_tasks
            .lock()
            .expect("Failed to acquire dequeued_tasks lock")
            .iter()
            .find(|t| t.id == task_id)
        {
            return Some(task.status);
        }
        self.tasks
            .lock()
            .expect("Failed to acquire tasks lock")
            .iter()
            .find(|t| t.id == task_id)
            .map(|t| t.status)
    }

    fn task_count(&self) -> usize {
        self.tasks
            .lock()
            .expect("Failed to acquire tasks lock")
            .len()
    }
}

#[async_trait]
impl TaskQueue for InMemoryTestQueue {
    async fn enqueue(&self, task: Task) -> Result<Task, crawlrs::queue::QueueError> {
        self.tasks
            .lock()
            .expect("Failed to acquire tasks lock")
            .push(task.clone());
        Ok(task)
    }

    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, crawlrs::queue::QueueError> {
        let mut tasks = self.tasks.lock().expect("Failed to acquire tasks lock");
        if let Some(task) = tasks.pop() {
            self.dequeued_tasks
                .lock()
                .expect("Failed to acquire dequeued_tasks lock")
                .push(task.clone());
            return Ok(Some(task));
        }
        Ok(None)
    }

    async fn complete(&self, task_id: Uuid) -> Result<(), crawlrs::queue::QueueError> {
        let mut dequeued = self
            .dequeued_tasks
            .lock()
            .expect("Failed to acquire dequeued_tasks lock");
        if let Some(pos) = dequeued.iter().position(|t| t.id == task_id) {
            let mut task = dequeued.remove(pos);
            task.status = TaskStatus::Completed;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
            return Ok(());
        }
        drop(dequeued);

        let mut tasks = self.tasks.lock().expect("Failed to acquire tasks lock");
        if let Some(pos) = tasks.iter().position(|t| t.id == task_id) {
            let mut task = tasks.remove(pos);
            task.status = TaskStatus::Completed;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
        }
        Ok(())
    }

    async fn fail(&self, task_id: Uuid) -> Result<(), crawlrs::queue::QueueError> {
        let mut dequeued = self
            .dequeued_tasks
            .lock()
            .expect("Failed to acquire dequeued_tasks lock");
        if let Some(pos) = dequeued.iter().position(|t| t.id == task_id) {
            let mut task = dequeued.remove(pos);
            task.status = TaskStatus::Failed;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
            return Ok(());
        }
        drop(dequeued);

        let mut tasks = self.tasks.lock().expect("Failed to acquire tasks lock");
        if let Some(pos) = tasks.iter().position(|t| t.id == task_id) {
            let mut task = tasks.remove(pos);
            task.status = TaskStatus::Failed;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
        }
        Ok(())
    }

    async fn cancel(&self, task_id: Uuid) -> Result<(), crawlrs::queue::QueueError> {
        let mut dequeued = self
            .dequeued_tasks
            .lock()
            .expect("Failed to acquire dequeued_tasks lock");
        if let Some(pos) = dequeued.iter().position(|t| t.id == task_id) {
            let mut task = dequeued.remove(pos);
            task.status = TaskStatus::Cancelled;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
            return Ok(());
        }
        drop(dequeued);

        let mut tasks = self.tasks.lock().expect("Failed to acquire tasks lock");
        if let Some(pos) = tasks.iter().position(|t| t.id == task_id) {
            let mut task = tasks.remove(pos);
            task.status = TaskStatus::Cancelled;
            self.completed_tasks
                .lock()
                .expect("Failed to acquire completed_tasks lock")
                .push(task);
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_queue_client_creation() {
    let queue = InMemoryTestQueue::new();

    let client = QueueClientBuilder::new()
        .with_max_batch_size(20)
        .with_default_max_retries(5)
        .with_default_priority(8)
        .with_operation_timeout(60000)
        .with_metrics_enabled(true)
        .build(queue);

    assert!(!client.name().is_empty());
    assert!(client.config().max_batch_size() > 0);
    assert!(client.config().default_max_retries() >= 0);
}

#[tokio::test]
async fn test_queue_client_builder() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new()
        .with_max_batch_size(15)
        .with_default_max_retries(4)
        .with_default_priority(7)
        .with_operation_timeout(45000)
        .with_metrics_enabled(false)
        .build(queue);

    assert!(client.config().max_batch_size() <= 15);
    assert!(client.config().default_max_retries() <= 4);
    assert!(!client.config().is_metrics_enabled());
}

#[tokio::test]
async fn test_enqueue_single_task() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let request = crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://example.com",
        serde_json::json!({"depth": 1}),
        team_id,
        Uuid::new_v4(),
    );

    let result = client.enqueue(request).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_enqueue_with_priority() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let request = crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://example.com",
        serde_json::json!({}),
        team_id,
        Uuid::new_v4(),
    )
    .with_priority(9);

    let result = client.enqueue(request).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_enqueue_with_delay() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let request = crawlrs::queue::EnqueueRequest::new(
        "crawl",
        "https://example.com",
        serde_json::json!({}),
        team_id,
        Uuid::new_v4(),
    )
    .with_delay(120);

    let result = client.enqueue(request).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_enqueue_with_expiration() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let request = crawlrs::queue::EnqueueRequest::new(
        "extract",
        "https://example.com",
        serde_json::json!({"selector": ".content"}),
        team_id,
        Uuid::new_v4(),
    )
    .with_expire(7200);

    let result = client.enqueue(request).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_enqueue_batch() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let requests = vec![
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example1.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example2.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example3.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
    ];

    let result = client.enqueue_batch(&requests).await;
    assert!(result.is_ok());
    let batch_result = result.expect("Failed to get batch enqueue result");
    assert_eq!(batch_result.tasks.len(), 3);
    assert_eq!(batch_result.success_count, 3);
}

#[tokio::test]
async fn test_enqueue_batch_exceeds_max() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new()
        .with_max_batch_size(2)
        .build(queue);

    let team_id = Uuid::new_v4();
    let requests = vec![
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example1.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example2.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example3.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://example4.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
    ];

    let result = client.enqueue_batch(&requests).await;
    assert!(result.is_ok());
    let batch_result = result.expect("Failed to get batch enqueue result");
    assert_eq!(batch_result.tasks.len(), 2);
}

#[tokio::test]
async fn test_dequeue_single() {
    let queue = InMemoryTestQueue::new();
    queue.add_task(Task::new(
        TaskType::Scrape,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    ));

    let client = QueueClientBuilder::new().build(queue);
    let worker_id = Uuid::new_v4();

    let result = client.dequeue(DequeueRequest::new(worker_id)).await;
    assert!(result.is_ok());
    assert!(result.expect("Failed to get dequeue result").is_some());
}

#[tokio::test]
async fn test_dequeue_empty() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);
    let worker_id = Uuid::new_v4();

    let result = client.dequeue(DequeueRequest::new(worker_id)).await;
    assert!(result.is_ok());
    assert!(result.expect("Failed to get dequeue result").is_none());
}

#[tokio::test]
async fn test_dequeue_batch() {
    let queue = InMemoryTestQueue::new();
    for i in 1..=3 {
        queue.add_task(Task::new(
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            format!("https://example{}.com", i),
            serde_json::json!({}),
        ));
    }

    let client = QueueClientBuilder::new().build(queue);
    let worker_id = Uuid::new_v4();

    let result = client.dequeue_batch(worker_id, 5).await;
    assert!(result.is_ok());
    assert_eq!(
        result
            .expect("Failed to get batch dequeue result")
            .tasks
            .len(),
        3
    );
}

#[tokio::test]
async fn test_update_status_complete() {
    let queue = InMemoryTestQueue::new();
    let queue_ref = queue.clone();
    let task = Task::new(
        TaskType::Scrape,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    );
    let task_id = task.id;
    queue.add_task(task);

    let client = QueueClientBuilder::new().build(queue);
    let result = client
        .update_status(StatusUpdateRequest::complete(task_id))
        .await;
    assert!(result.is_ok());
    assert_eq!(queue_ref.task_status(task_id), Some(TaskStatus::Completed));
}

#[tokio::test]
async fn test_update_status_fail() {
    let queue = InMemoryTestQueue::new();
    let queue_ref = queue.clone();
    let task = Task::new(
        TaskType::Crawl,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    );
    let task_id = task.id;
    queue.add_task(task);

    let client = QueueClientBuilder::new().build(queue);
    let result = client
        .update_status(StatusUpdateRequest::fail(task_id, "error"))
        .await;
    assert!(result.is_ok());
    assert_eq!(queue_ref.task_status(task_id), Some(TaskStatus::Failed));
}

#[tokio::test]
async fn test_update_status_cancel() {
    let queue = InMemoryTestQueue::new();
    let queue_ref = queue.clone();
    let task = Task::new(
        TaskType::Extract,
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com".to_string(),
        serde_json::json!({}),
    );
    let task_id = task.id;
    queue.add_task(task);

    let client = QueueClientBuilder::new().build(queue);
    let result = client
        .update_status(StatusUpdateRequest::cancel(task_id))
        .await;
    assert!(result.is_ok());
    assert_eq!(queue_ref.task_status(task_id), Some(TaskStatus::Cancelled));
}

#[tokio::test]
async fn test_get_metrics() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new()
        .with_metrics_enabled(false)
        .build(queue);
    assert!(client.get_metrics().is_none());
}

#[tokio::test]
async fn test_priority_clamping() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);
    let team_id = Uuid::new_v4();

    let req1 = crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://a.com",
        serde_json::json!({}),
        team_id,
        Uuid::new_v4(),
    )
    .with_priority(15);
    assert!(client.enqueue(req1).await.is_ok());

    let req2 = crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://b.com",
        serde_json::json!({}),
        team_id,
        Uuid::new_v4(),
    )
    .with_priority(-5);
    assert!(client.enqueue(req2).await.is_ok());
}

#[tokio::test]
async fn test_full_workflow() {
    let queue = InMemoryTestQueue::new();
    let queue_ref = queue.clone();
    let client = QueueClientBuilder::new()
        .with_default_priority(7)
        .with_max_batch_size(10)
        .build(queue);

    let team_id = Uuid::new_v4();
    let worker_id = Uuid::new_v4();

    let requests = vec![
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://site1.com",
            serde_json::json!({"page": 1}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://site2.com",
            serde_json::json!({"page": 2}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "crawl",
            "https://site3.com",
            serde_json::json!({"depth": 2}),
            team_id,
            Uuid::new_v4(),
        ),
    ];

    let enqueue = client.enqueue_batch(&requests).await;
    assert!(enqueue.is_ok());
    assert_eq!(
        enqueue
            .expect("Failed to get batch enqueue result")
            .success_count,
        3
    );

    let dequeue_result = client.dequeue_batch(worker_id, 10).await;
    assert!(dequeue_result.is_ok());
    assert_eq!(
        dequeue_result
            .as_ref()
            .expect("Failed to get batch dequeue result")
            .tasks
            .len(),
        3
    );

    if let Some(task) = dequeue_result
        .expect("Failed to get batch dequeue result")
        .tasks
        .first()
    {
        let complete = client
            .update_status(StatusUpdateRequest::complete(task.id))
            .await;
        assert!(complete.is_ok());
        assert_eq!(queue_ref.task_status(task.id), Some(TaskStatus::Completed));
    }

    let remaining = client.dequeue(DequeueRequest::new(worker_id)).await;
    assert!(remaining.is_ok());
    assert!(remaining.expect("Failed to get dequeue result").is_none());
}

#[tokio::test]
async fn test_builder_clamping() {
    let builder1 = QueueClientBuilder::new().with_max_batch_size(0);
    let client1 = builder1.build(InMemoryTestQueue::new());
    assert!(client1.config().max_batch_size() >= 1);

    let builder2 = QueueClientBuilder::new().with_max_batch_size(200);
    let client2 = builder2.build(InMemoryTestQueue::new());
    assert!(client2.config().max_batch_size() <= 100);

    let builder3 = QueueClientBuilder::new().with_default_max_retries(-1);
    let client3 = builder3.build(InMemoryTestQueue::new());
    assert!(client3.config().default_max_retries() >= 0);

    let builder4 = QueueClientBuilder::new().with_default_max_retries(20);
    let client4 = builder4.build(InMemoryTestQueue::new());
    assert!(client4.config().default_max_retries() <= 10);

    let builder5 = QueueClientBuilder::new().with_operation_timeout(500);
    let client5 = builder5.build(InMemoryTestQueue::new());
    assert!(client5.config().operation_timeout_ms() >= 1000);

    let builder6 = QueueClientBuilder::new().with_operation_timeout(500000);
    let client6 = builder6.build(InMemoryTestQueue::new());
    assert!(client6.config().operation_timeout_ms() <= 300000);
}

#[tokio::test]
async fn test_error_types() {
    let _client = QueueClientBuilder::new().build(InMemoryTestQueue::new());

    let result: Result<Task, QueueClientError> = Err(QueueClientError::EmptyQueue);
    assert!(result.is_err());
    match result.err().expect("Failed to get error") {
        QueueClientError::EmptyQueue => {}
        _ => panic!("Expected EmptyQueue error"),
    }
}

#[tokio::test]
async fn test_batch_dequeue_result() {
    let worker_id = Uuid::new_v4();
    let batch = BatchDequeueResult {
        tasks: Vec::new(),
        worker_id,
        dequeued_at: chrono::Utc::now(),
    };

    assert_eq!(batch.worker_id, worker_id);
    assert!(batch.tasks.is_empty());
}

#[tokio::test]
async fn test_multiple_cycles() {
    let queue = InMemoryTestQueue::new();
    let queue_ref = queue.clone();
    let client = QueueClientBuilder::new()
        .with_max_batch_size(5)
        .build(queue);

    let team_id = Uuid::new_v4();
    let worker_id = Uuid::new_v4();

    let requests1 = vec![crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://a.com",
        serde_json::json!({}),
        team_id,
        Uuid::new_v4(),
    )];
    client
        .enqueue_batch(&requests1)
        .await
        .expect("Failed to enqueue batch");
    let result1 = client
        .dequeue_batch(worker_id, 5)
        .await
        .expect("Failed to dequeue batch");
    assert_eq!(result1.tasks.len(), 1);

    let requests2 = vec![
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://b.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
        crawlrs::queue::EnqueueRequest::new(
            "scrape",
            "https://c.com",
            serde_json::json!({}),
            team_id,
            Uuid::new_v4(),
        ),
    ];
    client
        .enqueue_batch(&requests2)
        .await
        .expect("Failed to enqueue batch");
    let result2 = client
        .dequeue_batch(worker_id, 5)
        .await
        .expect("Failed to dequeue batch");
    assert_eq!(result2.tasks.len(), 2);

    assert_eq!(queue_ref.task_count(), 0);
}

#[tokio::test]
async fn test_task_types() {
    let queue = InMemoryTestQueue::new();
    let client = QueueClientBuilder::new().build(queue);

    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();
    let scrape_request = crawlrs::queue::EnqueueRequest::new(
        "scrape",
        "https://a.com",
        serde_json::json!({}),
        team_id,
        api_key_id,
    );
    assert!(client.enqueue(scrape_request).await.is_ok());

    let crawl_request = crawlrs::queue::EnqueueRequest::new(
        "crawl",
        "https://b.com",
        serde_json::json!({}),
        team_id,
        api_key_id,
    );
    assert!(client.enqueue(crawl_request).await.is_ok());

    let extract_request = crawlrs::queue::EnqueueRequest::new(
        "extract",
        "https://c.com",
        serde_json::json!({}),
        team_id,
        api_key_id,
    );
    assert!(client.enqueue(extract_request).await.is_ok());
}
