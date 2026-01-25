// Minimal test helpers for queue_client_test and task_repository_test
// This file provides just enough functionality to run the basic tests

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use std::sync::Arc;
use uuid::Uuid;

/// Minimal TestApp struct for testing
#[allow(dead_code)]
pub struct TestApp {
    pub db_pool: Arc<DatabaseConnection>,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub redis_port: u16,
    pub redis_process: Option<std::process::Child>,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if let Some(mut process) = self.redis_process.take() {
            let _ = process.kill();
        }
    }
}

/// Create a minimal test app without starting workers
pub async fn create_test_app_no_worker() -> TestApp {
    let db_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        let db_password =
            std::env::var("TEST_DATABASE_PASSWORD").unwrap_or_else(|_| "password".to_string());
        format!(
            "postgres://crawlrs:{}@localhost:5443/crawlrs_test",
            db_password
        )
    });

    let db = Database::connect(&db_url)
        .await
        .expect("Failed to connect to database");
    let db_pool = Arc::new(db);

    // 生成唯一的 team_id 和 api_key_id
    let team_id = Uuid::new_v4();
    let api_key_id = Uuid::new_v4();

    // 先在 teams 表中创建团队（因为 tasks 表有 foreign key 约束）
    let _ = db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO teams (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING",
            vec![team_id.into(), format!("Test Team {}", team_id).into()],
        ))
        .await;

    // 创建 API key（因为 tasks 表有外键约束）
    let _ = db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO api_keys (id, key, key_hash, team_id) VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO NOTHING",
            vec![
                api_key_id.into(),
                format!("test-api-key-{}", api_key_id).into(),
                format!("hash-{}", api_key_id).into(),
                team_id.into(),
            ],
        ))
        .await;

    // 清理测试数据 - 只清理 tasks，保留 api_keys 和 teams
    let _ = db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM tasks",
            vec![],
        ))
        .await;
    let _ = db_pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM tasks_backlog",
            vec![],
        ))
        .await;

    let redis_port = std::env::var("TEST_REDIS_PORT")
        .unwrap_or_else(|_| "6380".to_string())
        .parse::<u16>()
        .expect("Invalid redis port");

    // 启动 Redis 进程（如果需要）
    let redis_process = Some(
        std::process::Command::new("redis-server")
            .arg("--port")
            .arg(redis_port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start redis-server"),
    );

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);
    let _ = crawlrs::infrastructure::cache::redis_client::RedisClient::new(&redis_url);

    TestApp {
        db_pool,
        team_id,
        api_key_id,
        redis_port,
        redis_process,
    }
}

/// Create a test app (same as no_worker for now)
pub async fn create_test_app() -> TestApp {
    create_test_app_no_worker().await
}
