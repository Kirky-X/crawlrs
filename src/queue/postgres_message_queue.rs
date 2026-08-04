// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! PostgreSQL 消息队列存储实现
//!
//! 基于 dbnexus 的 PostgreSQL 消息队列 repository，
//! 使用 `SELECT ... FOR UPDATE SKIP LOCKED` 实现并发安全的消息读取。
//!
//! 所有 PostgreSQL 特有 SQL 语法（`BIGSERIAL`、`JSONB`、`FOR UPDATE SKIP LOCKED`、
//! `NOW()`、`INTERVAL` 等）均集中在此文件，不泄漏到上层 `DbMessageQueue`。

use crate::queue::message_queue::{Message, MessageQueueError, MessageQueueRepository};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dbnexus::DbPool;
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, FromQueryResult, JsonValue, Statement};
use std::sync::Arc;

/// Sea-ORM `DbErr` → `MessageQueueError` 转换
///
/// 仅在 PostgreSQL 存储实现中使用，保持上层 `message_queue.rs` 不依赖 sea_orm。
impl From<DbErr> for MessageQueueError {
    fn from(e: DbErr) -> Self {
        MessageQueueError::Database(e.to_string())
    }
}

/// PostgreSQL 消息队列存储实现
///
/// 使用 `message_queues` 表存储消息，通过 `SELECT ... FOR UPDATE SKIP LOCKED`
/// 实现并发安全的消息读取。
///
/// # 线程安全
///
/// 内部持有 `Arc<DbPool>`，可安全在多个 task/线程间共享。
pub struct PostgresMessageQueueRepository {
    /// 数据库连接池
    pool: Arc<DbPool>,
}

impl PostgresMessageQueueRepository {
    /// 创建新的 PostgreSQL 消息队列存储实例
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }

    /// 获取数据库连接（通过 system session）
    async fn get_conn(&self) -> Result<sea_orm::DatabaseConnection, MessageQueueError> {
        let session = self
            .pool
            .get_session("system")
            .await
            .map_err(|e| MessageQueueError::Database(e.to_string()))?;
        let conn = session
            .connection()
            .map_err(|e| MessageQueueError::Database(e.to_string()))?;
        Ok(conn.clone())
    }
}

/// 内部查询结果映射（PostgreSQL 行 → Message）
#[derive(Debug, FromQueryResult)]
struct RawMessage {
    msg_id: i64,
    queue_name: String,
    message: JsonValue,
    read_ct: i32,
    enqueued_at: DateTime<Utc>,
}

impl From<RawMessage> for Message {
    fn from(raw: RawMessage) -> Self {
        Self {
            msg_id: raw.msg_id,
            queue_name: raw.queue_name,
            message: raw.message,
            read_ct: raw.read_ct,
            enqueued_at: raw.enqueued_at,
        }
    }
}

#[async_trait]
impl MessageQueueRepository for PostgresMessageQueueRepository {
    async fn send(
        &self,
        queue_name: &str,
        message: &serde_json::Value,
    ) -> Result<i64, MessageQueueError> {
        let conn = self.get_conn().await?;

        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO message_queues (queue_name, message)
               VALUES ($1, $2) RETURNING msg_id"#,
            [queue_name.into(), message.clone().into()],
        );

        let result = conn
            .query_one_raw(stmt)
            .await?
            .ok_or_else(|| MessageQueueError::Database("INSERT RETURNING failed".into()))?;

