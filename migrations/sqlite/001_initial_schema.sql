-- Migration: 001_initial_schema (SQLite dialect)
-- Description: Initial database schema for crawlrs application.
--
-- Dialect mapping from migrations/postgres/001_initial_schema.sql:
--   UUID      -> TEXT          (hyphenated UUID strings, generated app-side)
--   JSONB     -> TEXT          (JSON stored as text)
--   TIMESTAMPTZ / TIMESTAMP -> TEXT (ISO-8601 UTC strings)
--   DEFAULT NOW()      -> DEFAULT CURRENT_TIMESTAMP
--   DEFAULT gen_random_uuid() -> omitted: ids are always supplied by the
--       application (SeaORM ActiveModel), SQLite has no server-side UUID.
--   PG functions (deduct_credits_safe/add_credits_safe/update_updated_at_column),
--   triggers, COMMENT ON and DO blocks are PostgreSQL-only and intentionally
--   omitted; the cross-dialect consistency test only compares tables/columns.
-- NOTE: FK constraints are intentionally omitted (parity with the PG dialect).

-- ========================================
-- 1. teams (no dependencies)
-- ========================================
CREATE TABLE IF NOT EXISTS teams (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    allowed_countries TEXT,
    blocked_countries TEXT,
    ip_whitelist TEXT,
    domain_blacklist TEXT,
    enable_geo_restrictions BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ========================================
-- 2. api_keys (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    key TEXT NOT NULL UNIQUE,
    key_hash TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_api_keys_team_id ON api_keys(team_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- ========================================
-- 3. scopes (depends on api_keys)
-- ========================================
CREATE TABLE IF NOT EXISTS scopes (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL UNIQUE,
    read BOOLEAN NOT NULL DEFAULT TRUE,
    write BOOLEAN NOT NULL DEFAULT FALSE,
    admin BOOLEAN NOT NULL DEFAULT FALSE,
    search_limit INTEGER NOT NULL DEFAULT 100,
    scrape_limit INTEGER NOT NULL DEFAULT 50,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_scopes_api_key_id ON scopes(api_key_id);

-- ========================================
-- 4. crawls (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS crawls (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    name TEXT NOT NULL,
    root_url TEXT NOT NULL,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    total_tasks INTEGER NOT NULL DEFAULT 0,
    completed_tasks INTEGER NOT NULL DEFAULT 0,
    failed_tasks INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_crawls_team_id ON crawls(team_id);
CREATE INDEX IF NOT EXISTS idx_crawls_status ON crawls(status);
CREATE INDEX IF NOT EXISTS idx_crawls_team_status ON crawls(team_id, status, created_at DESC);

-- ========================================
-- 5. tasks (depends on teams, api_keys, crawls)
-- ========================================
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    task_type TEXT NOT NULL,
    team_id TEXT NOT NULL,
    api_key_id TEXT NOT NULL,
    crawl_id TEXT,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL DEFAULT '{}',
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    scheduled_at TEXT,
    expires_at TEXT,
    completed_at TEXT,
    lock_token TEXT,
    lock_expires_at TEXT,
    started_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_tasks_team_id ON tasks(team_id);
CREATE INDEX IF NOT EXISTS idx_tasks_api_key_id ON tasks(api_key_id);
CREATE INDEX IF NOT EXISTS idx_tasks_crawl_id ON tasks(crawl_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_url ON tasks(url);
CREATE INDEX IF NOT EXISTS idx_tasks_scheduled_at ON tasks(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_tasks_lock_expires_at ON tasks(lock_expires_at);
CREATE INDEX IF NOT EXISTS idx_tasks_team_status_priority ON tasks(team_id, status, priority DESC, created_at ASC);

-- ========================================
-- 6. tasks_backlog (depends on teams, tasks)
-- ========================================
CREATE TABLE IF NOT EXISTS tasks_backlog (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    task_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL DEFAULT '{}',
    max_retries INTEGER NOT NULL DEFAULT 3,
    retry_count INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    scheduled_at TEXT,
    expires_at TEXT,
    processed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_tasks_backlog_task_id ON tasks_backlog(task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_team_id ON tasks_backlog(team_id);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_team_status ON tasks_backlog(team_id, status, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_scheduled_at ON tasks_backlog(scheduled_at);

-- ========================================
-- 7. scrape_results (depends on tasks)
-- ========================================
CREATE TABLE IF NOT EXISTS scrape_results (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    url TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_type TEXT NOT NULL,
    response_time_ms BIGINT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    headers TEXT,
    meta_data TEXT,
    screenshot TEXT
);

CREATE INDEX IF NOT EXISTS idx_scrape_results_task_id ON scrape_results(task_id);
CREATE INDEX IF NOT EXISTS idx_scrape_results_created_at ON scrape_results(created_at DESC);

-- ========================================
-- 8. credits (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL UNIQUE,
    balance BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_credits_team_id ON credits(team_id);

-- ========================================
-- 9. credits_transactions (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits_transactions (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    amount BIGINT NOT NULL,
    transaction_type TEXT NOT NULL,
    description TEXT NOT NULL,
    reference_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_credits_transactions_team_id ON credits_transactions(team_id);
CREATE INDEX IF NOT EXISTS idx_credits_transactions_team_created ON credits_transactions(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_credits_transactions_reference_id ON credits_transactions(reference_id);

-- ========================================
-- 10. webhooks (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhooks (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_webhooks_team_id ON webhooks(team_id);

-- ========================================
-- 11. webhook_events (depends on webhooks, teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhook_events (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    webhook_id TEXT,
    event_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload TEXT NOT NULL DEFAULT '{}',
    webhook_url TEXT NOT NULL,
    response_status INTEGER,
    response_body TEXT,
    error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    next_retry_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    delivered_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_webhook_events_team_id ON webhook_events(team_id);
CREATE INDEX IF NOT EXISTS idx_webhook_events_webhook_id ON webhook_events(webhook_id);
CREATE INDEX IF NOT EXISTS idx_webhook_events_team_status ON webhook_events(team_id, status, next_retry_at ASC);

-- ========================================
-- 12. geo_restriction_logs (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS geo_restriction_logs (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    country_code TEXT,
    restriction_type TEXT NOT NULL,
    url TEXT,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_team_id ON geo_restriction_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_team_created ON geo_restriction_logs(team_id, created_at DESC);

-- ========================================
-- 13. audit_logs (depends on api_keys, teams)
-- ========================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT PRIMARY KEY,
    api_key_id TEXT,
    team_id TEXT,
    requested_action TEXT NOT NULL,
    decision TEXT NOT NULL,
    denial_reason TEXT,
    scope_used TEXT,
    ip_address TEXT,
    trace_id TEXT,
    user_agent TEXT,
    request_path TEXT,
    request_method TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_api_key_id ON audit_logs(api_key_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_team_id ON audit_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_team_created ON audit_logs(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_trace_id ON audit_logs(trace_id);

-- ========================================
-- 14. feature_flags (no dependencies)
-- (dropped by 004_drop_feature_flags; kept for file-by-file parity)
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_percentage INTEGER NOT NULL DEFAULT 0,
    metadata TEXT NOT NULL DEFAULT '{}',
    started_at TEXT,
    stopped_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_feature_flags_name ON feature_flags(name);
CREATE INDEX IF NOT EXISTS idx_feature_flags_enabled ON feature_flags(enabled);

-- ========================================
-- 15. feature_flag_overrides (depends on feature_flags, api_keys)
-- (dropped by 004_drop_feature_flags; kept for file-by-file parity)
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flag_overrides (
    id TEXT PRIMARY KEY,
    feature_flag_id TEXT NOT NULL,
    api_key_id TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(feature_flag_id, api_key_id)
);

CREATE INDEX IF NOT EXISTS idx_feature_flag_overrides_feature_flag_id ON feature_flag_overrides(feature_flag_id);
CREATE INDEX IF NOT EXISTS idx_feature_flag_overrides_api_key_id ON feature_flag_overrides(api_key_id);
