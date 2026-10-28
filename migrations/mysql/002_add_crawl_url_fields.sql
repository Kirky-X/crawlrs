-- 添加 name, root_url, url 列到 crawls 表 (MySQL dialect)
-- Migration: add_crawl_url_fields
--
-- Columns already exist in 001_initial_schema (parity with the PG dialect
-- where 002 is an idempotent belt-and-suspenders for pre-002 databases).
-- MySQL has no ADD COLUMN IF NOT EXISTS; for fresh MySQL databases created
-- by 001 this file is a no-op statement-wise and therefore omitted.

-- (no statements: columns are part of 001_initial_schema.sql on MySQL)
