## Context

任务队列是爬虫系统的核心组件，负责任务的调度和执行。当前实现仅支持基础的 PostgreSQL 队列，缺少生产环境所需的关键能力。

### 约束条件
- 必须保持向后兼容
- 不能引入过重的外部依赖
- 需要支持高并发场景

### 利益相关者
- 运维团队: 需要监控和优雅关闭能力
- 开发团队: 需要更好的错误信息和调试能力
- 用户: 需要可靠的任务执行保证

## Goals / Non-Goals

### Goals
1. 提供优雅关闭能力，确保正在执行的任务完成
2. 支持批量操作以提高吞吐量
3. 添加优先级队列支持重要任务的优先执行
4. 提供监控指标用于生产环境运维
5. 支持延迟任务执行

### Non-Goals
- 不实现分布式锁
- 不实现消息持久化（依赖 PostgreSQL/Redis 本身）
- 不实现复杂的任务重试策略

## Decisions

### 1. 批量出队设计

**决策**: 添加 `dequeue_batch(size: u32)` 接口

```rust
async fn dequeue_batch(&self, worker_id: Uuid, size: u32) -> Result<Vec<Task>, QueueError>;
```

**理由**: 
- 减少数据库往返次数
- 支持工作者批量获取任务
- 兼容现有单任务出队接口

### 2. 优先级队列设计

**决策**: 使用显式优先级字段而非基于延迟时间

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,
}
```

**理由**:
- 优先级更直观可控
- 便于实现复杂调度策略
- 支持紧急任务插队

### 3. 优雅关闭设计

**决策**: 使用 tokio 的 watch channel 实现信号传递

```rust
pub trait GracefulShutdown {
    async fn shutdown(&self) -> Result<(), QueueError>;
    fn is_shutting_down(&self) -> bool;
}
```

**理由**:
- 与 tokio 生态集成良好
- 支持多个工作者协调
- 实现简单可靠

### 4. 监控指标设计

**决策**: 使用 Prometheus 指标标准

```rust
pub trait QueueMetrics {
    fn queue_depth(&self) -> IntGauge;
    fn tasks_processed(&self) -> IntCounter;
    fn processing_duration(&self) -> FloatHistogram;
}
```

**理由**:
- 业界标准格式
- 便于 Grafana 集成
- 开源生态丰富

### 5. Redis 延迟队列设计

**决策**: 使用 Sorted Set 实现延迟队列

- score: 执行时间戳
- member: 任务序列化数据

**理由**:
- Redis Sorted Set 操作复杂度 O(log N)
- 支持精确的时间排序
- 实现简单可靠

## Risks / Trade-offs

| 风险 | 影响 | 缓解措施 |
|-----|------|---------|
| PostgreSQL 性能瓶颈 | 批量出队可能增加锁竞争 | 限制批次大小，使用行锁优化 |
| Redis 连接数 | 延迟队列增加连接开销 | 使用连接池，配置合理池大小 |
| 优先级饥饿 | 低优先级任务长期无法执行 | 添加公平性机制 |

## Migration Plan

1. **数据库迁移**: 添加 priority 字段到 tasks 表
2. **代码迁移**: 新增接口与旧接口并行工作
3. **配置迁移**: 添加 Redis 连接配置
4. **回滚方案**: 保留旧版本代码，降级使用

## Open Questions

- [ ] 是否需要支持任务组（batch job）？
- [ ] 延迟队列的最大延迟时间限制是多少？
- [ ] 优先级是否需要支持动态调整？
