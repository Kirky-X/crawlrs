# Spec — logging

> Delta spec for change `platform-modernization-2026-07`. 覆盖 inklog 完全替换 tracing 的需求。

## Requirements

### R-log-001: 日志初始化替换为 inklog
bootstrap 层的 `tracing_subscriber::fmt()` 初始化替换为 `inklog::LoggerManager::with_config()`。

**验收标准：**
- src/bootstrap/ 中无 `tracing_subscriber` 引用
- 日志初始化使用 inklog，配置通过 confers 加载（InklogConfig）
- 支持控制台+文件双输出，文件自动轮转

### R-log-002: 全项目宏替换
所有 `tracing::info!/error!/warn!/debug!` 替换为 `log::info!/error!/warn!/trace!`。

**验收标准：**
- `grep -r "tracing::" src/` 返回 0 结果
- `grep -r "#[tracing::instrument" src/` 返回 0 结果
- `grep -r "tracing::Span" src/` 返回 0 结果
- `cargo build --features default` 通过

### R-log-003: 移除 tracing::instrument
所有 `#[tracing::instrument]` 属性移除，关键函数改用 inklog 结构化日志的 context 字段。

**验收标准：**
- src/ 下无 `#[tracing::instrument]` 或 `#[instrument]` 属性
- 关键业务函数（use cases）的日志包含函数名和参数 context

## Constraints
- metrics crate 保留（不依赖 tracing）
- 不影响 Prometheus 指标导出
- 日志格式保持结构化 JSON

## Out of Scope
- 不替换 log crate 本身（log 是 facade，inklog 实现它）
- 不修改 tracing 在第三方依赖中的使用（传递依赖）
