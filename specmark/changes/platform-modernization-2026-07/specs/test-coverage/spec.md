# Spec — test-coverage

> Delta spec for change `platform-modernization-2026-07`. 覆盖代码覆盖率提升到 90%+ 的需求。

## Requirements

### R-cov-001: 基线覆盖率测量
使用 cargo llvm-cov 测量当前覆盖率基线。

**验收标准：**
- 覆盖率报告生成（lcov 格式）
- 各模块覆盖率数据记录

### R-cov-002: 覆盖率提升到 90%+
为覆盖率不足的模块补充测试，目标行覆盖率 ≥ 90%。

**验收标准：**
- `cargo llvm-cov --features default --lcov --output-path /tmp/coverage-final.lcov` 生成报告
- 总行覆盖率 ≥ 90%
- src/domain/ 覆盖率 ≥ 90%
- src/application/ 覆盖率 ≥ 90%
- src/infrastructure/ 覆盖率 ≥ 85%（mock 测试）
- src/presentation/ 覆盖率 ≥ 85%（axum-test）

### R-cov-003: TDD 测试编写
新测试遵循 TDD 流程（Red→Green→Commit）。

**验收标准：**
- 每个 use case 至少 3 个测试：成功/失败/边界
- 测试验证有意义的行为（值、结构、副作用、错误类型）
- 测试通过 `cargo test --features default --lib`

## Constraints
- 测试不依赖外部服务（PostgreSQL/Redis 用 mock 或 testcontainers）
- 测试执行时间 < 60 秒（单元测试）
- 不为 examples/ 写测试

## Out of Scope
- 不补 e2e 测试（已有 4 个，本次不增加）
- 不做性能基准测试
