#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskRepository};
    use crate::domain::models::task::{Task, TaskType, TaskStatus};
    use crate::domain::repositories::task_repository::TaskQueryParams;
    use crate::workers::expiration_worker::ExpirationWorker;
    use async_trait::async_trait;
    use mockall::mock;
    use std::sync::Arc;
    use uuid::Uuid;
    use chrono::{DateTime, FixedOffset};
    use sea_orm::DbErr;

    mock! {
        pub TaskRepository {}
        #[async_trait]
        impl TaskRepository for TaskRepository {
            async fn create(&self, task: &Task) -> Result<Task, RepositoryError>;
            async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError>;
            async fn update(&self, task: &Task) -> Result<Task, RepositoryError>;
            async fn acquire_next(&self, worker_id: Uuid) -> Result<Option<Task>, RepositoryError>;
            async fn mark_completed(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn mark_failed(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn mark_cancelled(&self, id: Uuid) -> Result<(), RepositoryError>;
            async fn exists_by_url(&self, url: &str) -> Result<bool, RepositoryError>;
            async fn reset_stuck_tasks(&self, timeout: chrono::Duration) -> Result<u64, RepositoryError>;
            async fn cancel_tasks_by_crawl_id(&self, crawl_id: Uuid) -> Result<u64, RepositoryError>;
            async fn expire_tasks(&self) -> Result<u64, RepositoryError>;
            async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError>;
            async fn query_tasks(&self, params: TaskQueryParams) -> Result<(Vec<Task>, u64), RepositoryError>;
            async fn batch_cancel(&self, task_ids: Vec<Uuid>, team_id: Uuid, force: bool) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>;
        }
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_success() {
        let mut mock_repo = MockTaskRepository::new();
        
        mock_repo
            .expect_expire_tasks()
            .times(1)
            .returning(|| Ok(5)); // Simulate 5 tasks expired

        let repository = Arc::new(mock_repo);
        let worker = ExpirationWorker::new(repository);

        let result = worker.cleanup_expired_tasks().await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5);
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_failure() {
        let mut mock_repo = MockTaskRepository::new();
        
        mock_repo
            .expect_expire_tasks()
            .times(1)
            .returning(|| Err(RepositoryError::Database(DbErr::Custom("Connection failed".to_string()))));

        let repository = Arc::new(mock_repo);
        let worker = ExpirationWorker::new(repository);

        let result = worker.cleanup_expired_tasks().await;
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Database error: Custom Error: Connection failed");
    }
}
