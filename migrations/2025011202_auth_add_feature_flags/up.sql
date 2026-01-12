-- Migration: 2025011202_auth_add_feature_flags
-- Purpose: Add feature flags table for runtime feature control

-- Create auth_feature_flags table
CREATE TABLE IF NOT EXISTS auth_feature_flags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT false,
    rollout_percentage INTEGER NOT NULL DEFAULT 0 CHECK (rollout_percentage >= 0 AND rollout_percentage <= 100),
    metadata JSONB DEFAULT '{}'::jsonb,
    started_at TIMESTAMPTZ,
    stopped_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(name)
);

-- Create index for fast lookups by name
CREATE INDEX IF NOT EXISTS idx_auth_feature_flags_name ON auth_feature_flags(name);

-- Create auth_feature_flag_overrides table for per-API-Key feature flag settings
CREATE TABLE IF NOT EXISTS auth_feature_flag_overrides (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    feature_flag_id UUID NOT NULL REFERENCES auth_feature_flags(id) ON DELETE CASCADE,
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(feature_flag_id, api_key_id)
);

-- Create index for fast lookups
CREATE INDEX IF NOT EXISTS idx_auth_feature_flag_overrides_api_key ON auth_feature_flag_overrides(api_key_id);
CREATE INDEX IF NOT EXISTS idx_auth_feature_flag_overrides_flag ON auth_feature_flag_overrides(feature_flag_id);

-- Create function to update timestamp
CREATE OR REPLACE FUNCTION update_auth_feature_flags_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create triggers for auto-updating timestamp
DROP TRIGGER IF EXISTS trigger_update_auth_feature_flags ON auth_feature_flags;
CREATE TRIGGER trigger_update_auth_feature_flags
    BEFORE UPDATE ON auth_feature_flags
    FOR EACH ROW
    EXECUTE FUNCTION update_auth_feature_flags_timestamp();

DROP TRIGGER IF EXISTS trigger_update_auth_feature_flag_overrides ON auth_feature_flag_overrides;
CREATE TRIGGER trigger_update_auth_feature_flag_overrides
    BEFORE UPDATE ON auth_feature_flag_overrides
    FOR EACH ROW
    EXECUTE FUNCTION update_auth_feature_flags_timestamp();

COMMENT ON TABLE auth_feature_flags IS 'Global feature flags for runtime feature control';
COMMENT ON TABLE auth_feature_flag_overrides IS 'Per-API-Key feature flag overrides';
COMMENT ON COLUMN auth_feature_flags.rollout_percentage IS 'Percentage of API Keys that can access this feature (0-100)';
COMMENT ON COLUMN auth_feature_flags.metadata IS 'Additional configuration for the feature flag';
