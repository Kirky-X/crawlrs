use async_trait::async_trait;
use crawlrs::domain::models::task::Task;
use crawlrs::queue::task_queue::{QueueError, TaskQueue};
use crawlrs::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crawlrs::domain::repositories::task_repository::TaskRepository;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MockTaskQueue {
    tasks: Mutex<Vec<Task>>,
    repo: Option<Arc<TaskRepositoryImpl>>,
}

impl MockTaskQueue {
    pub fn new(repo: Option<Arc<TaskRepositoryImpl>>) -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            repo,
        }
    }
}

#[async_trait]
impl TaskQueue for MockTaskQueue {
    async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
        if let Some(repo) = &self.repo {
            repo.create(&task).await.map_err(QueueError::Repository)?;
        }
        self.tasks.lock().unwrap().push(task.clone());
        Ok(task)
    }

    async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
        // Simple FIFO for mock
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.pop() {
             Ok(Some(task))
        } else {
             Ok(None)
        }
    }

    async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }

    async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
        Ok(())
    }
}
