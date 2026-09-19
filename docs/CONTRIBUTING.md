# 🤝 Crawlrs 贡献指南

欢迎参与 crawlrs 开发！本文档介绍开发环境搭建、开发工作流、代码规范与 PR 提交流程。全部约定来自仓库实际配置（`Cargo.toml`、`.github/workflows/ci.yml`、`scripts/pre-commit-check.sh`、`AGENTS.md`）。

## 📋 目录

- [👋 欢迎](#-欢迎)
- [🧰 环境准备](#-环境准备)
- [🔄 开发工作流](#-开发工作流)
- [🧪 测试与验证](#-测试与验证)
- [📐 代码规范](#-代码规范)
- [🔧 提交 PR](#-提交-pr)
- [🐛 报告问题](#-报告问题)

---

## 👋 欢迎

crawlrs 是使用 Rust 构建的企业级网页数据采集平台。无论是修复 Bug、新增引擎、完善文档还是改进测试，都欢迎提交 [Issue](https://github.com/Kirky-X/crawlrs/issues/new) 或 [PR](https://github.com/Kirky-X/crawlrs/pulls)。安全问题请勿公开提交，见 [🔒 安全文档 · 漏洞报告流程](SECURITY.md)。

---

## 🧰 环境准备

### 前置条件

| 依赖 | 版本 | 用途 |
|------|------|------|
| Rust | 1.97+（`Cargo.toml` `rust-version`；stable 即可） | 工具链 |
| protoc | 任意较新版本 | sdforge 构建依赖（CI 用 taiki-e/install-action 安装） |
| PostgreSQL | 16+ | 集成测试（testcontainers 自动拉起，或外部实例） |
| Docker | 20+ | testcontainers / docker-compose 测试环境 |
| Python 3 | 3.x（可选） | 运行 `tests/python/` API 测试 |

### 克隆与构建

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

cargo build            # 默认特性（platform + db-postgres）
cargo build --features full   # 全部引擎与能力
```

> 注意：自研基座依赖（dbnexus / confers / garrison / limiteron / oxcache / inklog / sdforge / trait-kit）在 `Cargo.toml` 中以本地 path 引用并通过 `[patch.crates-io]` 统一，完整构建需要同级目录存在对应项目。

### 验证环境就绪

```bash
scripts/pre-commit-check.sh all   # fmt → clippy → check → build → 私钥扫描
```

---

## 🔄 开发工作流

### 分支与提交

```bash
# 同步 main 后创建特性分支
git checkout -b feature/amazing-feature

# 提交信息遵循 Conventional Commits：type(scope): subject
# type ∈ feat / fix / docs / refactor / test / chore（与 git log 现有历史一致）
git commit -m 'feat(engines): 添加惊人功能'
```

### 实现要求（硬性规则）

- 禁止占位实现：不允许 `TODO` / `FIXME` / `unimplemented!()` / `todo!()`。
- 处理全部错误路径与边界条件；对外输入做校验。
- 使用项目既有基础设施（真实 PostgreSQL、oxcache 等），日志统一走 `log::` facade（`log::info!` 等，**不是** tracing）。
- 为新功能编写测试；公共 API 添加文档注释。

### 规格驱动变更（可选）

本项目使用 specmark 管理较大变更：`specmark/specs/<cap>/spec.md` 存放能力规格，`specmark/changes/<change>/` 存放进行中的变更提案（propose → apply → converge → archive）。小改动无需走此流程。

---

## 🧪 测试与验证

### 提交前验证顺序（与 CI 一致）

```bash
# 1. 格式与静态检查
cargo fmt --all -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings

# 2. 编译（CI check-default / check-full job）
cargo check
cargo check --features "full"

# 3. 单元测试（需 PostgreSQL 16，本地由 testcontainers 自动拉起）
cargo test --features "standard" --lib
cargo test --features "full" --lib

# 4. 集成与全部测试目标
cargo test --features "full,test-mocks" --tests --no-fail-fast

# 5. 供应链与覆盖率
cargo deny check
cargo llvm-cov --features "full" --fail-under-lines 80
```

一键脚本：`scripts/pre-commit-check.sh all`；全量质量套件：`./tests/e2e/e2e-suite.sh`（特性矩阵 + 静态 + 测试 + 集成 + 基准 + 报告）。场景与命令的完整清单见 [🧪 测试场景矩阵](TEST_SCENARIOS.md)。

### CI 检查项（PR 必过）

`ci.yml` 包含 9 个 job：check-default、check-full、feature-matrix（7 组合 + garrison 门控断言 + RSA 前提校验）、fmt、clippy（standard/full）、test（Rust 1.95/stable 矩阵）、doc（`cargo doc --features "full" --no-deps`，零警告）、deny、integration-test、coverage（行覆盖 ≥ 80%）。

---

## 📐 代码规范

- 格式化以 `cargo fmt` 为准；clippy 全量告警视为错误（`-D warnings`，含 `--all-targets`，`[lints.rust.unexpected_cfgs]` 为 deny 级）。
- 并发原语约定：短临界区用 `parking_lot`（非 tokio 同步原语）；指标计数用 `DashMap`。
- 架构约定：遵循 DDD 四层（presentation → application → domain → infrastructure）；DI 组件在 `src/di/` 的各 capability Module（`SettingsModule`/`CacheModule`/`EngineModule` 等）装配；新增引擎实现 `ScraperEngine` trait 并接入 `EngineRouter`。
- 新特性必须正确 feature 门控：业务能力关闭时提供 Noop 实现，并在 `tests/e2e/feature-matrix.sh` 矩阵中覆盖相应组合。
- 提交粒度：一个逻辑变更一个 commit，避免混入无关格式化。

---

## 🔧 提交 PR

1. Fork 仓库并创建特性分支（`feature/xxx` 或 `fix/xxx`）。
2. 完成实现与测试，本地通过「提交前验证顺序」全部命令。
3. Push 分支并开启 PR（base：`main` 或 `develop`），描述变更动机、影响面与测试情况。
4. 等待 CI 全绿；如 feature-matrix 断言失败，检查新特性是否正确门控。
5. 维护者 review 后合并；合并前保持与 main 同步（CI 配置了按 ref 取消的并发组）。

---

## 🐛 报告问题

- **Bug**：[新建 Issue](https://github.com/Kirky-X/crawlrs/issues/new)，附版本（`/v1/version` 或 release 号）、特性组合、复现步骤与日志（脱敏后）。
- **功能建议**：[Issue](https://github.com/Kirky-X/crawlrs/issues/new) 描述场景与期望行为；大能力建议先走 specmark propose。
- **安全漏洞**：邮件 [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com)，勿公开披露，流程见 [🔒 安全文档](SECURITY.md)。
