-- Rollback: 2025011203_auth_audit_log
-- Purpose: Revert the audit log changes

-- Drop function
DROP FUNCTION IF EXISTS cleanup_old_audit_logs(INTEGER);

-- Drop table (CASCADE will handle dependent objects)
DROP TABLE IF EXISTS auth_audit_log;
