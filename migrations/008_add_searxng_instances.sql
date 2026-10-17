-- Migration: 008_add_searxng_instances
-- Description: SearXNG 公共实例登记簿（change searxng-meta-search）
--
-- 登记簿是实例生命周期的唯一持久真相：采集器（searx.space 数据集）写入准入元数据、
-- 健康探活 worker 回填探测结果、状态机（candidate/active/cooldown/retired）迁移状态。
-- 进程内热池（target=3）只是这份数据的缓存，任何内存态都可从本表 + 探活重建。
--
-- 幂等性：CREATE TABLE / INDEX 全部 IF NOT EXISTS，CI 重复执行无副作用。

CREATE TABLE IF NOT EXISTS searxng_instances (
    id                   UUID PRIMARY KEY,
    -- 规范化 base URL（https、去尾斜杠、host 小写），唯一键即实例身份
    url                  TEXT NOT NULL UNIQUE,
    -- searx_space（数据集） / seed（配置手工引导；采集器永不因缺席退役）
    source               TEXT NOT NULL,
    -- candidate / active / cooldown / retired
    state                TEXT NOT NULL DEFAULT 'candidate',
    -- 数据集元数据（可空：schema 漂移与 seed 实例均可能缺字段）
    network_type         TEXT,
    version              TEXT,
    api_grade            TEXT,
    http_grade           TEXT,
    tls_grade            TEXT,
    cdn_grade            TEXT,
    uptime_day           DOUBLE PRECISION,
    uptime_month         DOUBLE PRECISION,
    -- 探活 / 请求回填
    latency_ms           BIGINT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_success_at      TIMESTAMPTZ,
    last_failure_at      TIMESTAMPTZ,
    last_error           TEXT,
    -- 连续未出现在数据集的采集轮数（searx_space 来源退役依据）
    dataset_miss_count   INTEGER NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 候选晋升按 uptime_day DESC, latency_ms ASC NULLS LAST 排序
CREATE INDEX IF NOT EXISTS idx_searxng_instances_state_uptime
    ON searxng_instances (state, uptime_day DESC);

-- 管理 API 列表按 state 优先 + uptime 降序分页
CREATE INDEX IF NOT EXISTS idx_searxng_instances_uptime
    ON searxng_instances (uptime_day DESC);
