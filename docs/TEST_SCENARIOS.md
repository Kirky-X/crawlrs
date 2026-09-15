# 🧪 Crawlrs 测试场景矩阵

> 适用版本：crawlrs **0.2.0**（workspace，`Cargo.toml` `rust-version = "1.97"`，edition 2021）
> 数据来源：`Cargo.toml`、`.github/workflows/ci.yml`、`tests/`、`tests/docs/feature-test-matrix.md`（2026-09-14 校准）与 `benches/benchmark.rs` 实际代码。

## 📋 目录

- [🎯 测试体系总览](#-测试体系总览)
- [🗂️ 套件清单](#️-套件清单)
- [✅ 单元测试场景](#-单元测试场景)
- [🔌 集成与端到端场景](#-集成与端到端场景)
- [🧩 特性组合矩阵](#-特性组合矩阵)
- [▶️ 运行手册](#️-运行手册)
- [📊 统计汇总](#-统计汇总)

---

## 🎯 测试体系总览

| 层 | 位置 | 中间件需求 | 说明 |
|---|---|---|---|
| L1 内联单元测试 | `src/`（`#[cfg(test)]` 模块） | 无 | 覆盖 domain / application / engines / infrastructure 等全部模块 |
| L2 模块单元测试 | `tests/unit/`（71 个文件） | 无（wiremock 假服务） | 按 presentation / application / domain / engines / search / workers / queue / config / di / utils 分域组织 |
| L3 表驱动机 | `tests/main.rs`（`[[test]]` name=`main`） | 无（trait 假实现） | `test-mocks` 门控，共享 mock 位于 `tests/common/mocks/` |
| L4 SDK API 测试 | `tests/sdk_api_test.rs` | 无 | `test-mocks` 门控 |
| L5 路由诊断 | `tests/route_diag_test.rs` | 无 | `platform` 门控，验证路由注册与门控一致 |
| L6 集成测试 | `tests/integration/mod.rs`（`[[test]]` name=`integration_tests`） | PostgreSQL 16（testcontainers 自动拉起或外部 `TEST_DATABASE_URL`） | `full,test-mocks` 门控；含仓库层与 garrison 认证端到端 |
| L7 质量保障套件 | `tests/e2e/`、`tests/python/`、`tests/stress/`、`benches/` | 视阶段而定 | E2E 编排、Python API/性能测试、k6 压测脚本、Criterion 基准 |

**环境硬约束**（与 `tests/docs/feature-test-matrix.md` §1.3 一致）：

- 中间件仅需 **PostgreSQL 16**；Chrome / FlareSolverr 为可选浏览器 profile。
- 代码库无 Redis / Keycloak OIDC 客户端集成，不设相应服务轴。
- `migrations/*.sql` 为 PG 专用 DDL；`db-sqlite` / `db-mysql` 当前为编译级覆盖。

---

## 🗂️ 套件清单

| 套件 | 入口 | 特性要求 | 触发环境 |
|---|---|---|---|
| lib 单元测试（standard） | `cargo test --features "standard" --lib` | default（=platform+db-postgres） | CI test job / 本地 |
| lib 单元测试（full） | `cargo test --features "full" --lib` | full | CI test job / 本地 |
| 表驱动机 | `cargo test --test main` | test-mocks | CI integration-test job（`--tests`）/ e2e Stage 3 |
| SDK API | `cargo test --test sdk_api_test` | test-mocks | 同上 |
| 路由诊断 | `cargo test --test route_diag_test` | platform | 同上 |
| 集成测试 | `cargo test --test integration_tests` | full,test-mocks | CI integration-test job / e2e Stage 4（`--include-ignored`） |
| 认证端到端 | `cargo test --test integration_tests -- --ignored auth_garrison_test` | full | 需真实 PostgreSQL |
| TLS 指纹引擎 | `cargo test --test integration_tests -- --ignored wreq_fingerprint_test` | full,engine-tls-fingerprint | 需出网 |
| Python API/性能 | `./scripts/run-tests.sh local` | 运行中的服务实例 | 本地/手动 |
| E2E API 冒烟 | `tests/e2e/api_test.sh` | 运行中的服务实例 | 手动 / liveapi 阶段自动 |
| API 语义级验证 | `tests/e2e/api_semantics_test.py` | 运行中的服务实例 | liveapi 阶段自动 / 手动 |
| 活体 API E2E 编排 | `tests/e2e/live-api-e2e.sh` | Docker（compose PG） | 手动 / liveapi 阶段 |
| E2E 质量套件 | `./tests/e2e/e2e-suite.sh` | 全部 | 本地/手动，7 阶段 |
| 特性编译矩阵 | `tests/e2e/feature-matrix.sh` | 全部 | e2e Stage 1 |
| 手动真爬验证 | `tests/manual/test_real_crawl.py` | 运行中的服务实例 | 手动 |
| 压测 | `tests/stress/k6_script.js` | 运行中的服务实例 | `k6 run tests/stress/k6_script.js` |
| 基准测试 | `cargo bench` | default | CI 无独立 job；e2e Stage 5 短跑 |

共享测试基建位于 `tests/common/`：`factories/`（数据工厂）、`fixtures/`、`helpers/`、`macros/`、`mocks/`（MockTaskRepository / MockScraperEngine / MockWebhookService 等）、`assertions/`、`constants/`。项目无 mock 库——mock 均为真实 trait 实现 + `Arc<AtomicU32>` 调用计数。

---

## ✅ 单元测试场景

### 📌 正常路径

| 域 | 覆盖位置 | 代表场景 |
|---|---|---|
| 配置 | `tests/unit/config/settings_test.rs` | confers 配置加载、默认值、`CRAWLRS__` 环境变量覆盖 |
| 搜索 | `tests/unit/search/`（baidu / bing / sogou / aggregator / client / router） | 引擎解析、结果聚合、去重、路由 |
| 表现层 | `tests/unit/presentation/`（handlers / middleware / helpers / sdk / state） | 各 handler 请求校验与响应、限流与安全头中间件、SSRF 前置校验（`ssrf_mod_test` / `ssrf_redirect_test`）、同步等待（`sync_wait_*_test`） |
| Worker 与队列 | `tests/unit/workers/`、`tests/unit/queue/` | 任务领取/完成/失败路径、worker manager 生命周期、优先级队列 |
| 引擎 | `tests/unit/engines/`（engine_client / reqwest / playwright / browser_downloader） | 引擎选择、support_score 路由、错误传播 |
| 领域与应用 | `tests/unit/domain/`、`tests/unit/application/` | 实体规则、DTO 校验 |
| 基础设施 | `tests/unit/infrastructure/` | 数据库连接（dbnexus）、仓库层逻辑 |
| DI / 引导 | `tests/unit/di/module_test.rs`、`tests/unit/bootstrap/routes_test.rs` | 模块装配、路由注册 |
| 工具 | `tests/unit/utils/`（telemetry / encoding / processor） | 遥测、编码检测、内容处理 |
| i18n | `tests/unit/i18n/` | Fluent 文案加载 |

### ⚠️ 异常与边界（内建于上述测试目标）

| 维度 | 覆盖位置 |
|---|---|
| 网络超时/失败 | `tests/unit`（wiremock 假服务、engine 错误路径）、`tests/common/mocks` |
| DB 连接失败 | `tests/unit/infrastructure/database/dbnexus_connection_test.rs`（无效 URL/重试耗尽）、`repositories/*_test.rs` 错误路径 |
| Token 伪造/认证绕过 | `tests/integration/auth_garrison_test.rs`、`tests/unit/presentation`（middleware） |
| 并发竞争 | `tests/unit/queue`、`tests/unit/workers`（AtomicU32 调用计数、tokio test-util） |
| 配置错误 | `tests/unit/config`、confers 校验 |
| SSRF 防护 | `tests/unit`（security）、handler 前置校验 + 引擎路由双重校验 |

---

## 🔌 集成与端到端场景

### 🏬 仓库层集成（真实 PostgreSQL）

`tests/integration/repositories/`：`tasks_backlog_repo_test`、`scrape_result_repo_test`、`webhook_repo_test`、`credits_repo_test`——覆盖 CRUD、批量写入与错误路径。测试应用由 `tests/integration/helpers/test_app.rs` 构建（真实路由 + 中间件栈）。

### 🔐 认证端到端（`auth_garrison_test`，ignored）

garrison 认证全链路：API Key 签发与校验、RBAC scope 映射、暴力破解锁定（401/429 语义）、多租户隔离。garrison 全局单例经 `test-mocks` 门控的 `reset_garrison_global_state_for_integration_test()` 重置，并用 `GARRISON_TEST_LOCK` 串行化。

### 🧬 TLS 指纹引擎（`wreq_fingerprint_test`，ignored）

WreqEngine 出网验证 JA3/JA4 指纹伪装（需 `engine-tls-fingerprint` 特性）。

### 🌐 Python 端到端（`tests/python/`）

| 套件 | 场景 |
|---|---|
| `test_api_endpoints.py` | 全部 REST 端点冒烟与契约 |
| `test_error_handling.py` | 错误码与异常响应 |
| `test_performance.py` | 响应时间与吞吐基本盘 |
| `test_search_engines.py` / `test_search_comprehensive.py` | 四引擎搜索行为 |
| `conftest.py` + `api_test_framework.py` | 共享 fixture 与 HTTP 客户端封装 |

### 🏋️ 压测与手动

- `tests/stress/k6_script.js`：k6 负载场景。
- `tests/manual/test_real_crawl.py`：真实站点抓取回归（依赖出网）。
- `tests/e2e/api_test.sh`：服务级 API 冒烟脚本。

---

## 🧩 特性组合矩阵

### 🤖 CI feature-matrix job（7 组合，每个 PR 必过）

| 组合 | cargo 参数 | 验证目标 | garrison 期望 |
|------|------|----------|----------|
| no-default | `cargo check --no-default-features --lib` | 全门控就位，无业务能力 + 无引擎 | 不出现 |
| teams-only | `--no-default-features --features teams` | teams→auth→garrison 闭包 | 出现 |
| auth-only | `--no-default-features --features auth` | 仅认证 | 出现 |
| rate-limit-only | `--no-default-features --features rate-limit` | 仅限流 | 不出现 |
| webhook-only | `--no-default-features --features webhook` | 仅 Webhook | 不出现 |
| default | `--features default` | 默认发布面 | 出现 |
| full | `--features full` | 全功能 | 出现 |

每个组合额外执行 `cargo tree` 断言 garrison 仅在 auth-on 时进入依赖树（R-auth-engine-005）；auth-on 组合还断言 `src/` 无 RSA 代码路径（RS256/RS384/RS512/`use rsa::`），保证 `deny.toml` 对 RUSTSEC-2023-0071 的 ignore 前提成立。

### 🧪 E2E 编译矩阵（Stage 1，22 组合）

`tests/e2e/feature-matrix.sh` 按 `tests/docs/feature-test-matrix.md` §2 执行，要点组合：

| # | 组合 | cargo 参数 | 覆盖意图 |
|---|---|---|---|
| 1 | bare | `--no-default-features` | 空特性编译健全性 |
| 2-7 | agent-lib ± 引擎/llm/metrics | `--no-default-features --features agent-lib,...` | 嵌入式最小库面与引擎门控 |
| 8-10 | content / trafilatura / dom-smoothie 单独 | 同上 | 内容处理与提取器门控 |
| 11-14 | teams / auth / rate-limit / webhook 单独 | 同上 | 业务能力闭包 |
| 15-16 | platform+db-sqlite / platform+db-mysql | 同上 | 另两个数据库驱动面 |
| 17-19 | default / standard / full(+tls-fp+mllm) | `--features ...` | 发布面与引擎全闭包 |
| 20-21 | default+test-mocks / full+test-mocks（`--all-targets`） | 同上 | 全测试目标编译 |
| 22 | workspace | `cargo check --workspace` | examples crate |

### 🔒 硬约束

- 二进制 `crawlrs` 要求 `platform`；无平台面 feature 时自动排除 bin。
- `platform` 必须搭配恰好一个 `db-*`；embedded（sqlite）与 server-side（postgres/mysql）不可混、postgres/mysql 互斥（dbnexus 编译期强制 + `src/lib.rs` compile_error 守卫）。
- `teams` 隐含 `auth`，`auth` 隐含 `dep:garrison`。
- `integration_tests` 要求 `full,test-mocks`；`main` / `sdk_api_test` 要求 `test-mocks`；`route_diag_test` 要求 `platform`。

---

## ▶️ 运行手册

### 🔁 与 CI 一致的命令

```bash
# 单元测试（CI test job；需 PostgreSQL 16 + migrations/*.sql，本地由 testcontainers 自动拉起）
cargo test --features "standard" --lib
cargo test --features "full" --lib

# 集成 + 全部测试目标（CI integration-test job）
cargo test --features "full,test-mocks" --tests --no-fail-fast

# Lint / 格式 / 文档 / 供应链
cargo fmt --all -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings
cargo doc --features "full" --no-deps          # CI doc job（RUSTDOCFLAGS=-D warnings）
cargo deny check

# 覆盖率门禁：行覆盖率 ≥ 80%
cargo llvm-cov --features "full" --fail-under-lines 80
```

CI 的 test / integration-test / coverage job 均以 `postgres:16` service 容器为前提：先执行 `for f in migrations/*.sql; do psql ... -f "$f"; done` 应用迁移，再运行测试；`DATABASE_URL` / `TEST_DATABASE_URL` 指向 `postgres://postgres:postgres@localhost:5432/crawlrs_test`。test job 的工具链矩阵为 Rust 1.95 / stable（见 `ci.yml`）。

### 🎼 E2E 套件（7 阶段）

```bash
./tests/e2e/e2e-suite.sh                    # 全量（矩阵+静态+测试+集成+bench+活体API+报告）
./tests/e2e/e2e-suite.sh static unit        # 只跑指定阶段
./tests/e2e/e2e-suite.sh --skip matrix      # 跳过耗时阶段
./tests/e2e/e2e-suite.sh --quick            # 快速矩阵（liveapi 自动跳过）
TEST_DATABASE_URL=postgres://... ./tests/e2e/e2e-suite.sh   # 复用外部 PG（自动应用 migrations）
```

| 阶段 | 内容 |
|---|---|
| Stage 1 matrix | 特性编译矩阵（22 组合） |
| Stage 2 static | `cargo fmt --check` → clippy（default/full/agent-lib，`--all-targets -D warnings`）→ `cargo deny check` |
| Stage 3 unit | `--lib`（default/full）、`--test main`、`--test sdk_api_test`、`--test route_diag_test` |
| Stage 4 integration | `--test integration_tests --include-ignored`（含 garrison 认证端到端） |
| Stage 5 bench | `cargo bench` 缩短采样；首跑建立 `e2e-baseline`，此后自动对比检测回退 |
| Stage 6 liveapi | `tests/e2e/live-api-e2e.sh`：compose 拉起 PG → 迁移 → 构建 → `crawlrs bootstrap`（伪 TTY 捕获 admin key，日志自动掩码）→ 起服（8901 端口）→ 状态码冒烟（`api_test.sh`）+ 语义级验证（`api_semantics_test.py`，40+ 场景：响应包封一致性/安全脱敏/状态转换/数据过滤/并发）→ 产物 `test-results/{api-smoke.log, api-audit.jsonl, api-semantics-matrix.md}` |
| Stage 7 report | 汇总 `test-results/e2e-report.txt`；EXIT trap 清理 compose 容器/卷与泄漏的 testcontainers 容器 |

任一阶段失败即非零退出；日志集中在 `test-results/`；单阶段超时护栏 `E2E_STAGE_TIMEOUT`（默认 1800s）。

单独运行活体 API E2E：

```bash
./tests/e2e/live-api-e2e.sh                 # 全流程（DB→bootstrap→起服→冒烟+语义→清理）
./tests/e2e/live-api-e2e.sh --skip-build    # 跳过 cargo build
KEEP_ENV=1 ./tests/e2e/live-api-e2e.sh      # 结束后保留 DB/服务便于排查
# 已有运行中的服务时，直接指定凭证运行语义验证：
python3 tests/e2e/api_semantics_test.py --base-url http://localhost:8899 \
    --api-key "$KEY" --team-id "$TEAM_ID" --audit-log test-results/api-audit.jsonl
```

### 🐳 环境与清理

- 优先 testcontainers（自动、零残留；Docker 不可达时相关用例自动 skip，CI 中另有 `skip_if_no_test_db` 守卫）。
- 外部环境用 `docker/docker-compose.test.yml`：`docker compose -f docker/docker-compose.test.yml up -d test-db` → 测试 → `down -v`（含卷清理）。
- Python 测试：`./scripts/run-tests.sh local`（自动 `pip install -r tests/python/requirements.txt`，结果写 `test-results/`）。

---

## 📊 统计汇总

截至 0.2.0（以 `grep -rE '^\s*#\[(tokio::)?test\]'` 口径统计）：

| 指标 | 数量 |
|---|---|
| `src/` 内联测试（`#[test]` / `#[tokio::test]`） | 约 5,700+（372 个 `.rs` 文件，约 17.7 万行） |
| `tests/` 目录测试 | 约 950 个（其中 `tests/unit/` 795 个、71 个文件） |
| `[[test]]` 显式注册目标 | 4 个（integration_tests / main / sdk_api_test / route_diag_test） |
| Criterion 基准组 | 9 组（`benches/benchmark.rs`） |
| Python 测试套件 | 5 个（`tests/python/`） |
| 特性编译矩阵 | CI 7 组合 + E2E 22 组合 |
| 覆盖率门禁 | 行覆盖率 ≥ 80%（CI coverage job，Codecov 上报） |

> 统计随代码演进浮动，以仓库当前 HEAD 实测为准。
