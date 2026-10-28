-- 添加 name, root_url, url 列到 crawls 表 (SQLite dialect)
-- Migration: add_crawl_url_fields
--
-- SQLite has no ADD COLUMN IF NOT EXISTS; columns already exist in
-- 001_initial_schema (parity with the PG dialect where 002 is an idempotent
-- belt-and-suspenders for pre-002 databases). For fresh SQLite databases
-- created by 001 this file is a no-op statement-wise and therefore omitted;
-- the dialect consistency test tracks columns via 001 for all dialects.

-- (no statements: columns are part of 001_initial_schema.sql on SQLite)
