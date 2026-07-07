# Spec — api-sdk

> Delta spec for change `platform-modernization-2026-07`. 覆盖 sdforge 将 Rust 接口封装为 HTTP 接口的需求。

## Requirements

### R-sdk-001: sdforge 封装 domain services 为 HTTP 接口
用 sdforge 宏将 TaskService/CrawlService/ScrapeService/SearchService 的核心 trait 方法封装为 HTTP 端点。

**验收标准：**
- src/presentation/sdk/ 模块存在且通过 `#[cfg(feature = "api-sdk")]` 门禁
- `cargo build --features api-sdk` 编译通过
- `cargo build --no-default-features` 不包含 sdforge 依赖

### R-sdk-002: sdforge 路由注册到 Axum router
sdforge 生成的 HTTP 路由在 bootstrap/routes.rs 中注册到 Axum router。

**验收标准：**
- `#[cfg(feature = "api-sdk")]` 块中注册 sdforge 路由
- 非 api-sdk feature 下编译不受影响

### R-sdk-003: sdforge 接口集成测试
为 sdforge 封装的 HTTP 端点编写集成测试。

**验收标准：**
- tests/integration/sdk_api_test.rs 存在
- `cargo test --features api-sdk --test integration_tests` 通过
- 测试覆盖每个封装端点的成功+错误场景

## Constraints
- sdforge 不替换 Shaku DI，两者并存
- sdforge 配置通过 confers 加载
- 新增 feature `api-sdk` 控制 sdforge 启用

## Out of Scope
- 不为所有 domain service 生成 HTTP 接口（仅核心 4 个）
- 不生成 gRPC 接口（仅 HTTP）
- 不替换现有 Axum HTTP handler
