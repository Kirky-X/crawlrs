// Minimal test helpers for queue_client_test and task_repository_test
// This file provides just enough functionality to run the basic tests

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Minimal TestApp struct for testing
pub struct TestApp {
    pub db_pool: Arc<DatabaseConnection>,
    pub team_id: Uuid,
    pub redis_port: u16,
    pub redis_process: Option<std::process::Child>,
}

impl TestApp {
    /// Get a reference to the database pool
    pub fn db_pool(&self) -> &Arc<DatabaseConnection> {
        &self.db_pool
    }
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

    // 生成唯一的 team_id
    let team_id = Uuid::new_v4();

    // 先在 teams 表中创建团队（因为 tasks 表有 foreign key 约束）
    let _ = db_pool.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO teams (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING",
        vec![team_id.into(), format!("Test Team {}", team_id).into()],
    )).await;

    // 清理测试数据 - 在开始时就清理，确保干净的测试环境
    // 使用 sqlx 直接清理（更可靠）
    if db_url.starts_with("postgres://") {
        let sqlx_pool = sqlx::PgPool::connect(&db_url).await.expect("Failed to connect to sqlx pool");
        sqlx::query("DELETE FROM tasks")
            .execute(&sqlx_pool)
            .await
            .expect("Failed to delete tasks");
    }

    let start_port = 8000;
    let result =
        crawlrs::utils::port_sniffer::PortSniffer::find_available_port(start_port, true, 100)
            .expect("Failed to find available port");
    let redis_port = result.port;

    // Start Redis
    let redis_process = Some(
        std::process::Command::new("redis-server")
            .arg("--port")
            .arg(redis_port.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start redis-server"),
    );

    tokio::time::sleep(Duration::from_millis(500)).await;
    let redis_url = format!("redis://127.0.0.1:{}", redis_port);
    let _ = crawlrs::infrastructure::cache::redis_client::RedisClient::new(&redis_url);

    TestApp {
        db_pool,
        team_id,
        redis_port,
        redis_process,
    }
}

/// Create a test app (same as no_worker for now)
pub async fn create_test_app() -> TestApp {
    create_test_app_no_worker().await
}
