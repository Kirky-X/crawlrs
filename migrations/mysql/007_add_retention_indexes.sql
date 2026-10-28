-- Migration: 007_add_retention_indexes (MySQL dialect)
-- 保留期清理扫描索引；MySQL 不支持 partial index，用普通索引替代
-- （选择性由清理查询的 WHERE status 过滤补足）。

CREATE INDEX idx_webhook_events_delivered_at
    ON webhook_events (delivered_at);

CREATE INDEX idx_webhook_events_dead_updated_at
    ON webhook_events (updated_at);

CREATE INDEX idx_geo_restriction_logs_created_at
    ON geo_restriction_logs (created_at);

CREATE INDEX idx_audit_logs_created_at
    ON audit_logs (created_at);
