-- Migration: 001_initial_schema
-- Description: Initial database schema for crawlrs application
-- Generated from SeaORM entity definitions in src/infrastructure/database/entities/
--
-- Type mapping (Rust -> PostgreSQL):
--   Uuid                         -> UUID
--   String                       -> TEXT
--   i32                          -> INTEGER
--   i64                          -> BIGINT
--   bool                         -> BOOLEAN
--   Json / serde_json::Value     -> JSONB
--   ChronoDateTime               -> TIMESTAMP  (NaiveDateTime, NO timezone)
--   DateTimeWithTimeZone /
--   ChronoDateTimeWithTimeZone   -> TIMESTAMPTZ
--   Option<T>                    -> nullable
-- NOTE: FK constraints are intentionally omitted — SeaORM entities manage
-- relations at the application level, and tests insert child records without
-- always creating parent records first.

-- Enable UUID extension (provides gen_random_uuid via pgcrypto in PG13+, plus uuid-ossp helpers)
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ========================================
-- 1. teams (no dependencies)
-- ========================================
CREATE TABLE IF NOT EXISTS teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    allowed_countries JSONB,
    blocked_countries JSONB,
    ip_whitelist JSONB,
    domain_blacklist JSONB,
    enable_geo_restrictions BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ========================================
-- 2. api_keys (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    key TEXT NOT NULL UNIQUE,
    key_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_api_keys_team_id ON api_keys(team_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- ========================================
-- 3. scopes (depends on api_keys)
--    Entity: auth/scope.rs  table_name = "scopes"
-- ========================================
CREATE TABLE IF NOT EXISTS scopes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID NOT NULL UNIQUE,
    read BOOLEAN NOT NULL DEFAULT TRUE,
    write BOOLEAN NOT NULL DEFAULT FALSE,
    admin BOOLEAN NOT NULL DEFAULT FALSE,
    search_limit INTEGER NOT NULL DEFAULT 100,
    scrape_limit INTEGER NOT NULL DEFAULT 50,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scopes_api_key_id ON scopes(api_key_id);

-- ========================================
-- 4. crawls (depends on teams)
--    NOTE: created_at/updated_at/completed_at use ChronoDateTime -> TIMESTAMP (not TIMESTAMPTZ)
-- ========================================
CREATE TABLE IF NOT EXISTS crawls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    name TEXT NOT NULL,
    root_url TEXT NOT NULL,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    total_tasks INTEGER NOT NULL DEFAULT 0,
    completed_tasks INTEGER NOT NULL DEFAULT 0,
    failed_tasks INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_crawls_team_id ON crawls(team_id);
CREATE INDEX IF NOT EXISTS idx_crawls_status ON crawls(status);
CREATE INDEX IF NOT EXISTS idx_crawls_team_status ON crawls(team_id, status, created_at DESC);

-- ========================================
-- 5. tasks (depends on teams, api_keys, crawls)
-- ========================================
CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type TEXT NOT NULL,
    team_id UUID NOT NULL,
    api_key_id UUID NOT NULL,
    crawl_id UUID,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    scheduled_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    lock_token UUID,
    lock_expires_at TIMESTAMPTZ,
    started_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL,
    team_id UUID NOT NULL,
    task_type TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    max_retries INTEGER NOT NULL DEFAULT 3,
    retry_count INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    scheduled_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    processed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_tasks_backlog_task_id ON tasks_backlog(task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_team_id ON tasks_backlog(team_id);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_team_status ON tasks_backlog(team_id, status, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_scheduled_at ON tasks_backlog(scheduled_at);

-- ========================================
-- 7. scrape_results (depends on tasks)
-- ========================================
CREATE TABLE IF NOT EXISTS scrape_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL,
    url TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_type TEXT NOT NULL,
    response_time_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    headers JSONB,
    meta_data JSONB,
    screenshot TEXT
);

CREATE INDEX IF NOT EXISTS idx_scrape_results_task_id ON scrape_results(task_id);
CREATE INDEX IF NOT EXISTS idx_scrape_results_created_at ON scrape_results(created_at DESC);

-- ========================================
-- 8. credits (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL UNIQUE,
    balance BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credits_team_id ON credits(team_id);

-- ========================================
-- 9. credits_transactions (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS credits_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    amount BIGINT NOT NULL,
    transaction_type TEXT NOT NULL,
    description TEXT NOT NULL,
    reference_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credits_transactions_team_id ON credits_transactions(team_id);
CREATE INDEX IF NOT EXISTS idx_credits_transactions_team_created ON credits_transactions(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_credits_transactions_reference_id ON credits_transactions(reference_id);

-- ========================================
-- 10. webhooks (depends on teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhooks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhooks_team_id ON webhooks(team_id);

-- ========================================
-- 11. webhook_events (depends on webhooks, teams)
-- ========================================
CREATE TABLE IF NOT EXISTS webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    webhook_id UUID,
    event_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    webhook_url TEXT NOT NULL,
    response_status INTEGER,
    response_body TEXT,
    error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_webhook_events_team_id ON webhook_events(team_id);
CREATE INDEX IF NOT EXISTS idx_webhook_events_webhook_id ON webhook_events(webhook_id);
CREATE INDEX IF NOT EXISTS idx_webhook_events_team_status ON webhook_events(team_id, status, next_retry_at ASC);

-- ========================================
-- 12. geo_restriction_logs (depends on teams)
--    Entity: geo_restriction_log.rs  table_name = "geo_restriction_logs"
-- ========================================
CREATE TABLE IF NOT EXISTS geo_restriction_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL,
    ip_address TEXT NOT NULL,
    country_code TEXT,
    restriction_type TEXT NOT NULL,
    url TEXT,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_team_id ON geo_restriction_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_team_created ON geo_restriction_logs(team_id, created_at DESC);

-- ========================================
-- 13. audit_logs (depends on api_keys, teams)
--    Entity: auth/audit_log.rs  table_name = "audit_logs"
-- ========================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID,
    team_id UUID,
    requested_action TEXT NOT NULL,
    decision TEXT NOT NULL,
    denial_reason TEXT,
    scope_used JSONB,
    ip_address TEXT,
    trace_id UUID,
    user_agent TEXT,
    request_path TEXT,
    request_method TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_api_key_id ON audit_logs(api_key_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_team_id ON audit_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_team_created ON audit_logs(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_trace_id ON audit_logs(trace_id);

-- ========================================
-- 14. feature_flags (no dependencies)
--    Entity: auth/feature_flag.rs  table_name = "feature_flags"
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_percentage INTEGER NOT NULL DEFAULT 0,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    started_at TIMESTAMPTZ,
    stopped_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_feature_flags_name ON feature_flags(name);
CREATE INDEX IF NOT EXISTS idx_feature_flags_enabled ON feature_flags(enabled);

-- ========================================
-- 15. feature_flag_overrides (depends on feature_flags, api_keys)
--    Entity: auth/feature_flag_override.rs  table_name = "feature_flag_overrides"
-- ========================================
CREATE TABLE IF NOT EXISTS feature_flag_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feature_flag_id UUID NOT NULL,
    api_key_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(feature_flag_id, api_key_id)
);

CREATE INDEX IF NOT EXISTS idx_feature_flag_overrides_feature_flag_id ON feature_flag_overrides(feature_flag_id);
CREATE INDEX IF NOT EXISTS idx_feature_flag_overrides_api_key_id ON feature_flag_overrides(api_key_id);

-- ========================================
-- Safe credit deduction function (used by credits_repo_impl.rs)
-- Signature: deduct_credits_safe(team_id UUID, amount BIGINT, transaction_type TEXT, description TEXT, reference_id UUID)
-- ========================================
CREATE OR REPLACE FUNCTION deduct_credits_safe(
    p_team_id UUID,
    p_amount BIGINT,
    p_transaction_type TEXT,
    p_description TEXT,
    p_reference_id UUID DEFAULT NULL
) RETURNS BIGINT AS $$
DECLARE
    v_new_balance BIGINT;
BEGIN
    UPDATE credits
    SET
        balance = balance - p_amount,
        updated_at = NOW()
    WHERE id = (SELECT id FROM credits WHERE team_id = p_team_id LIMIT 1)
    RETURNING balance INTO v_new_balance;

    IF NOT FOUND THEN
        INSERT INTO credits (id, team_id, balance, created_at, updated_at)
        VALUES (gen_random_uuid(), p_team_id, 0 - p_amount, NOW(), NOW())
        RETURNING balance INTO v_new_balance;
    END IF;

    IF v_new_balance < 0 THEN
        RAISE EXCEPTION 'Insufficient credits: balance=%, required=%', v_new_balance + p_amount, p_amount;
    END IF;

    INSERT INTO credits_transactions (id, team_id, amount, transaction_type, description, reference_id, created_at)
    VALUES (gen_random_uuid(), p_team_id, -p_amount, p_transaction_type, p_description, p_reference_id, NOW());

    RETURN v_new_balance;
END;
$$ LANGUAGE plpgsql;

-- ========================================
-- Safe credit addition function (used by credits_repo_impl.rs)
-- Signature: add_credits_safe(team_id UUID, amount BIGINT, transaction_type TEXT, description TEXT, reference_id UUID)
-- ========================================
CREATE OR REPLACE FUNCTION add_credits_safe(
    p_team_id UUID,
    p_amount BIGINT,
    p_transaction_type TEXT,
    p_description TEXT,
    p_reference_id UUID DEFAULT NULL
) RETURNS BIGINT AS $$
DECLARE
    v_new_balance BIGINT;
BEGIN
    INSERT INTO credits (id, team_id, balance, created_at, updated_at)
    VALUES (gen_random_uuid(), p_team_id, p_amount, NOW(), NOW())
    ON CONFLICT (team_id) DO UPDATE
    SET
        balance = credits.balance + p_amount,
        updated_at = NOW()
    RETURNING balance INTO v_new_balance;

    INSERT INTO credits_transactions (id, team_id, amount, transaction_type, description, reference_id, created_at)
    VALUES (gen_random_uuid(), p_team_id, p_amount, p_transaction_type, p_description, p_reference_id, NOW());

    RETURN v_new_balance;
END;
$$ LANGUAGE plpgsql;

-- ========================================
-- Function to update updated_at timestamp automatically
-- ========================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply timestamp triggers to all tables that have an updated_at column.
-- Tables WITHOUT updated_at are excluded: webhooks, geo_restriction_logs,
-- audit_logs, credits_transactions (only created_at).
DO $$
DECLARE
    t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'teams', 'api_keys', 'scopes', 'crawls', 'tasks', 'tasks_backlog',
        'credits', 'webhook_events', 'feature_flags', 'feature_flag_overrides'
    ]
    LOOP
        EXECUTE format(
            'DROP TRIGGER IF EXISTS trigger_update_%s_updated_at ON %I; '
            'CREATE TRIGGER trigger_update_%s_updated_at '
            'BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();',
            t, t, t, t
        );
    END LOOP;
END $$;
