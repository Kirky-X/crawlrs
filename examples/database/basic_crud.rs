// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 数据库 CRUD 基础示例
//!
//! 演示如何使用 dbnexus 进行数据库连接池创建和基本 CRUD 操作。
//! 本示例需要运行中的 PostgreSQL 数据库才能实际执行查询；
//! 默认仅打印 API 用法说明，实际查询代码以函数形式展示。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_crud
//! ```
//!
//! # 前置条件
//!
//! - PostgreSQL 数据库（连接字符串可通过环境变量 `DATABASE_URL` 覆盖）
//! - 已启用 `dbnexus-postgres` 特性的 crawlrs 主项目

use dbnexus::{CacheConfig, DbConfig, DbPool, Session};
use log::info;

/// 数据库连接 URL（示例值，生产环境请通过环境变量覆盖）
const DATABASE_URL: &str = "postgresql://postgres:postgres@localhost/crawlrs";

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 dbnexus CRUD 基础示例");
    info!("=====================================\n");

    // 1. DbConfig 构造说明
    info!("1️⃣  构造 DbConfig");
    info!("-----------------------------");
    info!("📖 DbConfig 是 dbnexus 的核心配置结构，包含连接池参数、权限、迁移等设置");
    info!("");

    let config = build_sample_config();
    info!("✅ 已构造 DbConfig:");
    info!("   url: {}", config.url);
    info!("   max_connections: {}", config.max_connections);
    info!("   min_connections: {}", config.min_connections);
    info!("   idle_timeout: {}s", config.idle_timeout);
    info!("   acquire_timeout: {}ms", config.acquire_timeout);
    info!("   admin_role: {}", config.admin_role);
    info!("   auto_migrate: {}", config.auto_migrate);
    info!("");

    // 2. 创建连接池
    info!("2️⃣  创建连接池");
    info!("-----------------------------");
    info!("📖 通过 DbPool::with_config 异步创建连接池");
    info!("   连接池会预热 min_connections 个连接并加载权限配置");
    info!("");
    info!("📌 实际执行需要可访问的 PostgreSQL，本示例仅展示调用方式：");
    info!("   let pool = DbPool::with_config(config).await?;");
    info!("");

    // 3. 获取会话
    info!("3️⃣  获取 Session 执行查询");
    info!("-----------------------------");
    info!("📖 Session 是数据库会话句柄，按角色分配权限");
    info!("   - admin:    完整权限（绕过权限检查）");
    info!("   - system:   系统内部操作");
    info!("   - readonly: 只读权限");
    info!("");
    info!("📌 示例调用：");
    info!("   let session = pool.get_session(\"admin\").await?;");
    info!("");

    // 4. CRUD 操作演示
    info!("4️⃣  CRUD 操作代码片段");
    info!("-----------------------------");
    info!("📖 dbnexus Session 提供两种 CRUD 模式：");
    info!("   1. 原生 SQL：通过 session.execute(sql) 执行（带权限检查）");
    info!("   2. SeaORM 实体：通过 session.connection()? 获取连接后使用 Entity API");
    info!("");

    print_crud_snippets();

    info!("\n=====================================");
    info!("✨ dbnexus CRUD 基础示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - 生产环境通过 crawlrs::infrastructure::database::DatabasePool 复用连接池");
    info!("   - Session 在 Drop 时自动归还连接池");
    info!("   - 权限配置在 config/permissions.yaml 中声明");
}

/// 构造示例 DbConfig（不连接数据库）
fn build_sample_config() -> DbConfig {
    DbConfig {
        url: DATABASE_URL.to_string(),
        max_connections: 100,
        min_connections: 10,
        idle_timeout: 300,
        acquire_timeout: 30_000,
        permissions_path: None,
        migrations_dir: None,
        auto_migrate: false,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: CacheConfig::default(),
    }
}

/// 打印 CRUD 代码片段说明
fn print_crud_snippets() {
    info!("📝 模式一：原生 SQL（通过 Session::execute）");
    info!("   // CREATE");
    info!("   session.execute(\"INSERT INTO tasks (id, url) VALUES (gen_random_uuid(), 'https://example.com')\").await?;");
    info!("");
    info!("   // READ（返回 ExecResult，行数统计）");
    info!("   let result = session.execute(\"UPDATE tasks SET status = 'completed' WHERE id = '...'\").await?;");
    info!("   info!(\"受影响行数: {{}}\", result.rows_affected());");
    info!("");
    info!("📝 模式二：SeaORM 实体（推荐，类型安全）");
    info!("   use sea_orm::EntityTrait;");
    info!("");
    info!("   // CREATE");
    info!("   let conn = session.connection()?;");
    info!("   let model = task::ActiveModel {{ ... }};");
    info!("   task::Entity::insert(model).exec(conn).await?;");
    info!("");
    info!("   // READ");
    info!("   let task = task::Entity::find_by_id(id).one(conn).await?;");
    info!("");
    info!("   // UPDATE");
    info!("   let mut m: task::ActiveModel = task.into();");
    info!("   m.status = sea_orm::Set(\"completed\".to_string());");
    info!("   m.update(conn).await?;");
    info!("");
    info!("   // DELETE");
    info!("   task::Entity::delete_by_id(id).exec(conn).await?;");
}

// ============================================================================
// 实际 CRUD 函数示例（不被 main 调用，仅展示 API 模式）
// 这些函数接受 Session 参数，便于在真实环境中复用。
// ============================================================================

/// 通过原生 SQL 插入记录
#[allow(dead_code)]
async fn create_task_raw(session: &Session, url: &str) -> Result<u64, dbnexus::DbError> {
    let sql = format!(
        "INSERT INTO tasks (id, url, status) VALUES (gen_random_uuid(), '{}', 'queued')",
        url.replace('\'', "''")
    );
    let result = session.execute(&sql).await?;
    Ok(result.rows_affected())
}

/// 通过原生 SQL 更新记录状态
#[allow(dead_code)]
async fn update_task_status_raw(
    session: &Session,
    task_id: &str,
    status: &str,
) -> Result<u64, dbnexus::DbError> {
    let sql = format!(
        "UPDATE tasks SET status = '{}', updated_at = NOW() WHERE id = '{}'",
        status.replace('\'', "''"),
        task_id.replace('\'', "''")
    );
    let result = session.execute(&sql).await?;
    Ok(result.rows_affected())
}

/// 通过原生 SQL 删除记录
#[allow(dead_code)]
async fn delete_task_raw(session: &Session, task_id: &str) -> Result<u64, dbnexus::DbError> {
    let sql = format!(
        "DELETE FROM tasks WHERE id = '{}'",
        task_id.replace('\'', "''")
    );
    let result = session.execute(&sql).await?;
    Ok(result.rows_affected())
}

/// 通过 SeaORM 连接执行实体查询的模板
///
/// `session.connection()` 返回 `&DatabaseConnection`，可用于 SeaORM Entity API。
/// 以下示例需要为 `task` 表定义 SeaORM 实体（参考主项目 `src/infrastructure/database/entities/`）。
#[allow(dead_code)]
async fn read_via_sea_orm(session: &Session) -> Result<(), dbnexus::DbError> {
    let _conn = session.connection()?;
    // 实际使用（需引入实体模块）：
    //   use sea_orm::EntityTrait;
    //   let task = task::Entity::find_by_id(id).one(conn).await?;
    Ok(())
}

/// 占位：保证 Session 与 DbPool 类型被引用，避免 unused import 警告。
#[allow(dead_code)]
fn _ensure_types_used(_session: &Session, _pool: &DbPool) {}
