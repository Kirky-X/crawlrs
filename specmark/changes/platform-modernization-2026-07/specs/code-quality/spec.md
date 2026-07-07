# Spec — code-quality

> Delta spec for change `platform-modernization-2026-07`. 覆盖幽灵函数移除和命名修复的需求。

## Requirements

### R-quality-001: 幽灵函数移除
gitnexus 深度分析后，移除验证为死代码的函数。

**验收标准：**
- 候选清单通过 gitnexus cypher 查询生成
- 每个移除的函数经过：源码确认 + gitnexus context 360° 引用 + Grep 字符串搜索 = 0 引用
- 移除后 `cargo build --features default` 通过
- 移除后 `cargo test --features default` 通过

### R-quality-002: 命名修复
修复过时的函数命名调用（governor→limiteron、sea-orm 旧 API、db-postgres→dbnexus-postgres 等）。

**验收标准：**
- `grep -r "governor" src/` 返回 0 结果
- `grep -r "db-postgres" src/` 返回 0 结果
- gitnexus query 未发现过时 API 调用

### R-quality-003: 特性门禁完善
所有可按需启用的依赖设为 optional + feature 门禁。

**验收标准：**
- `cargo build --no-default-features` 编译通过（最小二进制）
- `cargo build --features lite` 编译通过
- `cargo build --features full` 编译通过
- 二进制大小：lite < default < full

## Constraints
- 幽灵函数移除必须保守——有疑虑的保留
- 命名修复不改变函数行为
- 特性门禁不破坏现有 API 兼容性

## Out of Scope
- 不重构函数签名
- 不优化算法
