# Design — platform-modernization-2026-07

## Context
crawlrs 是企业级 Rust 爬虫平台，使用 DDD 分层架构。当前 Cargo.toml 依赖 4 个本地路径 crate，且使用 tracing 做日志。crates.io 上已有所有目标 crate 的最新稳定版。项目使用 Shaku DI、Axum 0.8、Sea-ORM 2.0.0-rc（通过 dbnexus）、Redis 1.0。Feature flag 体系已建立（engine-*/storage-*/search-*/dbnexus-* 等），但 confers/dbnexus/limiteron/oxcache 未通过 feature 门禁。

## Decision

### D1: 依赖迁移策略
将 6 个本地路径依赖切换为 crates.io 版本，全部使用 `default-features = false` + 显式 features：

| Crate | 版本 | features |
|-------|------|----------|
| oxcache | 0.3.3 | moka, redis, serialization, macros, batch-write, metrics, bloom-filter, wal-recovery, rate-limiting, tracing, futures |
| dbnexus | 0.2.0 | postgres, runtime-tokio-rustls, permission, cache, macros, migration, sql-parser, metrics, config-env, with-chrono, with-uuid, with-json |
| inklog | 0.1.2 | file-sink, console-sink, compression, encryption, database-sink, http, cli, confers |
| sdforge | 0.3.1 | axum, http, tower, tower-http, tokio, serde, serde_json, uuid, chrono, validator, regex |
| confers | 0.2.2 | toml, json, yaml, env, validation, watch, dynamic |
| limiteron | 0.2.1 | postgres, ban-manager, quota-control, circuit-breaker, telemetry, monitoring, parallel-checker, audit-log |

移除依赖：tracing, tracing-subscriber, tracing-appender（被 inklog 替代）

### D2: inklog 完全替换 tracing
- 移除 Cargo.toml 中 tracing/tracing-subscriber/tracing-appender 依赖
- 添加 inklog 依赖（features: file-sink, console-sink, compression, encryption, database-sink, http, cli, confers）
- 日志初始化：bootstrap 中用 `inklog::LoggerManager::with_config()` 替换 `tracing_subscriber::fmt()`
- 宏替换：全项目 `tracing::info!` → `log::info!`、`tracing::error!` → `log::error!`、`tracing::warn!` → `log::warn!`、`tracing::debug!` → `log::debug!`
- `#[tracing::instrument]` 移除（inklog 用结构化日志，不需要 span instrument）
- `tracing::Span` / `tracing::Instrument` 用法移除或替换为 inklog 的 context
- metrics 层保留（metrics crate 不依赖 tracing）

### D3: sdforge 接口封装
- sdforge 将 domain services / application use cases 的 Rust trait 通过 `#[sdforge::api]` 宏封装为 HTTP 接口
- 新增 feature flag `api-sdk` 控制 sdforge 集成
- sdforge 生成的 HTTP 路由注册到 Axum router
- 配置通过 confers 加载 sdforge 宏参数
- 不替换 Shaku DI，sdforge 与 Shaku 并存

### D4: 幽灵函数移除策略
gitnexus cypher 查询无入边的 Function 节点，但需过滤以下误报：
- Trait 方法实现（`impl Trait for Type` 的方法）
- `From`/`Display`/`Default`/`Debug` 标准 trait 实现
- Getter 方法（struct 字段访问器）
- `new`/`build`/`build_async` 构造器（可能被宏调用）
- `test_*` 前缀函数
- 路由 handler（被 Axum 宏调用，gitnexus 可能漏检）

验证流程：cypher 查询候选 → 读取源码确认 → gitnexus context 查 360° 引用 → Grep 搜索字符串引用 → 确认 0 引用后移除

### D5: 特性门禁完善
- confers/dbnexus/limiteron/oxcache 设为 `optional = true`
- 新增 feature flag：`config` (confers), `logging` (inklog), `api-sdk` (sdforge)
- default features 包含必要子集，`full` 包含全部
- 移除不必要的 default-features

### D6: 测试覆盖率提升
- 用 `cargo tarpaulin` 或 `cargo llvm-cov` 测量基线
- 优先补 domain/application 层单元测试（TDD 方式）
- infrastructure 层用 mock 测试
- presentation 层用 axum-test 集成测试
- 目标：行覆盖率 ≥ 90%

## Alternatives Considered

### A1: inklog 作为 tracing 后端（拒绝）
inklog 可集成 tracing 生态，但用户明确要求"完全替换 tracing"。保留 tracing 宏会导致两套日志 API 并存，维护成本高。

### A2: sdforge 替换 Shaku DI（拒绝）
用户明确 sdforge 用于"将 Rust 接口通过 feature 封装成 HTTP 接口"，不替换 DI。Shaku 仍负责依赖注入，sdforge 负责接口暴露。

### A3: 保留本地路径依赖（拒绝）
用户明确要求"都使用云端仓库版本，而不是本地版本"。本地路径依赖导致项目无法独立构建，且与云端最新版脱节。

## Consequences
- **正面**：项目可独立构建（无需本地 path）；依赖版本统一；日志层升级为企业级；接口可通过 feature 暴露为 HTTP；二进制可通过 feature 控制大小
- **负面**：inklog 完全替换 tracing 是大范围代码修改（100+ 文件的宏替换）；sdforge 集成是新增架构层；幽灵函数移除需逐个验证（耗时）
- **技术债**：dbnexus 0.2.0 的 features 名称与本地版可能不同（config-yaml → config-toml），需适配；Sea-ORM 版本需与 dbnexus 0.2.0 对齐
- **后续跟进**：stashes 中有 3 个历史 stash 未处理（含 webhook/dbnexus 改动），本次不处理
