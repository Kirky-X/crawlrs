## 1. Cargo.toml 特性重构
- [x] 1.1 定义 `default` 特性组合（engine-reqwest, redis-cache, metrics, db-postgres）
- [x] 1.2 定义 `full` 特性组合（所有引擎 + db-sqlite）
- [x] 1.3 添加引擎特性定义（engine-reqwest, engine-playwright, engine-fire-cdp, engine-fire-tls）
- [x] 1.4 添加基础设施特性定义（redis-cache, metrics）
- [x] 1.5 添加数据库特性定义（db-postgres, db-sqlite）
- [x] 1.6 将 chromiumoxide 标记为 optional 依赖
- [x] 1.7 将 redis 标记为 optional 依赖
- [x] 1.8 将 metrics 和 metrics-exporter-prometheus 标记为 optional 依赖
- [x] 1.9 调整 sea-orm 和 sqlx 的特性透传

## 2. 引擎模块条件编译
- [x] 2.1 修改 `src/engines/client/mod.rs`，添加条件编译属性
- [x] 2.2 使用 `#[cfg(feature = "engine-playwright")]` 控制 playwright 模块导出
- [x] 2.3 使用 `#[cfg(feature = "engine-fire-cdp")]` 控制 fire_cdp 模块导出
- [x] 2.4 使用 `#[cfg(feature = "engine-fire-tls")]` 控制 fire_tls 模块导出
- [x] 2.5 添加对应的 re-export 语句条件编译

## 3. 基础设施模块条件编译
- [x] 3.1 修改 `src/infrastructure/cache/mod.rs`，条件导出 redis_client
- [x] 3.2 修改 `src/infrastructure/observability/mod.rs`，条件导出 metrics

## 4. main.rs 应用集成
- [x] 4.1 为 PlaywrightEngine 导入添加条件编译
- [x] 4.2 为 FireEngineCdp 导入添加条件编译
- [x] 4.3 为 FireEngineTls 导入添加条件编译
- [x] 4.4 为 metrics 初始化添加条件编译
- [x] 4.5 为 Redis 客户端初始化添加条件编译
- [x] 4.6 使用 `#[cfg(...)]` 块包裹引擎注册逻辑
- [x] 4.7 处理禁用特性时的回退逻辑

## 5. 库入口特性检查
- [x] 5.1 在 `src/lib.rs` 添加数据库特性组合检查
- [x] 5.2 使用 `compile_error!` 宏确保至少启用一个数据库后端

## 6. 测试验证
- [x] 6.1 验证默认特性组合能正常编译
- [ ] 6.2 验证全功能组合能正常编译
- [ ] 6.3 验证禁用 redis-cache 时系统行为
- [ ] 6.4 验证禁用 metrics 时系统行为
- [x] 6.5 运行 `cargo build --release` 验证构建成功
- [ ] 6.6 运行 `cargo clippy -- -D warnings` 验证代码质量（现有警告非本次修改引入）
