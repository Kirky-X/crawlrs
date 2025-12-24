#[cfg(test)]
mod tests {
    use crate::domain::models::task::{TaskStatus, TaskType};
    use crate::infrastructure::database::entities::{task, team};
    use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
    use crate::workers::expiration_worker::ExpirationWorker;
    use chrono::Utc;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, EntityTrait, Set};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn setup_db() -> Arc<DatabaseConnection> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let db = Arc::new(db);
        Migrator::up(db.as_ref(), None).await.unwrap();
        db
    }

    async fn create_team(db: &DatabaseConnection) -> Uuid {
        let team_id = Uuid::new_v4();
        let team = team::ActiveModel {
            id: Set(team_id),
            name: Set("Test Team".to_string()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
            enable_geo_restrictions: Set(false),
            ..Default::default()
        };
        team.insert(db).await.unwrap();
        team_id
    }

    async fn create_test_task(
        db: &DatabaseConnection,
        team_id: Uuid,
        status: TaskStatus,
        created_at_offset_hours: i64,
        started_at_offset_hours: Option<i64>,
    ) -> Uuid {
        let task_id = Uuid::new_v4();
        let created_at = Utc::now() - chrono::Duration::hours(created_at_offset_hours);

        let mut task = task::ActiveModel {
            id: Set(task_id),
            team_id: Set(team_id),
            task_type: Set(TaskType::Scrape.to_string()),
            status: Set(status.to_string()),
            url: Set("http://example.com".to_string()),
            payload: Set(serde_json::json!({})),
            priority: Set(0),
            max_retries: Set(3),
            attempt_count: Set(0),
            created_at: Set(created_at.into()),
            updated_at: Set(Utc::now().into()),
            ..Default::default()
        };

        if let Some(offset) = started_at_offset_hours {
            let started_at = Utc::now() - chrono::Duration::hours(offset);
            task.started_at = Set(Some(started_at.into()));
        }

        task.insert(db).await.unwrap();
        task_id
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_success() {
        let db = setup_db().await;
        let repo = TaskRepositoryImpl::new(db.clone(), chrono::Duration::seconds(60));
        let repository = Arc::new(repo);
        let worker = ExpirationWorker::new(repository);
        let team_id = create_team(&db).await;

        // Create tasks that should be expired
        // 1. Queued task older than 24h (25h old)
        create_test_task(&db, team_id, TaskStatus::Queued, 25, None).await;

        // 2. Active task started more than 24h ago (25h ago)
        create_test_task(&db, team_id, TaskStatus::Active, 26, Some(25)).await;

        // Create tasks that should NOT be expired
        // 3. Queued task newer than 24h (23h old)
        create_test_task(&db, team_id, TaskStatus::Queued, 23, None).await;

        // 4. Active task started less than 24h ago (23h ago)
        create_test_task(&db, team_id, TaskStatus::Active, 24, Some(23)).await;

        let result = worker.cleanup_expired_tasks().await;

        assert!(result.is_ok());
        // Should expire 2 tasks
        assert_eq!(result.unwrap(), 2);

        // Verify statuses in DB
        let tasks = task::Entity::find().all(db.as_ref()).await.unwrap();
        let failed_count = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Failed.to_string())
            .count();
        let queued_count = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued.to_string())
            .count();
        let active_count = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Active.to_string())
            .count();

        assert_eq!(failed_count, 2);
        assert_eq!(queued_count, 1);
        assert_eq!(active_count, 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_empty() {
        let db = setup_db().await;
        let repo = TaskRepositoryImpl::new(db.clone(), chrono::Duration::seconds(60));
        let repository = Arc::new(repo);
        let worker = ExpirationWorker::new(repository);
        let team_id = create_team(&db).await;

        // Create only non-expired tasks
        create_test_task(&db, team_id, TaskStatus::Queued, 10, None).await;
        create_test_task(&db, team_id, TaskStatus::Active, 10, Some(5)).await;

        let result = worker.cleanup_expired_tasks().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }
}
