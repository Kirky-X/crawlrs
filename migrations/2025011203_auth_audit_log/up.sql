-- Migration: 2025011203_auth_audit_log
-- Purpose: Add enhanced audit logging for authentication and authorization decisions

-- Create auth_audit_log table
CREATE TABLE IF NOT EXISTS auth_audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID, -- Nullable for anonymous requests
    team_id UUID, -- Nullable for anonymous requests
    requested_action VARCHAR(255) NOT NULL,
    decision VARCHAR(50) NOT NULL, -- 'ALLOW' or 'DENY'
    denial_reason VARCHAR(255), -- Reason for denial if applicable
    scope_used JSONB, -- Scopes used for authorization
    ip_address INET,
    trace_id UUID,
    user_agent TEXT,
    request_path TEXT,
    request_method VARCHAR(10),
    metadata JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_auth_audit_log_api_key ON auth_audit_log(api_key_id);
CREATE INDEX IF NOT EXISTS idx_auth_audit_log_team_id ON auth_audit_log(team_id);
CREATE INDEX IF NOT EXISTS idx_auth_audit_log_created_at ON auth_audit_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_auth_audit_log_trace_id ON auth_audit_log(trace_id);
CREATE INDEX IF NOT EXISTS idx_auth_audit_log_decision ON auth_audit_log(decision);

-- Create function to clean old audit logs (for scheduled job)
CREATE OR REPLACE FUNCTION cleanup_old_audit_logs(retention_days INTEGER DEFAULT 90)
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM auth_audit_log WHERE created_at < NOW() - (retention_days || ' days')::interval;
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

COMMENT ON TABLE auth_audit_log IS 'Comprehensive audit log for authentication and authorization decisions';
COMMENT ON COLUMN auth_audit_log.api_key_id IS 'Anonymized API Key ID for privacy';
COMMENT ON COLUMN auth_audit_log.decision IS 'Authorization decision: ALLOW or DENY';
COMMENT ON COLUMN auth_audit_log.denial_reason IS 'Error code or reason for denial (e.g., SCOPE_FORBIDDEN)';
COMMENT ON COLUMN auth_audit_log.scope_used IS 'JSON representation of scopes used for authorization';
COMMENT ON FUNCTION cleanup_old_audit_logs IS 'Removes audit logs older than specified retention days';
