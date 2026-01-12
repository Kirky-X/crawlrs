-- Rollback: 2025011202_auth_add_feature_flags
-- Purpose: Revert the feature flags changes

-- Drop triggers
DROP TRIGGER IF EXISTS trigger_update_auth_feature_flag_overrides ON auth_feature_flag_overrides;
DROP TRIGGER IF EXISTS trigger_update_auth_feature_flags ON auth_feature_flags;

-- Drop functions
DROP FUNCTION IF EXISTS update_auth_feature_flags_timestamp();

-- Drop tables (CASCADE will handle dependent objects)
DROP TABLE IF EXISTS auth_feature_flag_overrides;
DROP TABLE IF EXISTS auth_feature_flags;
