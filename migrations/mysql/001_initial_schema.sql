-- Migration: 001_initial_schema (MySQL dialect)
-- Description: Initial database schema for crawlrs application.
--
-- Dialect mapping from migrations/postgres/001_initial_schema.sql:
--   UUID      -> CHAR(36)      (hyphenated UUID strings, generated app-side)
--   JSONB     -> JSON          (expression default, MySQL 8.0.13+)
--   TIMESTAMPTZ / TIMESTAMP -> DATETIME (DEFAULT CURRENT_TIMESTAMP)
--   DOUBLE PRECISION -> DOUBLE
--   DEFAULT gen_random_uuid() -> omitted: ids are always supplied by the
--       application (SeaORM ActiveModel); MySQL has no server-side UUID.
--   PG functions (deduct_credits_safe/add_credits_safe/update_updated_at_column),
--   triggers, COMMENT ON and DO blocks are PostgreSQL-only and intentionally
--   omitted; the cross-dialect consistency test only compares tables/columns.
--   Partial indexes (CREATE INDEX ... WHERE) are a PostgreSQL/SQLite feature;
--   MySQL gets plain composite indexes (see 003/006/007).
--   Reserved-word columns (api_keys.`key`, scopes.`read`/`write`) are quoted.
--   CREATE INDEX does not support IF NOT EXISTS on MySQL.
-- NOTE: FK constraints are intentionally omitted (parity with the PG dialect).

