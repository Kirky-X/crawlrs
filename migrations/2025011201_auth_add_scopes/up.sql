-- Migration: 2025011201_auth_add_scopes
-- Purpose: Add API Key scopes and feature flags tables for enhanced authorization

-- Create auth_scopes table for API Key permissions
CREATE TABLE IF NOT EXISTS auth_scopes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    read BOOLEAN NOT NULL DEFAULT true,
    write BOOLEAN NOT NULL DEFAULT false,
    admin BOOLEAN NOT NULL DEFAULT false,
    search_limit INTEGER NOT NULL DEFAULT 100,
    scrape_limit INTEGER NOT NULL DEFAULT 50,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(api_key_id)
);

-- Create index for fast lookups
CREATE INDEX IF NOT EXISTS idx_auth_scopes_api_key_id ON auth_scopes(api_key_id);

-- Update api_keys table to add feature_flags column (JSONB for flexibility)
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS feature_flags JSONB DEFAULT '{}'::jsonb;

-- Add updated_at column if not exists
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ DEFAULT NOW();

-- Create function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_api_keys_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger for auto-updating timestamp
DROP TRIGGER IF EXISTS trigger_update_api_keys_timestamp ON api_keys;
CREATE TRIGGER trigger_update_api_keys_timestamp
    BEFORE UPDATE ON api_keys
    FOR EACH ROW
    EXECUTE FUNCTION update_api_keys_timestamp();

-- Create function to update auth_scopes timestamp
CREATE OR REPLACE FUNCTION update_auth_scopes_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger for auto-updating timestamp
DROP TRIGGER IF EXISTS trigger_update_auth_scopes_timestamp ON auth_scopes;
CREATE TRIGGER trigger_update_auth_scopes_timestamp
    BEFORE UPDATE ON auth_scopes
    FOR EACH ROW
    EXECUTE FUNCTION update_auth_scopes_timestamp();

COMMENT ON TABLE auth_scopes IS 'Stores fine-grained permissions for each API Key';
COMMENT ON COLUMN auth_scopes.read IS 'Permission to access read-only endpoints (search, scrape GET)';
COMMENT ON COLUMN auth_scopes.write IS 'Permission to access write endpoints (config, upload)';
COMMENT ON COLUMN auth_scopes.admin IS 'Permission to access administrative endpoints (team, billing)';
COMMENT ON COLUMN auth_scopes.search_limit IS 'Maximum number of search requests per hour';
COMMENT ON COLUMN auth_scopes.scrape_limit IS 'Maximum number of scrape requests per hour';
