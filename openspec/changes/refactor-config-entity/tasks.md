## 1. 创建配置实体模块结构

- [ ] 1.1 创建 `src/config/entity/mod.rs` 文件
- [ ] 1.2 创建 `src/config/entity/rate_limiting.rs` - 包含 RateLimitConfig, ConcurrencyConfig, RateLimitingConfig
- [ ] 1.3 创建 `src/config/entity/cache.rs` - 包含 CacheStrategyConfig, PreheatConfig, LayeredCacheConfig, CacheType
- [ ] 1.4 创建 `src/config/entity/engines.rs` - 包含 CircuitConfig, HealthCheckConfig, ScreenshotConfig
- [ ] 1.5 创建 `src/config/entity/search.rs` - 包含 SearchEngineRouterConfig, SmartSearchEngineConfig, SearchEngineFactoryConfig
- [ ] 1.6 创建 `src/config/entity/deduplication.rs` - 包含 DeduplicationConfig, ContentFingerprintConfig, DeduplicationStrategy, FingerprintAlgorithm
- [ ] 1.7 创建 `src/config/entity/mod.rs` 统一导出所有配置

## 2. 迁移配置结构体

- [ ] 2.1 迁移 `RateLimitConfig` 和 `ConcurrencyConfig` 到 entity
- [ ] 2.2 迁移 `RateLimitingConfig` 到 entity
- [ ] 2.3 迁移 `CacheStrategyConfig`, `PreheatConfig`, `LayeredCacheConfig`, `CacheType` 到 entity
- [ ] 2.4 迁移 `CircuitConfig` 到 entity
- [ ] 2.5 迁移 `HealthCheckConfig` 到 entity
- [ ] 2.6 迁移 `ScreenshotConfig` 到 entity
- [ ] 2.7 迁移 `SearchEngineRouterConfig` 到 entity
- [ ] 2.8 迁移 `SmartSearchEngineConfig` 到 entity
- [ ] 2.9 迁移 `SearchEngineFactoryConfig` 到 entity
- [ ] 2.10 迁移 `DeduplicationConfig`, `ContentFingerprintConfig`, `DeduplicationStrategy`, `FingerprintAlgorithm` 到 entity

## 3. 更新导入路径

- [ ] 3.1 更新 `src/domain/services/rate_limiting_service.rs` 导入
- [ ] 3.2 更新 `src/infrastructure/services/rate_limiting_service_impl.rs` 导入
- [ ] 3.3 更新 `src/infrastructure/cache/cache_strategy.rs` 导入
- [ ] 3.4 更新 `src/engines/circuit_breaker.rs` 导入
- [ ] 3.5 更新 `src/engines/health_monitor.rs` 导入
- [ ] 3.6 更新 `src/engines/traits.rs` 导入
- [ ] 3.7 更新 `src/infrastructure/search/search_engine_router.rs` 导入
- [ ] 3.8 更新 `src/infrastructure/search/smart_search.rs` 导入
- [ ] 3.9 更新 `src/infrastructure/search/factory.rs` 导入
- [ ] 3.10 更新 `src/infrastructure/search/deduplicator.rs` 导入
- [ ] 3.11 更新所有测试文件中引用的配置结构体导入

## 4. 验证和测试

- [ ] 4.1 运行 `cargo check` 确保编译通过
- [ ] 4.2 运行 `cargo test` 确保所有测试通过
- [ ] 4.3 运行 `cargo clippy -- -D warnings` 确保代码质量
- [ ] 4.4 验证配置文件加载功能正常

## 5. 清理和文档

- [ ] 5.1 删除原位置空荡荡的配置结构体定义（保留类型别名或重新导出）
- [ ] 5.2 更新 `src/config/mod.rs` 导出 entity 模块
- [ ] 5.3 确保所有配置结构体的 Default 实现正确且有文档
