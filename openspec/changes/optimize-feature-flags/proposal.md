# Change: Optimize Feature Flags and Conditional Compilation

## Why

当前项目的特性管理存在以下问题：
1. **Cargo.toml 特性定义不完整**：缺少系统化的特性分组和依赖管理
2. **代码模块缺少条件编译**：所有引擎、缓存、指标模块始终编译，即使未使用
3. **依赖管理混乱**：chromiumoxide、redis 等重量级依赖未标记为可选
4. **无法实现轻量化部署**：无法仅编译必要的组件以减小二进制体积
5. **构建时间过长**：每次构建都会编译所有依赖，包括可能不需要的组件

通过引入完善的特性系统，可以实现：
- 按需编译，减少构建时间和二进制体积
- 清晰的特性依赖关系，避免无效特性组合
- 支持不同部署场景（轻量化 vs 全功能）

## What Changes

- **Cargo.toml 特性重构**：
  - 定义 `default`、`full` 特性组合
  - 分离引擎特性（engine-reqwest, engine-playwright, engine-fire-cdp, engine-fire-tls）
  - 分离基础设施特性（redis-cache, metrics）
  - 分离数据库特性（db-postgres, db-sqlite）
  - 将重量级依赖标记为 optional

- **代码模块条件编译**：
  - `src/engines/client/mod.rs`：根据特性开关导出对应引擎
  - `src/infrastructure/cache/mod.rs`：条件导出 redis_client
  - `src/infrastructure/observability/mod.rs`：条件导出 metrics

- **main.rs 动态初始化**：
  - 使用 `#[cfg(feature = "...")]` 包裹引擎初始化代码
  - 实现特性开关与组件初始化的联动

- **编译时错误检查**：
  - 添加 `compile_error!` 防止无效特性组合
  - 确保至少启用一个数据库后端

## Impact

- Affected specs: `feature-flags`
- Affected code:
  - `Cargo.toml` - 特性定义和依赖管理
  - `src/engines/client/mod.rs` - 引擎模块条件编译
  - `src/infrastructure/cache/mod.rs` - 缓存模块条件编译
  - `src/infrastructure/observability/mod.rs` - 指标模块条件编译
  - `src/main.rs` - 应用集成逻辑
  - `src/lib.rs` - 库入口和特性检查

**Breaking Changes**: 无（向后兼容，仅增加编译选项）
