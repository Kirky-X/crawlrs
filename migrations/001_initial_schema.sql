-- Migration: 001_initial_schema
-- Created at: 2025-01-21
-- Description: Initial database schema for crawlrs application

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ========================================
-- 1. Teams table
-- ========================================
CREATE TABLE IF NOT EXISTS teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    allowed_countries JSONB,
    blocked_countries JSONB,
    ip_whitelist JSONB,
    domain_blacklist JSONB,
    enable_geo_restrictions BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ========================================
-- 2. API Keys table
-- ========================================
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID NOT NULL DEFAULT gen_random_uuid() PRIMARY KEY,
    key VARCHAR(255) NOT NULL UNIQUE,
    key_hash VARCHAR(255),
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    feature_flags JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_api_key_team ON api_keys(team_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- ========================================
-- 3. Tasks table
-- ========================================
CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type VARCHAR(50) NOT NULL,
    url TEXT NOT NULL,
    status VARCHAR(50) NOT NULL,
    result_id UUID,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    priority INTEGER DEFAULT 0,
    metadata JSONB,
    error_message TEXT,
    payload JSONB,
    retry_count INTEGER DEFAULT 0,
    max_retries INTEGER DEFAULT 3,
    attempt_count INTEGER DEFAULT 0,
    scheduled_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    started_at TIMESTAMPTZ,
    lock_token UUID,
    lock_expires_at TIMESTAMPTZ,
    crawl_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tasks_team ON tasks(team_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_url ON tasks(url);
CREATE INDEX IF NOT EXISTS idx_tasks_scheduled_at ON tasks(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_tasks_lock_expires ON tasks(lock_expires_at);
CREATE INDEX IF NOT EXISTS idx_tasks_crawl_id ON tasks(crawl_id);
CREATE INDEX IF NOT EXISTS idx_tasks_team_status_priority ON tasks(team_id, status, priority DESC, created_at ASC);

-- ========================================
-- 4. Scrape Results table
-- ========================================
CREATE TABLE IF NOT EXISTS scrape_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    status_code INTEGER,
    content TEXT,
    content_type VARCHAR(255),
    headers JSONB,
    response_time_ms INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_scrape_results_task_id ON scrape_results(task_id);
CREATE INDEX IF NOT EXISTS idx_scrape_results_created_at ON scrape_results(created_at DESC);

-- ========================================
-- 5. Crawls table
-- ========================================
CREATE TABLE IF NOT EXISTS crawls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    status VARCHAR(50) NOT NULL,
    total_tasks INTEGER DEFAULT 0,
    completed_tasks INTEGER DEFAULT 0,
    failed_tasks INTEGER DEFAULT 0,
    config JSONB,
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_crawls_team ON crawls(team_id);
CREATE INDEX IF NOT EXISTS idx_crawls_status ON crawls(status);
CREATE INDEX IF NOT EXISTS idx_crawls_team_status ON crawls(team_id, status DESC, created_at DESC);

-- ========================================
-- 6. Credits table
-- ========================================
CREATE TABLE IF NOT EXISTS credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL UNIQUE REFERENCES teams(id) ON DELETE CASCADE,
    balance BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credits_team_id ON credits(team_id);

-- ========================================
-- 7. Credits Transactions table
-- ========================================
CREATE TABLE IF NOT EXISTS credits_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    amount BIGINT NOT NULL,
    transaction_type VARCHAR(50) NOT NULL,
    description TEXT,
    reference_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_credits_transactions_team_created ON credits_transactions(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_credits_transactions_reference ON credits_transactions(reference_id);

-- ========================================
-- 8. Webhooks table
-- ========================================
CREATE TABLE IF NOT EXISTS webhooks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    events JSONB NOT NULL,
    secret VARCHAR(255),
    active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhooks_team_id ON webhooks(team_id);

-- ========================================
-- 9. Webhook Events table
-- ========================================
CREATE TABLE IF NOT EXISTS webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL,
    payload JSONB,
    status VARCHAR(50) NOT NULL,
    response_code INTEGER,
    response_body TEXT,
    attempt_count INTEGER DEFAULT 0,
    next_retry_at TIMESTAMPTZ,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_events_team_status ON webhook_events(team_id, status, next_retry_at ASC);
CREATE INDEX IF NOT EXISTS idx_webhook_events_webhook_id ON webhook_events(webhook_id);

-- ========================================
-- 10. Tasks Backlog table
-- ========================================
CREATE TABLE IF NOT EXISTS tasks_backlog (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type VARCHAR(50) NOT NULL,
    url TEXT NOT NULL,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    priority INTEGER DEFAULT 0,
    payload JSONB,
    scheduled_at TIMESTAMPTZ,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tasks_backlog_team_status ON tasks_backlog(team_id, status, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_tasks_backlog_scheduled ON tasks_backlog(scheduled_at);

-- ========================================
-- 11. Geo Restriction Log table
-- ========================================
CREATE TABLE IF NOT EXISTS geo_restriction_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    country_code VARCHAR(2),
    ip_address INET,
    action VARCHAR(50) NOT NULL,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_geo_restriction_log_team_created ON geo_restriction_log(team_id, created_at DESC);

-- ========================================
-- 12. Auth Scopes table
-- ========================================
CREATE TABLE IF NOT EXISTS auth_scopes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    read BOOLEAN NOT NULL DEFAULT TRUE,
    write BOOLEAN NOT NULL DEFAULT FALSE,
    admin BOOLEAN NOT NULL DEFAULT FALSE,
    search_limit INTEGER NOT NULL DEFAULT 100,
    scrape_limit INTEGER NOT NULL DEFAULT 50,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(api_key_id)
);

CREATE INDEX IF NOT EXISTS idx_auth_scopes_api_key_id ON auth_scopes(api_key_id);

-- ========================================
-- 13. Audit Log table
-- ========================================
CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID REFERENCES teams(id) ON DELETE CASCADE,
    user_id UUID,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(100),
    resource_id UUID,
    ip_address INET,
    user_agent TEXT,
    request_path TEXT,
    request_method VARCHAR(10),
    response_status INTEGER,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_team ON audit_log(team_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id, created_at DESC);

-- ========================================
-- Safe credit deduction function
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
-- Safe credit addition function
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

-- GIN index for JSON columns
CREATE INDEX IF NOT EXISTS idx_tasks_payload_gin ON tasks USING GIN (payload);

-- ========================================
-- Functions to update timestamps automatically
-- ========================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply timestamp triggers to all relevant tables
DO $$
DECLARE
    table_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['teams', 'api_keys', 'tasks', 'crawls', 'credits', 'webhooks', 'webhook_events', 'tasks_backlog', 'auth_scopes']
    LOOP
        EXECUTE format('
            DROP TRIGGER IF EXISTS trigger_update_%s_updated_at ON %s;
            CREATE TRIGGER trigger_update_%s_updated_at
                BEFORE UPDATE ON %s
                FOR EACH ROW
                EXECUTE FUNCTION update_updated_at_column();
        ', table_name, table_name, table_name, table_name);
    END LOOP;
END $$;
