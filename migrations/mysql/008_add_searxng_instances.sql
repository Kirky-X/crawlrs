-- Migration: 008_add_searxng_instances (MySQL dialect)
-- SearXNG 公共实例登记簿。DOUBLE PRECISION -> DOUBLE，TIMESTAMPTZ -> DATETIME。

CREATE TABLE IF NOT EXISTS searxng_instances (
    id                   CHAR(36) PRIMARY KEY,
    url                  TEXT NOT NULL UNIQUE,
    source               TEXT NOT NULL,
    state                TEXT NOT NULL DEFAULT 'candidate',
    network_type         TEXT,
    version              TEXT,
    api_grade            TEXT,
    http_grade           TEXT,
    tls_grade            TEXT,
    cdn_grade            TEXT,
    uptime_day           DOUBLE,
    uptime_month         DOUBLE,
    latency_ms           BIGINT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_success_at      DATETIME,
    last_failure_at      DATETIME,
    last_error           TEXT,
    dataset_miss_count   INTEGER NOT NULL DEFAULT 0,
    created_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 候选晋升按 uptime_day DESC, latency_ms ASC 排序
CREATE INDEX idx_searxng_instances_state_uptime
    ON searxng_instances (state, uptime_day DESC);

-- 管理 API 列表按 state 优先 + uptime 降序分页
CREATE INDEX idx_searxng_instances_uptime
    ON searxng_instances (uptime_day DESC);
