// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 数据库事务处理示例
//!
//! 演示如何使用 dbnexus Session 进行事务管理，包括：
//! - 开启事务（`begin_transaction`）
//! - 提交事务（`commit`）
//! - 回滚事务（`rollback`）
//! - 事务中的错误处理
//! - 保存点（Savepoint）用法
//!
//! 本示例需要运行中的 PostgreSQL 才能实际执行；
//! 默认仅打印 API 用法说明，事务代码以函数形式展示。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example transaction
//! ```
//!
//! # 前置条件
//!
//! - PostgreSQL 数据库
//! - 已启用 `dbnexus-postgres` 特性

use dbnexus::{DbPool, Session};
use log::info;

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 dbnexus 事务处理示例");
    info!("=====================================\n");

    // 1. 事务基本流程
    info!("1️⃣  事务基本流程");
    info!("-----------------------------");
    info!("📖 dbnexus Session 提供三个核心事务方法：");
    info!("   - begin_transaction() : 开启事务");
    info!("   - commit()             : 提交事务");
    info!("   - rollback()           : 回滚事务");
    info!("");
    info!("📌 典型调用序列：");
    info!("   let session = pool.get_session(\"admin\").await?;");
    info!("   session.begin_transaction().await?;");
    info!("   // ... 执行多条 SQL ...");
    info!("   session.commit().await?;     // 或 session.rollback().await?;");
    info!("");

    // 2. 事务隔离级别说明
    info!("2️⃣  事务隔离与角色");
    info!("-----------------------------");
    info!("📖 dbnexus 的事务基于角色权限：");
    info!("   - admin 角色：可执行所有操作，包括 DDL");
    info!("   - 非 admin 角色：受 permissions.yaml 限制");
    info!("");
    info!("📌 选择角色的建议：");
    info!("   - 转账类业务：使用 admin 角色确保完整权限");
    info!("   - 业务数据写入：使用受限角色遵循最小权限原则");
    info!("");

    // 3. 错误处理模式
    info!("3️⃣  事务中的错误处理");
    info!("-----------------------------");
    info!("📖 推荐使用 `?` 传播错误，并在外层统一回滚");
    info!("   Session 在 Drop 时不会自动回滚，需显式调用 rollback()");
    info!("");
    info!("📌 错误处理代码片段：");
    info!("   match perform_transfer(&session, from, to, amount).await {{");
    info!("       Ok(_) => session.commit().await?,");
    info!("       Err(e) => {{");
    info!("           let _ = session.rollback().await;");
    info!("           return Err(e);");
    info!("       }}");
    info!("   }}");
    info!("");

    // 4. 保存点示例
    info!("4️⃣  保存点（Savepoint）");
    info!("-----------------------------");
    info!("📖 通过 execute_raw_ddl 执行 SAVEPOINT/RELEASE/ROLLBACK TO 语句");
    info!("   保存点允许事务内部的部分回滚，常用于批量处理中的容错");
    info!("");
    info!("📌 保存点代码片段：");
    info!("   session.execute_raw_ddl(\"SAVEPOINT sp1\").await?;");
    info!("   // ... 风险操作 ...");
    info!("   if risk_failed {{");
    info!("       session.execute_raw_ddl(\"ROLLBACK TO SAVEPOINT sp1\").await?;");
    info!("   }} else {{");
    info!("       session.execute_raw_ddl(\"RELEASE SAVEPOINT sp1\").await?;");
    info!("   }}");
    info!("");

    info!("\n=====================================");
    info!("✨ dbnexus 事务处理示例完成");
    info!("");
    info!("💡 提示:");
    info!("   - crawlrs 在 src/infrastructure/database/transaction.rs 中提供 TransactionManager");
    info!("   - 生产环境推荐使用 TransactionManager 而非直接调用 Session 方法");
    info!("   - 事务期间避免长时间持有锁，防止阻塞其他请求");
}

// ============================================================================
// 实际事务函数示例（不被 main 调用，仅展示 API 模式）
// ============================================================================

/// 转账示例：在事务中执行多个操作，失败时回滚
///
/// 该函数展示了典型的事务模式：
/// 1. 开启事务
/// 2. 执行多条 SQL
/// 3. 任一步骤失败则回滚
/// 4. 全部成功才提交
#[allow(dead_code)]
async fn perform_transfer(
    session: &Session,
    from_account: &str,
    to_account: &str,
    amount: u64,
) -> Result<(), dbnexus::DbError> {
    session.begin_transaction().await?;

    let debit_sql = format!(
        "UPDATE accounts SET balance = balance - {} WHERE id = '{}'",
        amount,
        from_account.replace('\'', "''")
    );
    session.execute(&debit_sql).await?;

    let credit_sql = format!(
        "UPDATE accounts SET balance = balance + {} WHERE id = '{}'",
        amount,
        to_account.replace('\'', "''")
    );
    session.execute(&credit_sql).await?;

    let log_sql = format!(
        "INSERT INTO transfer_log (from_id, to_id, amount) VALUES ('{}', '{}', {})",
        from_account.replace('\'', "''"),
        to_account.replace('\'', "''"),
        amount
    );
    session.execute(&log_sql).await?;

    session.commit().await?;
    Ok(())
}

/// 安全执行包装器：自动回滚失败的事务
#[allow(dead_code)]
async fn with_rollback_on_error<F, T>(session: &Session, f: F) -> Result<T, dbnexus::DbError>
where
    F: std::future::Future<Output = Result<T, dbnexus::DbError>>,
{
    session.begin_transaction().await?;
    match f.await {
        Ok(value) => {
            session.commit().await?;
            Ok(value)
        }
        Err(e) => {
            let _ = session.rollback().await;
            Err(e)
        }
    }
}

/// 使用保存点的批量插入示例
///
/// 该函数演示如何在批量插入中跳过失败项而不回滚整个事务。
#[allow(dead_code)]
async fn batch_insert_with_savepoint(
    session: &Session,
    items: &[String],
) -> Result<usize, dbnexus::DbError> {
    session.begin_transaction().await?;
    let mut success_count = 0usize;

    for (idx, item) in items.iter().enumerate() {
        let sp = format!("sp_{}", idx);
        session
            .execute_raw_ddl(&format!("SAVEPOINT {}", sp))
            .await?;

        let sql = format!(
            "INSERT INTO items (name) VALUES ('{}')",
            item.replace('\'', "''")
        );
        match session.execute(&sql).await {
            Ok(_) => {
                session
                    .execute_raw_ddl(&format!("RELEASE SAVEPOINT {}", sp))
                    .await?;
                success_count += 1;
            }
            Err(_) => {
                // 跳过失败项，回滚到保存点
                session
                    .execute_raw_ddl(&format!("ROLLBACK TO SAVEPOINT {}", sp))
                    .await?;
            }
        }
    }

    session.commit().await?;
    Ok(success_count)
}

/// 占位：保证 Session 与 DbPool 类型被引用
#[allow(dead_code)]
fn _ensure_types_used(_session: &Session, _pool: &DbPool) {}
