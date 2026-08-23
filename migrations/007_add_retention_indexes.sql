-- Migration: 007_add_retention_indexes
-- Description: 为保留期清理扫描提供索引（R-retention-006，change db-retention-governance）
--
-- RetentionWorker（src/workers/retention_worker.rs）按 created_at/delivered_at/
-- updated_at 执行 DELETE 扫描。现有索引均以 team_id 打头（团队维度查询），
-- 无法支撑按时间条件的全表清理，会产生 Seq Scan（表增长后代价线性上升）。
--
-- 设计依据（沿用 migration 003 的 partial index 思路）：
--   - webhook_events 只对终态行建 partial index（delivered/dead），
--     仅索引会被清理命中的行，写入开销低于全表索引，且索引体积小
--   - geo_restriction_logs / audit_logs 用 created_at 单列索引（无高频
--     status 过滤条件，partial 无收益）
--   - scrape_results 复用 migration 001 的 idx_scrape_results_created_at，
--     不重复建索引
--
-- 幂等性：全部 IF NOT EXISTS，CI 重复执行无副作用。

-- 1. webhook_events：delivered 按 delivered_at 清理
CREATE INDEX IF NOT EXISTS idx_webhook_events_delivered_at
    ON webhook_events (delivered_at)
    WHERE status = 'delivered';

-- 2. webhook_events：dead 按 updated_at 清理
CREATE INDEX IF NOT EXISTS idx_webhook_events_dead_updated_at
    ON webhook_events (updated_at)
    WHERE status = 'dead';

-- 3. geo_restriction_logs：按 created_at 清理
CREATE INDEX IF NOT EXISTS idx_geo_restriction_logs_created_at
    ON geo_restriction_logs (created_at);

-- 4. audit_logs：按 created_at 清理
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at
    ON audit_logs (created_at);