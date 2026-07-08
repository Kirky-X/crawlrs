// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
#[cfg(feature = "dbnexus-sqlite")]
mod expiration_worker_tests {
    use crate::domain::models::{TaskStatus, TaskType};
    use crate::infrastructure::database::entities::{task, team};
    use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
    use crate::workers::expiration_worker::ExpirationWorker;
    use chrono::Utc;
    use dbnexus::{DbConfig, DbPool};
    use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set, Statement};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn setup_db() -> Arc<DbPool> {
        // SQLite in-memory 必须共享同一连接，否则每个连接有独立的数据库
        let config = DbConfig {
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            min_connections: 0,
            ..Default::default()
        };
        let pool = DbPool::with_config(config)
            .await
            .expect("Failed to create DbPool");
        let pool = Arc::new(pool);
        let session = pool
            .get_session("admin")
            .await
            .expect("Failed to get session");
        let conn = session
            .connection()
            .expect("Failed to get connection");

        let create_teams_sql = r#"
            CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                allowed_countries TEXT,
                blocked_countries TEXT,
                ip_whitelist TEXT,
                domain_blacklist TEXT,
                enable_geo_restrictions INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#;
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            create_teams_sql.to_string(),
        ))
        .await
        .expect("Failed to create teams table");

        let create_tasks_sql = r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                task_type TEXT NOT NULL,
                team_id TEXT NOT NULL,
                api_key_id TEXT NOT NULL,
                crawl_id TEXT,
                url TEXT NOT NULL,
                status TEXT NOT NULL,
                priority INTEGER DEFAULT 0,
                payload TEXT,
                retry_count INTEGER DEFAULT 0,
                max_retries INTEGER DEFAULT 3,
                scheduled_at TEXT,
                expires_at TEXT,
                completed_at TEXT,
                lock_token TEXT,
                lock_expires_at TEXT,
                started_at TEXT,
                attempt_count INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#;
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            create_tasks_sql.to_string(),
        ))
        .await
        .expect("Failed to create tasks table");

        let create_api_keys_sql = r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL,
                key_hash TEXT,
                team_id TEXT NOT NULL,
                name TEXT,
                scopes TEXT,
                rate_limit_rpm INTEGER,
                rate_limit_concurrent INTEGER,
                expires_at TEXT,
                last_used_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#;
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            create_api_keys_sql.to_string(),
        ))
        .await
        .expect("Failed to create api_keys table");

        drop(session);
        pool
    }

    async fn create_team(db: &DbPool) -> Uuid {
        let team_id = Uuid::new_v4();
        let session = db.get_session("admin").await.expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");
        let team = team::ActiveModel {
            id: Set(team_id),
            name: Set("Test Team".to_string()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
            enable_geo_restrictions: Set(false),
            ..Default::default()
        };
        team.insert(conn)
            .await
            .expect("Failed to insert team into database");
        team_id
    }

    async fn create_api_key(db: &DbPool, team_id: Uuid) -> Uuid {
        let api_key_id = Uuid::new_v4();
        let session = db.get_session("admin").await.expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");

        let insert_sql = format!(
            r#"
                INSERT INTO api_keys (id, key, key_hash, team_id, created_at, updated_at)
                VALUES ('{}', 'test-key-{}', 'hash-{}', '{}', '{}', '{}')
            "#,
            api_key_id,
            api_key_id,
            api_key_id,
            team_id,
            Utc::now().format("%Y-%m-%d %H:%M:%S.%f UTC"),
            Utc::now().format("%Y-%m-%d %H:%M:%S.%f UTC")
        );
        conn.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            insert_sql,
        ))
        .await
        .expect("Failed to insert api_key into database");

        api_key_id
    }

    async fn create_test_task(
        db: &DbPool,
        team_id: Uuid,
        api_key_id: Uuid,
        status: TaskStatus,
        created_at_offset_hours: i64,
        started_at_offset_hours: Option<i64>,
    ) -> Uuid {
        let task_id = Uuid::new_v4();
        let created_at = Utc::now() - chrono::Duration::hours(created_at_offset_hours);

        let mut task = task::ActiveModel {
            id: Set(task_id),
            team_id: Set(team_id),
            api_key_id: Set(api_key_id),
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

        let session = db.get_session("admin").await.expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");
        task.insert(conn)
            .await
            .expect("Failed to insert task into database");
        task_id
    }

    #[tokio::test]
    async fn test_cleanup_expired_tasks_success() {
        let db = setup_db().await;
        let repo = TaskRepositoryImpl::new(db.clone(), chrono::Duration::seconds(60));
        let repository = Arc::new(repo);
        let worker = ExpirationWorker::new(repository);
        let team_id = create_team(&db).await;
        let api_key_id = create_api_key(&db, team_id).await;

        // Create tasks that should be expired
        // 1. Queued task older than 24h (25h old)
        create_test_task(&db, team_id, api_key_id, TaskStatus::Queued, 25, None).await;

        // 2. Active task started more than 24h ago (25h ago)
        create_test_task(&db, team_id, api_key_id, TaskStatus::Active, 26, Some(25)).await;

        // Create tasks that should NOT be expired
        // 3. Queued task newer than 24h (23h old)
        create_test_task(&db, team_id, api_key_id, TaskStatus::Queued, 23, None).await;

        // 4. Active task started less than 24h ago (23h ago)
        create_test_task(&db, team_id, api_key_id, TaskStatus::Active, 24, Some(23)).await;

        let result = worker.cleanup_expired_tasks().await;

        assert!(result.is_ok());
        // Should expire 2 tasks
        assert_eq!(result.expect("Cleanup result should be OK"), 2);

        // Verify statuses in DB
        let session = db.get_session("admin").await.expect("Failed to get session");
        let conn = session.connection().expect("Failed to get connection");
        let tasks = task::Entity::find()
            .all(conn)
            .await
            .expect("Failed to fetch tasks from database");
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
        let api_key_id = create_api_key(&db, team_id).await;

        // Create only non-expired tasks
        create_test_task(&db, team_id, api_key_id, TaskStatus::Queued, 10, None).await;
        create_test_task(&db, team_id, api_key_id, TaskStatus::Active, 10, Some(5)).await;

        let result = worker.cleanup_expired_tasks().await;

        assert!(result.is_ok());
        assert_eq!(result.expect("Cleanup result should be OK"), 0);
    }
}
