-- Migration: 006_add_message_queue (SQLite dialect)
-- pgmq-style message queue; dialect mapping as in 001 (BIGSERIAL ->
-- INTEGER PRIMARY KEY AUTOINCREMENT, JSONB -> TEXT, TIMESTAMPTZ -> TEXT,
-- NULLS FIRST modifier dropped — SQLite orders NULLs first by default in ASC).

-- ==========================================================
-- 1. 消息队列表
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues (
    msg_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_name  VARCHAR(255) NOT NULL,
    message     TEXT NOT NULL,
    read_ct     INTEGER NOT NULL DEFAULT 0,
    enqueued_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    vt          TEXT,
    archived    BOOLEAN NOT NULL DEFAULT FALSE
);

-- 核心查询索引：按 (queue_name, vt) 加速 read 操作（partial index，SQLite 支持）
CREATE INDEX IF NOT EXISTS idx_mq_queue_vt
    ON message_queues (queue_name, vt)
    WHERE archived = FALSE;

-- 归档消息索引
CREATE INDEX IF NOT EXISTS idx_mq_queue_archived
    ON message_queues (queue_name, archived)
    WHERE archived = TRUE;

-- ==========================================================
-- 2. 归档表（结构与主表一致）
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues_archive (
    msg_id      INTEGER PRIMARY KEY,
    queue_name  VARCHAR(255) NOT NULL,
    message     TEXT NOT NULL,
    read_ct     INTEGER NOT NULL DEFAULT 0,
    enqueued_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    vt          TEXT,
    archived_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
