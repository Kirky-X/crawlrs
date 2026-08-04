-- 消息队列表（pgmq 风格）
-- Migration: add_message_queue
--
-- 基于 pgmq 设计思想，在 dbnexus 数据库层实现轻量级消息队列：
-- - 命名队列：通过 queue_name 区分不同队列
-- - 消息保证：visibility timeout 确保消息至少被消费一次
-- - 原子操作：read 使用 SELECT ... FOR UPDATE SKIP LOCKED 避免并发冲突
-- - 归档支持：archive 将消息移入归档表而非删除，支持回放
--
-- 幂等性：
--   - CREATE TABLE IF NOT EXISTS：表已存在则跳过
--   - CREATE INDEX IF NOT EXISTS：索引已存在则跳过
--   - 可重复执行无副作用

-- ==========================================================
-- 1. 消息队列表
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues (
    msg_id      BIGSERIAL    PRIMARY KEY,
    queue_name  VARCHAR(255) NOT NULL,
    message     JSONB        NOT NULL,
    read_ct     INTEGER      NOT NULL DEFAULT 0,
    enqueued_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    vt          TIMESTAMPTZ,              -- visibility timeout（NULL = 立即可读）
    archived    BOOLEAN      NOT NULL DEFAULT FALSE
);

-- 核心查询索引：按 (queue_name, vt) 加速 read 操作
-- vt NULLS FIRST 确保新消息优先被消费（vt=NULL 表示立即可用）
CREATE INDEX IF NOT EXISTS idx_mq_queue_vt
    ON message_queues (queue_name, vt NULLS FIRST)
    WHERE archived = FALSE;

-- 归档消息索引
CREATE INDEX IF NOT EXISTS idx_mq_queue_archived
    ON message_queues (queue_name, archived)
    WHERE archived = TRUE;

COMMENT ON TABLE message_queues IS 'pgmq-style message queue: named queues with visibility timeout, archive support, and atomic read via FOR UPDATE SKIP LOCKED.';
COMMENT ON COLUMN message_queues.vt IS 'Visibility timeout: message is invisible to other consumers until this time. NULL = immediately available.';
COMMENT ON COLUMN message_queues.read_ct IS 'Number of times this message has been read/consumed.';

-- ==========================================================
-- 2. 归档表（结构与主表一致）
-- ==========================================================

CREATE TABLE IF NOT EXISTS message_queues_archive (
    msg_id      BIGINT       PRIMARY KEY,
    queue_name  VARCHAR(255) NOT NULL,
    message     JSONB        NOT NULL,
    read_ct     INTEGER      NOT NULL DEFAULT 0,
    enqueued_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    vt          TIMESTAMPTZ,
    archived_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE message_queues_archive IS 'Archive table for message_queues. Messages moved here via archive() instead of being deleted.';

-- ==========================================================
-- 3. updated_at 触发器
-- ==========================================================

-- 仅在触发器不存在时创建（幂等）
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trigger_update_mq_updated_at'
    ) THEN
        CREATE TRIGGER trigger_update_mq_updated_at
            BEFORE UPDATE ON message_queues
            FOR EACH ROW
            EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;
