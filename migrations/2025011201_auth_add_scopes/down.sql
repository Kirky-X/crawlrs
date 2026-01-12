-- Rollback: 2025011201_auth_add_scopes
-- Purpose: Revert the auth scopes changes

-- Drop triggers
DROP TRIGGER IF EXISTS trigger_update_auth_scopes_timestamp ON auth_scopes;
DROP TRIGGER IF EXISTS trigger_update_api_keys_timestamp ON api_keys;

-- Drop functions
DROP FUNCTION IF EXISTS update_auth_scopes_timestamp();
DROP FUNCTION IF EXISTS update_api_keys_timestamp();

-- Drop table (CASCADE will handle dependent objects)
DROP TABLE IF EXISTS auth_scopes;

-- Remove columns from api_keys
ALTER TABLE api_keys DROP COLUMN IF EXISTS feature_flags;
ALTER TABLE api_keys DROP COLUMN IF EXISTS updated_at;
