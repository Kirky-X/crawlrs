use super::helpers::create_test_app_no_worker;
use chrono::{Duration, Utc};
use crawlrs::domain::models::task::{Task, TaskStatus, TaskType};
use crawlrs::domain::repositories::task_repository::TaskRepository;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn test_reset_stuck_tasks() {
    let app = create_test_app_no_worker().await;

    // 1. Create a task that is "stuck"
    // Status = Active, LockExpiresAt = past time
    let task_id = Uuid::new_v4();
    let past_time = Utc::now() - Duration::minutes(10);

    let task = Task {
        id: task_id,
        team_id: Uuid::new_v4(),
        url: "http://example.com/stuck".to_string(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        payload: json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: past_time.into(),
        started_at: Some(past_time.into()),
        completed_at: None,
        crawl_id: None,
        updated_at: past_time.into(),
        lock_token: Some(Uuid::new_v4()),
        lock_expires_at: Some(past_time.into()), // Expired
    };

    app.task_repo.create(&task).await.unwrap();
    // We need to manually update it to Active/Locked because create usually sets defaults or we need to ensure the DB state matches.
    // However, our repository implementation inserts what we give it, or sets defaults if missing.
    // The create implementation uses `model.insert`, and `ActiveModel` From<Task> sets all fields.
    // But let's double check if we need to force update it to ensure it's stuck.
    // Actually `create` in repo inserts the task as provided.
    // But let's verify if `create` respects the status and lock fields.
    // Looking at TaskRepositoryImpl::create:
    // model: task_entity::ActiveModel = task.clone().into();
    // model.insert(self.db.as_ref()).await?;
    // Yes, it inserts as is.

    // 2. Create a task that is Active but valid (not expired)
    let valid_task_id = Uuid::new_v4();
    let future_time = Utc::now() + Duration::minutes(10);
    let valid_task = Task {
        id: valid_task_id,
        team_id: Uuid::new_v4(),
        url: "http://example.com/valid".to_string(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Active,
        priority: 0,
        payload: json!({}),
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: Utc::now().into(),
        started_at: Some(Utc::now().into()),
        completed_at: None,
        crawl_id: None,
        updated_at: Utc::now().into(),
        lock_token: Some(Uuid::new_v4()),
        lock_expires_at: Some(future_time.into()), // Not Expired
    };
    app.task_repo.create(&valid_task).await.unwrap();

    // 3. Call reset_stuck_tasks
    // Timeout parameter is used to check against started_at if lock_expires_at is null.
    // But here we rely on lock_expires_at logic mostly.
    // The query is: (Status=Active AND (LockExpiresAt <= Now OR (LockExpiresAt IS NULL AND StartedAt <= Threshold)))
    let affected = app
        .task_repo
        .reset_stuck_tasks(Duration::minutes(5))
        .await
        .unwrap();

    // 4. Verify results
    assert_eq!(affected, 1, "Should reset exactly 1 task");

    let updated_stuck_task = app.task_repo.find_by_id(task_id).await.unwrap().unwrap();
    assert_eq!(updated_stuck_task.status, TaskStatus::Queued);
    assert!(updated_stuck_task.lock_token.is_none());
    assert!(updated_stuck_task.lock_expires_at.is_none());

    let updated_valid_task = app
        .task_repo
        .find_by_id(valid_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_valid_task.status, TaskStatus::Active);
}
