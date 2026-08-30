// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 保留期清理的单批短事务删除骨架（retention-worker-hardening T003/R-retention-002）。
//!
//! 设计约束（specs/data-retention Constraints）：
//! - 每批独立短事务，事务内先 `SET LOCAL statement_timeout` 再执行参数化 `DELETE`
//! - 删除 SQL 由调用方提供，必须含 `$1`（保留天数，配合 `make_interval`）与 `$2`（limit）
//!   两个占位符；本模块不拼接任何调用方数据进 SQL 文本

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, TransactionTrait};

/// 删除一批过期行：短事务内 `SET LOCAL statement_timeout` 后执行参数化 `DELETE`。
///
/// - `delete_sql`：调用方提供的删除模板，必须含 `$1`（days）与 `$2`（limit）占位符
/// - `days`：保留天数（配合 SQL 内 `NOW() - make_interval(days => $1)`，DB 时钟）
/// - 返回本批实际删除行数
pub async fn delete_one_batch(
    conn: &DatabaseConnection,
    delete_sql: &str,
    days: i64,
    batch_size: u64,
    statement_timeout_ms: u64,
) -> anyhow::Result<u64> {
    let delete_sql = delete_sql.to_string();
    conn.transaction(|tx| {
        Box::pin(async move {
            // SET LOCAL 仅在当前事务内生效；值为经 [retention] range 校验的 u64，
            // 非外部输入，无注入面（SET LOCAL 不支持参数绑定，只能拼接）
            let set_timeout = Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                format!("SET LOCAL statement_timeout = {statement_timeout_ms}"),
            );
            tx.execute_raw(set_timeout).await?;

            let delete_stmt = Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                delete_sql,
                // make_interval(days =>) 形参为 int4，days 须绑定为 i32
                [(days as i32).into(), batch_size.into()],
            );
            let result = tx.execute_raw(delete_stmt).await?;
            Ok(result.rows_affected())
        })
    })
    .await
    .map_err(|e| match e {
        sea_orm::TransactionError::Connection(db) => {
            anyhow::anyhow!("retention batch connection error: {db}")
        }
        sea_orm::TransactionError::Transaction(t) => t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;
    use sea_orm::{ConnectionTrait, Statement};

    /// 序列化用例：两个用例并行 CREATE 同名表会竞争 pg_class 序列唯一索引。
    static RETENTION_BATCH_TEST_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
        std::sync::OnceLock::new();

    fn test_lock() -> &'static tokio::sync::Mutex<()> {
        RETENTION_BATCH_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    /// 建独立测试表并插入 `n` 行 created_at 为 40 天前的数据。
    async fn setup_test_table(conn: &sea_orm::DatabaseConnection, n: u64) {
        conn.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS retention_batch_test_rows (
                id BIGSERIAL PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await
        .expect("create table failed");
        conn.execute_unprepared("DELETE FROM retention_batch_test_rows")
            .await
            .expect("clear table failed");
        conn.execute_unprepared(&format!(
            "INSERT INTO retention_batch_test_rows (created_at)
             SELECT NOW() - INTERVAL '40 days' FROM generate_series(1, {n})"
        ))
        .await
        .expect("insert rows failed");
    }

    fn delete_sql() -> &'static str {
        "DELETE FROM retention_batch_test_rows WHERE id IN (
            SELECT id FROM retention_batch_test_rows
            WHERE created_at < NOW() - make_interval(days => $1)
            ORDER BY created_at
            LIMIT $2
        )"
    }

    async fn row_count(conn: &sea_orm::DatabaseConnection) -> u64 {
        let row = conn
            .query_one_raw(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT COUNT(*)::BIGINT AS n FROM retention_batch_test_rows",
            ))
            .await
            .expect("count failed")
            .expect("count row");
        row.try_get::<i64>("", "n").expect("get count") as u64
    }

    /// R-retention-002：单批最多删 `batch_size` 行，返回行数准确。
    #[tokio::test]
    async fn delete_one_batch_deletes_at_most_batch_size_rows() {
        let _guard = test_lock().lock().await;
        let pool = create_test_db_pool();
        let session = pool.get_session("admin").await.expect("session");
        let conn = session.connection().expect("connection");

        setup_test_table(conn, 7).await; // > 2 * batch_size(3)

        let deleted = delete_one_batch(conn, delete_sql(), 30, 3, 60_000)
            .await
            .expect("batch 1");
        assert_eq!(
            deleted, 3,
            "first batch must delete exactly batch_size rows"
        );
        assert_eq!(row_count(conn).await, 4);

        let deleted = delete_one_batch(conn, delete_sql(), 30, 3, 60_000)
            .await
            .expect("batch 2");
        assert_eq!(deleted, 3);
        assert_eq!(row_count(conn).await, 1);

        // 尾批：只剩 1 行过期，返回 1 而非 batch_size
        let deleted = delete_one_batch(conn, delete_sql(), 30, 3, 60_000)
            .await
            .expect("tail batch");
        assert_eq!(deleted, 1);

        // 空批：无过期行返回 0
        let deleted = delete_one_batch(conn, delete_sql(), 30, 3, 60_000)
            .await
            .expect("empty batch");
        assert_eq!(deleted, 0);
    }

    /// R-retention-002：statement_timeout 在事务内生效（SET LOCAL 不报错且删除正常）。
    #[tokio::test]
    async fn delete_one_batch_applies_statement_timeout() {
        let _guard = test_lock().lock().await;
        let pool = create_test_db_pool();
        let session = pool.get_session("admin").await.expect("session");
        let conn = session.connection().expect("connection");

        setup_test_table(conn, 2).await;

        let deleted = delete_one_batch(conn, delete_sql(), 30, 5000, 1_000)
            .await
            .expect("batch");
        assert_eq!(deleted, 2);
    }
}
