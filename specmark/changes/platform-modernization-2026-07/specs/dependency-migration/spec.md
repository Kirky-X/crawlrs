# Spec — dependency-migration

> Delta spec for change `platform-modernization-2026-07`. 覆盖依赖从本地路径迁移到 crates.io 的需求。

## Requirements

### R-dep-001: 全部本地路径依赖切换为 crates.io 版本
oxcache/dbnexus/confers/limiteron 四个 crate 从 `path = "..."` 切换为 `version = "x.y.z"`，版本取 crates.io 最新稳定版。

**验收标准：**
- Cargo.toml 中无 `path = "/home/dev/projects/..."` 字样
- `cargo build --features default` 无需本地 /home/dev/projects/ 下任何 crate 即可编译
- 版本号：oxcache=0.3.3, dbnexus=0.2.0, confers=0.2.2, limiteron=0.2.1

### R-dep-002: 新增 inklog 和 sdforge 依赖
inklog 0.1.2 和 sdforge 0.3.1 作为新依赖加入，均设为 optional + feature 门禁。

**验收标准：**
- `cargo build --no-default-features` 不包含 inklog/sdforge
- `cargo build --features logging` 包含 inklog
- `cargo build --features api-sdk` 包含 sdforge

### R-dep-003: 全部使用显式 features 而非 default-features
所有 6 个 crate 使用 `default-features = false` + 显式 features 列表。

**验收标准：**
- Cargo.toml 中每个目标 crate 都有 `default-features = false`
- features 列表非空且与 crates.io 文档一致

### R-dep-004: 移除 tracing 相关依赖
tracing/tracing-subscriber/tracing-appender 从 Cargo.toml 移除。

**验收标准：**
- Cargo.toml [dependencies] 段无 tracing/tracing-subscriber/tracing-appender
- `cargo tree -e features` 输出无 tracing 直接依赖（传递依赖可保留）

## Constraints
- dbnexus 0.2.0 的 features 名称可能与本地版不同（config-yaml→config-toml），需查阅 crates.io 文档适配
- Sea-ORM 版本需与 dbnexus 0.2.0 的 sea-orm 依赖对齐
- 不破坏现有 feature flag 体系（engine-*/storage-*/search-* 等保持不变）

## Out of Scope
- 不处理 stashes 中的历史代码
- 不升级非本地路径的第三方依赖（tokio/axum/reqwest 等保持当前版本）
