-- Migration: 007_add_retention_indexes (SQLite dialect)
-- 保留期清理扫描索引；SQLite 支持 partial index。

CREATE INDEX IF NOT EXISTS idx_webhook_events_delivered_at
    ON webhook_events (delivered_at)
    WHERE status = 'delivered';

CREATE INDEX IF NOT EXISTS idx_webhook_events_dead_updated_at
    ON webhook_events (updated_at)
    WHERE status = 'dead';

CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_created_at
    ON geo_restriction_logs (created_at);

CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at
    ON audit_logs (created_at);
