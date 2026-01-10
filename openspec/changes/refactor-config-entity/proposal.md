# Change: 集中配置结构体到 src/config/entity

## Why

当前项目的配置结构体分散在多个模块中，包括：
- `src/domain/services/rate_limiting_service.rs` (RateLimitConfig, ConcurrencyConfig)
- `src/infrastructure/services/rate_limiting_service_impl.rs` (RateLimitingConfig)
- `src/infrastructure/cache/cache_strategy.rs` (CacheStrategyConfig, PreheatConfig, LayeredCacheConfig)
- `src/engines/circuit_breaker.rs` (CircuitConfig)
- `src/engines/health_monitor.rs` (HealthCheckConfig)
- `src/engines/traits.rs` (ScreenshotConfig)
- `src/infrastructure/search/search_engine_router.rs` (SearchEngineRouterConfig)
- `src/infrastructure/search/smart_search.rs` (SmartSearchEngineConfig)
- `src/infrastructure/search/deduplicator.rs` (DeduplicationConfig, ContentFingerprintConfig)
- `src/infrastructure/search/factory.rs` (SearchEngineFactoryConfig)

这种分散带来了以下问题：
1. 配置查找和维护困难
2. 难以确保所有配置都有合理的默认值
3. 新开发者难以理解配置结构全貌
4. 配置复用性差

## What Changes

- 创建 `src/config/entity/` 目录，存放所有配置结构体
- 将所有配置结构体及其 Default 实现迁移到 entity 目录
- 为每个配置结构体添加完整的默认初始值
- 更新所有引用配置结构体的模块，导入路径从 entity 目录
- 保持配置结构体的 `Debug`、`Clone`、`Serialize/Deserialize` 等派生 trait

## Impact

- Affected specs: config, engines, infrastructure
- Affected code:
  - `src/config/mod.rs` - 添加 entity 模块导出
  - `src/config/entity/mod.rs` - 新建，导出所有配置结构体
  - 所有分散的配置结构体位置
  - 所有引用配置结构体的文件

## Migration

1. 创建 entity 目录并移动配置结构体
2. 更新所有导入路径
3. 运行测试确保功能正常
4. 更新文档
