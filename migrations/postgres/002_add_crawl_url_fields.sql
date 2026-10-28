-- 添加 name, root_url, url 列到 crawls 表
-- Migration: add_crawl_url_fields

ALTER TABLE crawls ADD COLUMN IF NOT EXISTS name VARCHAR(255) DEFAULT 'Untitled Crawl';
ALTER TABLE crawls ADD COLUMN IF NOT EXISTS root_url TEXT;
ALTER TABLE crawls ADD COLUMN IF NOT EXISTS url TEXT;
