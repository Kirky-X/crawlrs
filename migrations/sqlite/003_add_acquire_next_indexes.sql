-- 为 acquire_next 查询添加 partial indexes (SQLite dialect)
-- Migration: add_acquire_next_indexes
--
-- SQLite supports partial indexes; syntax matches the PG original.

CREATE INDEX IF NOT EXISTS idx_tasks_acquire_queued
    ON tasks (priority ASC, created_at ASC)
    WHERE status = 'queued';

CREATE INDEX IF NOT EXISTS idx_tasks_acquire_stale
    ON tasks (priority ASC, created_at ASC)
    WHERE status = 'active' AND lock_expires_at IS NOT NULL;
