# Design: Auth, Rate Limit, Credits & Cache Enhancement

## Context

### Background
当前 crawlrs 项目的认证、限流、配额和缓存系统存在以下挑战：

1. **认证系统**：
   - 仅有 `team_id` 级别的 API Key 验证
   - 无细粒度权限控制，无法按 endpoint 或功能限制
   - 审计日志不完整

2. **限流系统**：
   - 分散在多个脚本中（per-second/minute/hour）
   - 每次限流检查多次调用 Redis
   - 无法自适应调整阈值

3. **配额系统**：
   - 使用简单的余额快照模型
   - 无法追溯完整的消费历史
   - 高并发下可能出现竞态条件

4. **缓存系统**：
   - 缺乏统一的 key 命名规范
   - TTL 配置混乱
   - 无失效策略

### Stakeholders
- API 用户（需要细粒度权限控制）
- 运维团队（需要可观测的限流与缓存）
- 安全团队（需要完整审计日志）
- 计费团队（需要精确配额管理）

### Constraints
- 必须向后兼容现有 API
- 迁移过程不能导致服务中断
- 需要支持百万级 API Key

## Goals / Non-Goals

### Goals
1. 实现 API Key 级能力集（scopes + feature flags）
2. 使用 Redis Cell 实现高效限流（单次 Redis 调用）
3. 引入账本模型支持事件溯源
4. 建立统一的缓存规范

### Non-Goals
- 不实现完整的 OAuth 2.0 或 OpenID Connect
- 不实现复杂的费用结算（仅限 API 调用配额）
- 不强制所有缓存使用本规范（渐进式迁移）
- 不替换现有的熔断器机制

## Decisions

### D1: 权限模型选择

**Decision**: 采用 `Scope + FeatureFlag` 双层模型

**Details**:
- `Scope`: 资源级权限，控制能访问哪些 API
- `FeatureFlag`: 功能级开关，控制能使用哪些功能

**Alternatives Considered**:
- RBAC（角色权限）：过于复杂，不适合 API Key 场景
- ABAC（属性权限）：实现成本高，规则引擎复杂
- OAuth Scopes：过于重量级，学习曲线陡

**Rationale**: 
- Scope 提供粗粒度资源访问控制
- FeatureFlag 提供细粒度功能控制
- 两者结合可以满足大多数场景

**Implementation**:
```rust
// src/domain/auth/scope.rs
pub struct ApiKeyScope {
    pub read: bool,        // 读取权限
    pub write: bool,       // 写入权限
    pub admin: bool,       // 管理权限
    pub scrape_limit: u32, // 抓取请求限制
    pub search_limit: u32, // 搜索请求限制
}

// src/domain/auth/feature_flag.rs  
pub struct FeatureFlag {
    pub name: String,
    pub enabled: bool,
    pub metadata: HashMap<String, String>,
}
```

### D2: 限流算法选择

**Decision**: Redis Cell + 滑动窗口

**Details**:
- 使用 Redis Cell 实现原子计数器（单次 Redis 调用）
- 滑动窗口避免边界突刺
- 支持自适应调整

**Alternatives Considered**:
- Token Bucket：需要额外存储，难以精确控制
- Leaky Bucket：实现复杂，效果类似
- Fixed Window：存在双倍突刺问题

**Rationale**:
- Redis Cell 是 Redis 官方推荐的限流方案
- 原子操作保证并发安全
- 单次调用减少网络开销

**Implementation**:
```rust
// src/infrastructure/rate_limiter/mod.rs
pub struct SlidingWindowRateLimiter {
    redis: RedisConnection,
    cell: RedisCell,
}

impl RateLimiter for SlidingWindowRateLimiter {
    async fn check_rate(
        &self,
        key: &str,
        limit: u32,
        window: Duration,
    ) -> Result<RateLimitResult, RateLimitError> {
        // 使用 Redis Cell 单次调用
        let (allowed, remaining, reset) = self.cell
            .throttle(key, limit, window.as_secs())
            .await?;
            
        Ok(RateLimitResult { allowed, remaining, reset })
    }
}
```

### D3: 账本模型设计

**Decision**: Event Sourcing + Materialized View

**Details**:
- `CreditLedger`: 事件账本表（append-only）
- `CreditBalance`: 物化视图（异步更新）
- 支持完整的审计追溯