        result
            .try_get("", "msg_id")
            .map_err(|e| MessageQueueError::Database(e.to_string()))
    }

    async fn send_batch(
        &self,
        queue_name: &str,
        messages: &[serde_json::Value],
    ) -> Result<Vec<i64>, MessageQueueError> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        let conn = self.get_conn().await?;

        // 构建多行 INSERT：VALUES ($1,$2), ($3,$4), ...
        let mut value_placeholders = Vec::with_capacity(messages.len());
        let mut values = Vec::with_capacity(messages.len() * 2);
        for (i, msg) in messages.iter().enumerate() {
            let p1 = 2 * i as i32 + 1;
            let p2 = 2 * i as i32 + 2;
            value_placeholders.push(format!("(${}, ${})", p1, p2));
            values.push(queue_name.into());
            values.push(msg.clone().into());
        }

        let sql = format!(
            "INSERT INTO message_queues (queue_name, message) VALUES {} RETURNING msg_id",
            value_placeholders.join(", ")
        );

        let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);

        let rows = conn.query_all_raw(stmt).await?;
        let mut msg_ids = Vec::with_capacity(messages.len());
        for row in rows {
            let msg_id: i64 = row
                .try_get("", "msg_id")
                .map_err(|e| MessageQueueError::Database(e.to_string()))?;
            msg_ids.push(msg_id);
        }

        Ok(msg_ids)
    }

    async fn read_batch(
        &self,
        queue_name: &str,
        vt: i32,
        batch_size: i32,
    ) -> Result<Vec<Message>, MessageQueueError> {
        let conn = self.get_conn().await?;

        // 原子读取：SELECT ... FOR UPDATE SKIP LOCKED
        // 1. 找到 queue_name 匹配、vt 已过（或 NULL）、未归档的消息
        // 2. 按 msg_id 升序（FIFO）
        // 3. FOR UPDATE SKIP LOCKED 跳过被其他消费者锁定的行
        // 4. UPDATE vt 和 read_ct 标记为已读
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            WITH available AS (
                SELECT msg_id
                FROM message_queues
                WHERE queue_name = $1
                  AND archived = FALSE
                  AND (vt IS NULL OR vt <= NOW())
                ORDER BY msg_id ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            UPDATE message_queues mq
            SET vt = NOW() + ($3 || ' seconds')::INTERVAL,
                read_ct = mq.read_ct + 1,
                updated_at = NOW()
            FROM available a
            WHERE mq.msg_id = a.msg_id
            RETURNING mq.msg_id, mq.queue_name, mq.message, mq.read_ct, mq.enqueued_at
            "#,
            [
                queue_name.into(),
                (batch_size as i64).into(),
                vt.to_string().into(),
            ],
        );

        let rows = conn.query_all_raw(stmt).await?;
        let messages: Vec<Message> = rows
            .into_iter()
            .filter_map(|row| {
                RawMessage::from_query_result(&row, "")
                    .ok()
                    .map(Message::from)
            })
            .collect();

        Ok(messages)
    }

    async fn delete(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        let conn = self.get_conn().await?;

        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM message_queues WHERE queue_name = $1 AND msg_id = $2 AND archived = FALSE",
            [queue_name.into(), msg_id.into()],
        );

        let result = conn.execute_raw(stmt).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        if msg_ids.is_empty() {
            return Ok(0);
        }

        let conn = self.get_conn().await?;

        let placeholders: Vec<String> = (2..2 + msg_ids.len() as i32)
            .map(|i| format!("${}", i))
            .collect();
        let sql = format!(
            "DELETE FROM message_queues WHERE queue_name = $1 AND msg_id IN ({}) AND archived = FALSE",
            placeholders.join(", ")
        );

        let mut values: Vec<sea_orm::Value> = vec![queue_name.into()];
        for id in msg_ids {
            values.push((*id).into());
        }

        let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
        let result = conn.execute_raw(stmt).await?;

        Ok(result.rows_affected())
    }

    async fn archive(&self, queue_name: &str, msg_id: i64) -> Result<bool, MessageQueueError> {
        let conn = self.get_conn().await?;

        // 原子归档：INSERT INTO archive + UPDATE main（标记 archived）
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            WITH moved AS (
                INSERT INTO message_queues_archive
                    (msg_id, queue_name, message, read_ct, enqueued_at, updated_at, vt)
                SELECT msg_id, queue_name, message, read_ct, enqueued_at, updated_at, vt
                FROM message_queues
                WHERE queue_name = $1 AND msg_id = $2 AND archived = FALSE
                RETURNING msg_id
            )
            UPDATE message_queues
            SET archived = TRUE, updated_at = NOW()
            WHERE queue_name = $1 AND msg_id = $2 AND archived = FALSE
              AND EXISTS (SELECT 1 FROM moved)
            "#,
            [queue_name.into(), msg_id.into()],
        );

        let result = conn.execute_raw(stmt).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn archive_batch(
        &self,
        queue_name: &str,
        msg_ids: &[i64],
    ) -> Result<u64, MessageQueueError> {
        if msg_ids.is_empty() {
            return Ok(0);
        }

        let conn = self.get_conn().await?;

        let placeholders: Vec<String> = (2..2 + msg_ids.len() as i32)
            .map(|i| format!("${}", i))
            .collect();

        let sql = format!(
            r#"
            WITH moved AS (
                INSERT INTO message_queues_archive
                    (msg_id, queue_name, message, read_ct, enqueued_at, updated_at, vt)
                SELECT msg_id, queue_name, message, read_ct, enqueued_at, updated_at, vt
                FROM message_queues
                WHERE queue_name = $1 AND msg_id IN ({}) AND archived = FALSE
                RETURNING msg_id
            )
            UPDATE message_queues
            SET archived = TRUE, updated_at = NOW()
            WHERE queue_name = $1 AND msg_id IN ({}) AND archived = FALSE
              AND EXISTS (
                  SELECT 1 FROM moved WHERE moved.msg_id = message_queues.msg_id
              )
            "#,
            placeholders.join(", "),
            placeholders.join(", "),
        );

        let mut values: Vec<sea_orm::Value> = vec![queue_name.into()];
        // msg_ids 出现两次（INSERT ... SELECT + UPDATE WHERE），需要重复参数
        for id in msg_ids {
            values.push((*id).into());
        }
        for id in msg_ids {
            values.push((*id).into());
        }

        let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
        let result = conn.execute_raw(stmt).await?;

        Ok(result.rows_affected())
    }
}
