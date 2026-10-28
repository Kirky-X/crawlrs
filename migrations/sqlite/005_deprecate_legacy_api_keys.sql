-- 标记旧 api_keys/scopes 表为弃用（保留数据只读）(SQLite dialect)
-- Migration: deprecate_legacy_api_keys
--
-- PG 的 COMMENT ON / DO $$ NOTICE / trigger DISABLE-ENABLE 包裹均为
-- PostgreSQL 专属，SQLite 不适用；此处只保留等效的 schema 变更：
--   - api_keys/scopes 各加 deprecated_at 标记列（SQLite 无 ADD COLUMN
--     IF NOT EXISTS，重复执行会报 duplicate column —— SQLite 侧迁移由
--     dbnexus auto-migrate 按序执行一次，不做 CI 幂等循环）
--   - 历史行填充由应用层或运维 SQL 完成（触发器在 SQLite 方言中不存在）

ALTER TABLE api_keys ADD COLUMN deprecated_at TEXT;
ALTER TABLE scopes ADD COLUMN deprecated_at TEXT;
