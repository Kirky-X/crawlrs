-- 为 acquire_next 查询添加索引 (MySQL dialect)
-- Migration: add_acquire_next_indexes
--
-- MySQL 不支持 partial index（CREATE INDEX ... WHERE），此处建普通复合索引；
-- 选择性由查询侧 WHERE status='queued' / status='active' 过滤补足。

CREATE INDEX idx_tasks_acquire_queued
    ON tasks (priority ASC, created_at ASC);

CREATE INDEX idx_tasks_acquire_stale
    ON tasks (priority ASC, created_at ASC);