-- ========================================
-- 1. teams (no dependencies)
-- ========================================
CREATE TABLE IF NOT EXISTS teams (
    id CHAR(36) PRIMARY KEY,
    name TEXT NOT NULL,
    allowed_countries JSON,
    blocked_countries JSON,
    ip_whitelist JSON,
    domain_blacklist JSON,
    enable_geo_restrictions BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ========================================
-- 2. api_keys (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS api_keys (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    `key` VARCHAR(255) NOT NULL UNIQUE,
    key_hash VARCHAR(64),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME
);

CREATE INDEX idx_api_keys_team_id ON api_keys(team_id);
CREATE INDEX idx_api_keys_key_hash ON api_keys(key_hash);

-- ========================================
-- 3. scopes (depends on api_keys)
-- ========================================
CREATE TABLE IF NOT EXISTS scopes (
    id CHAR(36) PRIMARY KEY,
    api_key_id CHAR(36) NOT NULL UNIQUE,
    `read` BOOLEAN NOT NULL DEFAULT TRUE,
    `write` BOOLEAN NOT NULL DEFAULT FALSE,
    `admin` BOOLEAN NOT NULL DEFAULT FALSE,
    search_limit INTEGER NOT NULL DEFAULT 100,
    scrape_limit INTEGER NOT NULL DEFAULT 50,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_scopes_api_key_id ON scopes(api_key_id);

-- ========================================
-- 4. crawls (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS crawls (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    name TEXT NOT NULL,
    root_url TEXT NOT NULL,
    url TEXT NOT NULL,
    status VARCHAR(32) NOT NULL,
    config JSON NOT NULL DEFAULT ('{}'),
    total_tasks INTEGER NOT NULL DEFAULT 0,
    completed_tasks INTEGER NOT NULL DEFAULT 0,
    failed_tasks INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME
);

CREATE INDEX idx_crawls_team_id ON crawls(team_id);
CREATE INDEX idx_crawls_status ON crawls(status);
CREATE INDEX idx_crawls_team_status ON crawls(team_id, status, created_at DESC);

-- ========================================
-- 5. tasks (depends on teams, api_keys, crawls)
-- ========================================
CREATE TABLE IF NOT EXISTS tasks (
    id CHAR(36) PRIMARY KEY,
    task_type TEXT NOT NULL,
    team_id CHAR(36) NOT NULL,
    api_key_id CHAR(36) NOT NULL,
    crawl_id CHAR(36),
    url TEXT NOT NULL,
    status VARCHAR(32) NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload JSON NOT NULL DEFAULT ('{}'),
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    scheduled_at DATETIME,
    expires_at DATETIME,
    completed_at DATETIME,
    lock_token CHAR(36),
    lock_expires_at DATETIME,
    started_at DATETIME,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_tasks_team_id ON tasks(team_id);
CREATE INDEX idx_tasks_api_key_id ON tasks(api_key_id);
CREATE INDEX idx_tasks_crawl_id ON tasks(crawl_id);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_created_at ON tasks(created_at);
CREATE INDEX idx_tasks_url ON tasks(url(768));
CREATE INDEX idx_tasks_scheduled_at ON tasks(scheduled_at);
CREATE INDEX idx_tasks_lock_expires_at ON tasks(lock_expires_at);
CREATE INDEX idx_tasks_team_status_priority ON tasks(team_id, status, priority DESC, created_at ASC);

-- ========================================
-- 6. tasks_backlog (depends on teams, tasks)
-- ========================================
CREATE TABLE IF NOT EXISTS tasks_backlog (
    id CHAR(36) PRIMARY KEY,
    task_id CHAR(36) NOT NULL,
    team_id CHAR(36) NOT NULL,
    task_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload JSON NOT NULL DEFAULT ('{}'),
    max_retries INTEGER NOT NULL DEFAULT 3,
    retry_count INTEGER NOT NULL DEFAULT 0,
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    scheduled_at DATETIME,
    expires_at DATETIME,
    processed_at DATETIME
);

CREATE INDEX idx_tasks_backlog_task_id ON tasks_backlog(task_id);
CREATE INDEX idx_tasks_backlog_team_id ON tasks_backlog(team_id);
CREATE INDEX idx_tasks_backlog_team_status ON tasks_backlog(team_id, status, priority DESC, created_at ASC);
CREATE INDEX idx_tasks_backlog_scheduled_at ON tasks_backlog(scheduled_at);

-- ========================================
-- 7. scrape_results (depends on tasks)
-- ========================================
CREATE TABLE IF NOT EXISTS scrape_results (
    id CHAR(36) PRIMARY KEY,
    task_id CHAR(36) NOT NULL,
    url TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_type TEXT NOT NULL,
    response_time_ms BIGINT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    headers JSON,
    meta_data JSON,
    screenshot TEXT
);

CREATE INDEX idx_scrape_results_task_id ON scrape_results(task_id);
CREATE INDEX idx_scrape_results_created_at ON scrape_results(created_at DESC);

-- ========================================
-- 8. credits (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL UNIQUE,
    balance BIGINT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_credits_team_id ON credits(team_id);

-- ========================================
-- 9. credits_transactions (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits_transactions (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    amount BIGINT NOT NULL,
    transaction_type TEXT NOT NULL,
    description TEXT NOT NULL,
    reference_id CHAR(36),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_credits_transactions_team_id ON credits_transactions(team_id);
CREATE INDEX idx_credits_transactions_team_created ON credits_transactions(team_id, created_at DESC);
CREATE INDEX idx_credits_transactions_reference_id ON credits_transactions(reference_id);

-- ========================================
-- 10. webhooks (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhooks (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    url TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_webhooks_team_id ON webhooks(team_id);

-- ========================================
-- 11. webhook_events (depends on webhooks, teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhook_events (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    webhook_id CHAR(36),
    event_type TEXT NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    payload JSON NOT NULL DEFAULT ('{}'),
    webhook_url TEXT NOT NULL,
    response_status INTEGER,
    response_body TEXT,
    error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    next_retry_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    delivered_at DATETIME
);

CREATE INDEX idx_webhook_events_team_id ON webhook_events(team_id);
CREATE INDEX idx_webhook_events_webhook_id ON webhook_events(webhook_id);
CREATE INDEX idx_webhook_events_team_status ON webhook_events(team_id, status, next_retry_at ASC);

-- ========================================
-- 12. geo_restriction_logs (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS geo_restriction_logs (
    id CHAR(36) PRIMARY KEY,
    team_id CHAR(36) NOT NULL,
    ip_address TEXT NOT NULL,
    country_code TEXT,
    restriction_type TEXT NOT NULL,
    url TEXT,
    reason TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_geo_restriction_logs_team_id ON geo_restriction_logs(team_id);
CREATE INDEX idx_geo_restriction_logs_team_created ON geo_restriction_logs(team_id, created_at DESC);

-- ========================================
-- 13. audit_logs (depends on api_keys, teams)
-- ========================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id CHAR(36) PRIMARY KEY,
    api_key_id CHAR(36),
    team_id CHAR(36),
    requested_action TEXT NOT NULL,
    decision TEXT NOT NULL,
    denial_reason TEXT,
    scope_used JSON,
    ip_address TEXT,
    trace_id CHAR(36),
    user_agent TEXT,
    request_path TEXT,
    request_method TEXT,
    metadata JSON NOT NULL DEFAULT ('{}'),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_audit_logs_api_key_id ON audit_logs(api_key_id);
CREATE INDEX idx_audit_logs_team_id ON audit_logs(team_id);
CREATE INDEX idx_audit_logs_team_created ON audit_logs(team_id, created_at DESC);
CREATE INDEX idx_audit_logs_trace_id ON audit_logs(trace_id);

-- ========================================
-- 14. feature_flags (no dependencies)
-- (dropped by 004_drop_feature_flags; kept for file-by-file parity)
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flags (
    id CHAR(36) PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_percentage INTEGER NOT NULL DEFAULT 0,
    metadata JSON NOT NULL DEFAULT ('{}'),
    started_at DATETIME,
    stopped_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_feature_flags_name ON feature_flags(name);
CREATE INDEX idx_feature_flags_enabled ON feature_flags(enabled);

-- ========================================
-- 15. feature_flag_overrides (depends on feature_flags, api_keys)
-- (dropped by 004_drop_feature_flags; kept for file-by-file parity)
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flag_overrides (
    id CHAR(36) PRIMARY KEY,
    feature_flag_id CHAR(36) NOT NULL,
    api_key_id CHAR(36) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(feature_flag_id, api_key_id)
);

CREATE INDEX idx_feature_flag_overrides_feature_flag_id ON feature_flag_overrides(feature_flag_id);
CREATE INDEX idx_feature_flag_overrides_api_key_id ON feature_flag_overrides(api_key_id);
