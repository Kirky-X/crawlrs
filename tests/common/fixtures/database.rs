// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(dead_code)]

/// 数据库测试固件
///
/// 提供内存数据库和PostgreSQL数据库的设置和清理功能
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use std::sync::Arc;

/// 数据库连接选项
#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    /// 数据库URL
    pub url: String,
    /// 是否使用Redis
    pub use_redis: bool,
    /// Redis端口
    pub redis_port: u16,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            url: std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
                let db_password = std::env::var("TEST_DATABASE_PASSWORD")
                    .unwrap_or_else(|_| "password".to_string());
                format!(
                    "postgres://crawlrs:{}@localhost:5433/crawlrs_test",
                    db_password
                )
            }),
            use_redis: true,
            redis_port: 6381,
        }
    }
}

/// 数据库测试固件
pub struct DatabaseFixture {
    /// 数据库连接池
    pub db_pool: Arc<DatabaseConnection>,
    /// 数据库URL
    pub db_url: String,
    /// 数据库后端类型
    pub db_backend: DbBackend,
}

impl DatabaseFixture {
    /// 创建新的数据库固件（使用默认配置）
    pub async fn new() -> Self {
        Self::with_options(DatabaseOptions::default()).await
    }

    /// 使用指定选项创建数据库固件
    pub async fn with_options(options: DatabaseOptions) -> Self {
        let db = Database::connect(&options.url).await.unwrap();
        let db_pool = Arc::new(db);

        // 运行数据库迁移
        Migrator::up(db_pool.as_ref(), None).await.unwrap();

        let db_backend = if options.url.starts_with("postgres://") {
            DbBackend::Postgres
        } else {
            DbBackend::Sqlite
        };

        Self {
            db_pool,
            db_url: options.url,
            db_backend,
        }
    }

    /// 清理测试数据（删除以 https://example.com/ 开头的任务）
    pub async fn cleanup_test_data(&self) {
        let cleanup_pattern = "https://example.com/%";

        match self.db_backend {
            DbBackend::Postgres => {
                // PostgreSQL语法
                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "DELETE FROM tasks WHERE url LIKE $1",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();

                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "DELETE FROM tasks_backlog WHERE payload->>'url' LIKE $1",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();

                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "DELETE FROM scrape_results WHERE url LIKE $1",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();
            }
            DbBackend::Sqlite => {
                // SQLite语法
                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "DELETE FROM tasks WHERE url LIKE ?",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();

                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "DELETE FROM tasks_backlog WHERE payload->>'url' LIKE ?",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();

                self.db_pool
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "DELETE FROM scrape_results WHERE url LIKE ?",
                        vec![cleanup_pattern.into()],
                    ))
                    .await
                    .unwrap();
            }
            _ => {}
        }
    }
}

impl Drop for DatabaseFixture {
    fn drop(&mut self) {
        // 可以在此添加清理逻辑
    }
}
