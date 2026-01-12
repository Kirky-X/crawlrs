# Change: 增强认证、限流、配额与缓存系统

## Why

当前系统存在以下问题：
1. **认证模型过于简单**：仅有团队级 API Key，缺乏细粒度的权限控制（scope/feature flags），无法实现"key 级能力集"
2. **限流实现分散**：per-second/minute/hour 限流分散在多个脚本中，使用 Redis 多次调用，带来不必要的网络开销和性能损耗
3. **配额计费不完善**：当前余额模型缺乏事件溯源（event sourcing）能力，无法支持高并发场景下的精确计费与审计
4. **缓存管理混乱**：缺乏统一的缓存 key 规范、TTL 分层策略和失效机制

## What Changes

### 1. 认证系统增强 (Auth)
- **BREAKING**: 从"团队级全能 key"迁移到"key 级能力集"
- 新增 `ApiKeyScope` 权限模型，支持 endpoint 级权限控制
- 新增 `FeatureFlag` 机制，支持功能开关控制
- 完善审计日志，记录所有认证决策
- 支持 API Key 的权限继承与覆盖

### 2. 限流与并发控制 (Rate Limiting)
- **BREAKING**: 统一限流算法为滑动窗口/漏桶
- 使用 Redis Cell 或成熟限流库，减少 Redis 调用次数
- 支持多维度限流（per-key, per-endpoint, global）
- 实现自适应限流，根据系统负载动态调整阈值

### 3. 配额系统重构 (Credits)
- **BREAKING**: 引入账本模型（Event Sourcing / Append-Only Ledger）
- 实现余额的 Event Store + 异步物化视图
- 支持高并发精确计费与完整审计追溯
- 新增配额透支与预警机制

### 4. 缓存系统规范 (Cache)
- 建立统一的缓存 Key 命名规范（层级、命名空间）
- 实现 TTL 分层策略（热/温/冷数据）
- 建立缓存失效策略（主动失效、TTL 驱逐、容量淘汰）
- 新增缓存预热与降级机制

## Impact

### Affected Capabilities
- `auth` - 认证与授权
- `rate-limit` - 限流控制（新增）
- `credits` - 配额管理（新增）
- `cache` - 缓存管理（新增）

### Affected Code
- `src/presentation/middleware/auth_middleware.rs`
- `src/domain/services/rate_limiting_service.rs`
- `src/infrastructure/cache/`
- `src/domain/services/credits_service.rs`
- 数据库 schema 变更（新增表）

### Breaking Changes
1. API Key 表结构变更（新增 scope、feature_flags 列）
2. 限流配置格式变更（统一为 JSON Schema）
3. 配额记录模型变更（从快照到事件溯源）
4. 缓存 Key 格式变更（遵循新规范）

### Database Migrations Required
- `2025011201_auth_add_scopes.sql` - 新增 scopes 表
- `2025011202_auth_add_feature_flags.sql` - 新增 feature_flags 表
- `2025011203_credits_event_ledger.sql` - 新增事件账本表
- `2025011204_audit_log_enhancement.sql` - 增强审计日志

## Dependencies
- Redis Cell 库（或 Governor 库的增强使用）
- 事件溯源框架（可选，或手写实现）
- 缓存预热组件

## Timeline Estimate
- **Phase 1**: 认证增强 - 2 周
- **Phase 2**: 限流重构 - 1.5 周
- **Phase 3**: 账本模型 - 2 周
- **Phase 4**: 缓存规范 - 1 周
- **Total**: 6.5 周（含测试与迁移）
