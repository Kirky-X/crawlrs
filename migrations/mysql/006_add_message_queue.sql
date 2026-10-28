-- 消息队列表（pgmq 风格）(MySQL dialect)
-- Migration: add_message_queue
--
-- BIGSERIAL -> BIGINT AUTO_INCREMENT PRIMARY KEY；JSONB -> JSON；
-- TIMESTAMPTZ -> DATETIME；partial index 与 NULLS FIRST 为 PG 专属，
-- MySQL 用普通复合索引（vt 为 NULL 的行由查询侧 ORDER BY vt IS NULL, vt 补足）。

-- ==========================================================
-- 1. 消息队列表
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues (
    msg_id      BIGINT AUTO_INCREMENT PRIMARY KEY,
    queue_name  VARCHAR(255) NOT NULL,
    message     JSON NOT NULL,
    read_ct     INTEGER NOT NULL DEFAULT 0,
    enqueued_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    vt          DATETIME,
    archived    BOOLEAN NOT NULL DEFAULT FALSE
);

-- 核心查询索引：按 (queue_name, vt) 加速 read 操作
CREATE INDEX idx_mq_queue_vt
    ON message_queues (queue_name, vt);

-- 归档消息索引
CREATE INDEX idx_mq_queue_archived
    ON message_queues (queue_name, archived);

-- ==========================================================
-- 2. 归档表（结构与主表一致）
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues_archive (
    msg_id      BIGINT PRIMARY KEY,
    queue_name  VARCHAR(255) NOT NULL,
    message     JSON NOT NULL,
    read_ct     INTEGER NOT NULL DEFAULT 0,
    enqueued_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    vt          DATETIME,
    archived_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
