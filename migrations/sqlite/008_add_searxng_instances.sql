-- Migration: 008_add_searxng_instances (SQLite dialect)
-- SearXNG 公共实例登记簿。DOUBLE PRECISION -> REAL，TIMESTAMPTZ -> TEXT；
-- DESC 索引 SQLite 支持。

CREATE TABLE IF NOT EXISTS searxng_instances (
    id                   TEXT PRIMARY KEY,
    url                  TEXT NOT NULL UNIQUE,
    source               TEXT NOT NULL,
    state                TEXT NOT NULL DEFAULT 'candidate',
    network_type         TEXT,
    version              TEXT,
    api_grade            TEXT,
    http_grade           TEXT,
    tls_grade            TEXT,
    cdn_grade            TEXT,
    uptime_day           REAL,
    uptime_month         REAL,
    latency_ms           BIGINT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_success_at      TEXT,
    last_failure_at      TEXT,
    last_error           TEXT,
    dataset_miss_count   INTEGER NOT NULL DEFAULT 0,
    created_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_searxng_instances_state_uptime
    ON searxng_instances (state, uptime_day DESC);

CREATE INDEX IF NOT EXISTS idx_searxng_instances_uptime
    ON searxng_instances (uptime_day DESC);
