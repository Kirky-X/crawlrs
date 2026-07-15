// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 数据库迁移示例
//!
//! 演示如何使用 dbnexus 进行数据库迁移管理。
//!
//! dbnexus 提供两种迁移方式：
//! 1. **声明式实体宏**：通过 `db_entity` 宏在代码中定义实体，
//!    dbnexus 自动生成对应的表结构（推荐，主项目使用此方式）。
//! 2. **SQL 迁移文件**：通过 `pool.run_migrations(dir)` 执行指定目录下的
//!    `.sql` 迁移文件（按版本顺序应用，记录到 `_dbnexus_migrations` 表）。
//!
//! 本示例需要运行中的 PostgreSQL 才能实际执行；
//! 默认仅打印 API 用法说明。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example migration
//! ```
//!
//! # 前置条件
//!
//! - PostgreSQL 数据库
//! - 已启用 `dbnexus-postgres` 和 `migration` 特性

use dbnexus::DbPool;
use log::info;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 dbnexus 数据库迁移示例");
    info!("=====================================\n");

    // 1. dbnexus 迁移机制说明
    info!("1️⃣  dbnexus 迁移机制");
    info!("-----------------------------");
    info!("📖 dbnexus 提供两种迁移方式：");
    info!("");
    info!("   方式一：声明式实体宏（推荐）");
    info!("     - 在代码中用 #[db_entity] 宏定义实体");
    info!("     - dbnexus 自动生成 CREATE TABLE 语句");
    info!("     - 无需手写 SQL 迁移文件");
    info!("     - 主项目 src/infrastructure/database/entities/ 中使用此方式");
    info!("");
    info!("   方式二：SQL 迁移文件");
    info!("     - 在指定目录放置 .sql 文件（V{{version}}__{{name}}.sql）");
    info!("     - 通过 pool.run_migrations(dir) 按版本顺序执行");
    info!("     - 执行历史记录在 _dbnexus_migrations 表");
    info!("");

    // 2. SQL 迁移文件示例
    info!("2️⃣  SQL 迁移文件命名规范");
    info!("-----------------------------");
    info!("📖 文件名格式：V{{version}}__{{description}}.sql");
    info!("   - version：递增整数（如 001、002）");
    info!("   - description：简短描述（用下划线分隔）");
    info!("");
    info!("📌 示例目录结构：");
    info!("   migrations/");
    info!("   ├── V001__create_tasks_table.sql");
    info!("   ├── V002__create_crawls_table.sql");
    info!("   ├── V003__add_task_priority.sql");
    info!("   └── V004__create_indices.sql");
    info!("");

    // 3. 调用 run_migrations
    info!("3️⃣  执行迁移");
    info!("-----------------------------");
    info!("📖 通过 DbPool::run_migrations 执行指定目录下的迁移文件");
    info!("");
    info!("📌 调用示例：");
    info!("   let pool = DbPool::with_config(config).await?;");
    info!("   let applied = pool.run_migrations(PathBuf::from(\"migrations\")).await?;");
    info!("   info!(\"已应用 {{}} 个迁移\", applied);");
    info!("");

    // 4. 自动迁移配置
    info!("4️⃣  自动迁移配置");
    info!("-----------------------------");
    info!("📖 在 DbConfig 中启用 auto_migrate 并设置 migrations_dir");
    info!("   连接池创建时会自动应用未执行的迁移");
    info!("");
    info!("📌 配置示例：");
    info!("   let config = DbConfig {{");
    info!("       migrations_dir: Some(PathBuf::from(\"migrations\")),");
    info!("       auto_migrate: true,");
    info!("       migration_timeout: 300,  // 5 分钟");
    info!("       ..Default::default()");
    info!("   }};");
    info!("");

    // 5. 迁移文件内容示例
    info!("5️⃣  迁移文件内容示例");
    info!("-----------------------------");
    info!("📖 V001__create_tasks_table.sql 内容示例：");
    info!("");
    info!("   CREATE TABLE IF NOT EXISTS tasks (");
    info!("       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),");
    info!("       task_type VARCHAR(50) NOT NULL,");
    info!("       status VARCHAR(50) NOT NULL DEFAULT 'queued',");
    info!("       priority INTEGER NOT NULL DEFAULT 0,");
    info!("       team_id UUID NOT NULL,");
    info!("       url TEXT NOT NULL,");
    info!("       payload JSONB NOT NULL DEFAULT '{{}}',");
    info!("       retry_count INTEGER NOT NULL DEFAULT 0,");
    info!("       max_retries INTEGER NOT NULL DEFAULT 3,");
    info!("       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),");
    info!("       updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()");
    info!("   );");
    info!("");
    info!("   CREATE INDEX idx_tasks_status ON tasks(status);");
    info!("   CREATE INDEX idx_tasks_team ON tasks(team_id);");
    info!("");

    info!("\n=====================================");
    info!("✨ dbnexus 数据库迁移示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - crawlrs 主项目使用声明式实体宏，无需 SQL 迁移文件");
    info!("   - 生产环境推荐启用 auto_migrate=true，确保部署时 schema 同步");
    info!("   - 迁移失败不会破坏已有数据（每个文件在独立事务中执行）");
}

// ============================================================================
// 实际迁移函数示例（不被 main 调用，仅展示 API 模式）
// ============================================================================

/// 执行指定目录下的迁移文件
///
/// 该函数演示如何通过 DbPool::run_migrations 应用迁移。
/// 已执行的迁移会被记录在 `_dbnexus_migrations` 表中，避免重复应用。
#[allow(dead_code)]
async fn run_migrations(pool: &DbPool, migrations_dir: &str) -> Result<u32, dbnexus::DbError> {
    let path = PathBuf::from(migrations_dir);
    let applied = pool.run_migrations(&path).await?;
    Ok(applied)
}

/// 在 DbConfig 中启用自动迁移的示例
///
/// 当 auto_migrate=true 且 migrations_dir 设置时，
/// DbPool::with_config 会在连接池初始化时自动应用未执行的迁移。
#[allow(dead_code)]
fn build_config_with_auto_migrate(url: &str) -> dbnexus::DbConfig {
    dbnexus::DbConfig {
        url: url.to_string(),
        max_connections: 10,
        min_connections: 1,
        idle_timeout: 300,
        acquire_timeout: 30_000,
        permissions_path: None,
        migrations_dir: Some(PathBuf::from("migrations")),
        auto_migrate: true,
        migration_timeout: 300,
        admin_role: "admin".to_string(),
        warmup_timeout: 30,
        warmup_retries: 3,
        cache_config: dbnexus::CacheConfig::default(),
    }
}