**Table Design**:
```sql
-- 事件账本（主表）
CREATE TABLE credit_ledger (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID NOT NULL,
    event_type VARCHAR(50) NOT NULL,  -- 'CREDIT_ADD', 'CREDIT_SPEND', 'CREDIT_REFUND'
    amount DECIMAL(20,4) NOT NULL,
    balance_after DECIMAL(20,4) NOT NULL,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    trace_id UUID,  -- 用于分布式追踪
    idempotency_key VARCHAR(255) UNIQUE  -- 防止重复处理
);

-- 物化视图（用于快速查询）
CREATE MATERIALIZED VIEW credit_balances AS
SELECT 
    api_key_id,
    SUM(CASE WHEN event_type = 'CREDIT_ADD' THEN amount 
             WHEN event_type = 'CREDIT_SPEND' THEN -amount 
             ELSE 0 END) as balance,
    MAX(created_at) as last_updated
FROM credit_ledger
GROUP BY api_key_id;

-- 异步刷新物化视图
CREATE OR REPLACE FUNCTION refresh_credit_balance()
RETURNS TRIGGER AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY credit_balances;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
```

**Alternatives Considered**:
- 传统余额快照：无法追溯历史，高并发竞态
- 事件溯源完整CQRS：实现成本高，需要额外基础设施

**Rationale**:
- Event Sourcing 保证审计完整性
- Materialized View 提供查询性能
- 异步刷新减少主表写入影响

### D4: 缓存规范设计

**Decision**: 层级命名 + TTL 分层 + 失效策略

**Cache Key 规范**:
```
{namespace}:{layer}:{entity}:{id}[:{extra}]
```

**命名空间**:
- `search` - 搜索结果
- `scrape` - 抓取结果  
- `auth` - 认证信息
- `credits` - 配额信息

**TTL 分层**:
| Layer | TTL | 刷新策略 | 典型数据 |
|-------|-----|----------|----------|
| hot | 1-5min | 主动失效 | API Key 信息 |
| warm | 5-30min | TTL 驱逐 | 搜索结果 |
| cold | 30min-2h | TTL 驱逐 | 聚合统计 |

**失效策略**:
1. **主动失效**: 数据变更时主动删除相关缓存
2. **TTL 驱逐**: 时间到期自动删除
3. **容量淘汰**: LRU 策略淘汰冷数据
4. **降级策略**: 缓存不可用时降级到直接查询

## Risks / Trade-offs

| Risk | Impact | Mitigation |
|------|--------|------------|
| 迁移期间 API 不可用 | 高 | 使用蓝绿部署，逐步切换 |
| Redis Cell 性能 | 中 | 监控延迟，设置熔断 |
| 账本数据一致性 | 中 | 使用分布式事务或补偿机制 |
| 缓存 Key 规范冲突 | 低 | 提供迁移脚本和兼容层 |

## Migration Plan

### Phase 1: 认证增强（Week 1-2）
1. 创建新表（scopes, feature_flags）
2. 迁移脚本将现有 API Key 转换为默认 Scope
3. 新增 API Key 创建时支持自定义 Scope
4. 部署认证中间件
5. 验证功能后删除旧字段

### Phase 2: 限流重构（Week 3-4）
1. 部署 Redis Cell 集群
2. 实现新的 RateLimiter
3. 保留旧限流逻辑作为降级
4. 灰度切换 10% 流量
5. 全面切换后删除旧逻辑

### Phase 3: 账本模型（Week 5-6）
1. 创建 ledger 表和物化视图
2. 实现 CreditService 的事件写入
3. 异步刷新物化视图
4. 迁移现有余额数据
5. 切换查询路径

### Phase 4: 缓存规范（Week 7）
1. 发布新的 Key 规范文档
2. 新缓存使用新规范
3. 旧缓存逐步迁移
4. 建立缓存监控

## Open Questions

1. **Q: Redis Cell 的集群支持如何？**
   A: 需要验证 Redis Cluster 下的 Cell 支持情况，可能需要使用 Redlock 或其他方案。

2. **Q: 账本模型的物化视图刷新频率？**
   A: 建议每分钟刷新一次，或使用触发器实时刷新（需要性能测试）。

3. **Q: FeatureFlag 的动态更新？**
   A: 需要提供管理 API 或配置中心集成，支持运行时动态修改。
