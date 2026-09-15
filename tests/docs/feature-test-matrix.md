# Crawlrs 特性组合测试矩阵

> 初始化日期：2026-09-10；最近校准：2026-09-14（全量回归 ALL GREEN 后）。
> 对齐 `Cargo.toml` `[features]` 与 `.github/workflows/ci.yml`。
> 执行入口：`tests/e2e/e2e-suite.sh`（本矩阵的自动化编排，见文末）。

## 1. 特性依赖分析

### 1.1 独立 feature（原子能力）

| Feature | 引入的依赖 | 说明 |
|---|---|---|
| `content` | aho-corasick, htmd | 反爬检测 + HTML→Markdown |
| `trafilatura` | rs-trafilatura | 正文提取主路径 |
| `dom-smoothie` | dom_smoothie | 正文提取性能回退 |
| `metrics` | metrics, metrics-exporter-prometheus, sysinfo | Prometheus 指标 |
| `llm` | genai | LLM 能力 |
| `webhook` | —（纯代码门控） | Webhook 投递与 /v1/webhooks 路由 |
| `engine-flaresolverr` | —（纯代码门控） | FlareSolverr 引擎 |
| `test-mocks` | — | 激活 `tests/main.rs` / `sdk_api_test` 两个测试目标 |
| `http` | sdforge/http | `#[forge]` 宏生成 HTTP 路由注册代码 |

### 1.2 聚合 feature（依赖边）

```
default ──> platform
platform ──> teams ──> auth ──> {dep:garrison, dep:inventory}
         ├──> rate-limit ──> dep:limiteron
         ├──> webhook
         ├──> metrics
         ├──> content
         ├──> http ──> sdforge/http
         └──> dep:{dbnexus, dbnexus-macros, axum, sdforge} + inklog/http
db-postgres / db-sqlite / db-mysql ──> 透传 dbnexus + garrison + limiteron + inklog 驱动（互斥）
standard ──> engine-playwright, metrics, content
full ──> standard, engine-flaresolverr, extractors, llm
extractors ──> trafilatura, dom-smoothie
engine-mllm ──> engine-playwright, llm
agent-lib ──> content, trafilatura, dom-smoothie
engine-playwright ──> dep:{chromiumoxide, chromiumoxide_fetcher}
engine-tls-fingerprint ──> dep:wreq
```

### 1.3 硬约束（编译期互斥/门控）

- 二进制 `crawlrs` 要求 `platform`；`--no-default-features` 且无平台面 feature 时自动排除 bin。
- 集成测试 `integration_tests` 要求 `full`；`main` / `sdk_api_test` 要求 `test-mocks`；`route_diag_test` 要求 `platform`。
- `teams` 隐含 `auth`，`auth` 隐含 `dep:garrison`（认证引擎迁移后唯一实现）。
- **数据库后端（2026-09-15 起三驱动可选）**：`db-postgres` / `db-sqlite` / `db-mysql` 三组特性
  透传全链路驱动选择（dbnexus + garrison + limiteron + inklog），`default = ["platform", "db-postgres"]`。
  互斥由 dbnexus 编译期强制（embedded 与 server-side 不可混、postgres/mysql 互斥），crawlrs 侧
  `src/lib.rs` 另有 platform 缺驱动 / db-* 混用的清晰 compile_error 守卫。`platform` 不含驱动，
  必须搭配恰好一个 `db-*`。
  **运行时注意**：`migrations/*.sql` 为 PG 专用 DDL——sqlite/mysql 的测试运行时供给需要独立的
  schema 层（后续工作），当前三驱动轴为编译级覆盖。
- **Redis / Keycloak OIDC**：代码库无任何客户端集成（全库检索零命中），不设服务轴。

## 2. 编译矩阵（Stage 1 — `tests/e2e/feature-matrix.sh`）

| # | 组合 | cargo 参数 | 覆盖意图 |
|---|---|---|---|
| 1 | bare | `--no-default-features` | 空特性编译健全性 |
| 2 | agent-lib | `--no-default-features --features agent-lib` | 嵌入式最小库面 |
| 3 | agent-lib+playwright | + `engine-playwright` | 浏览器引擎门控 |
| 4 | agent-lib+tls-fp | + `engine-tls-fingerprint` | wreq/BoringSSL 门控 |
| 5 | agent-lib+mllm | + `engine-mllm` | mllm→playwright+llm 传递闭包 |
| 6 | agent-lib+llm | + `llm` | genai 单独门控 |
| 7 | agent-lib+metrics | + `metrics` | 库面指标门控 |
| 8 | content-only | + `content` | 反爬/Markdown 单独 |
| 9 | trafilatura-only | + `trafilatura` | 单提取器 |
| 10 | dom-smoothie-only | + `dom-smoothie` | 单提取器 |
| 11 | teams-only | + `teams` | teams→auth→garrison 闭包 |
| 12 | auth-only | + `auth` | garrison 引入 |
| 13 | rate-limit-only | + `rate-limit` | limiteron 门控 |
| 14 | webhook-only | + `webhook` | 纯代码门控 |
| 15 | platform+db-sqlite | + `platform,db-sqlite` | SQLite 驱动面（embedded 组） |
| 16 | platform+db-mysql | + `platform,db-mysql` | MySQL 驱动面（server-side 组） |
| 16 | default | `--features default` | 默认发布面 |
| 17 | standard | `--features standard` | 引擎+指标+内容 |
| 18 | full | `--features full` | 全功能 |
| 19 | full+tls-fp+mllm | `--features full,engine-tls-fingerprint,engine-mllm` | 引擎全闭包 |
| 20 | default+test-mocks | `--features default,test-mocks --all-targets` | 全测试目标编译 |
| 21 | full+test-mocks | `--features full,test-mocks --all-targets` | CI 集成测试面 |
| 22 | workspace | `cargo check --workspace` | examples crate |

## 3. 行为测试矩阵（Stage 3/4 — 测试目标与维度）

### 3.1 正常路径

| 层 | 测试目标 | 特性要求 | 中间件 |
|---|---|---|---|
| 领域/应用/引擎单元 | `cargo test --lib` | default / full 各跑一轮 | 无 |
| 表驱动机（mock 注入） | `cargo test --test main` | test-mocks | 无（trait 假实现） |
| SDK 面 | `cargo test --test sdk_api_test` | test-mocks | 无 |
| 路由诊断 | `cargo test --test route_diag_test` | platform | 无 |
| 仓库集成（真实 PG） | `cargo test --test integration_tests` | full | PostgreSQL |
| 认证端到端 | `-- --ignored auth_garrison_test` | full | PostgreSQL |
| TLS 指纹引擎 | `-- --ignored wreq_fingerprint_test` | full,engine-tls-fingerprint | 出网 |

### 3.2 异常与边界（已内建于上述测试目标）

| 维度 | 覆盖位置 |
|---|---|
| 网络超时/失败 | `tests/unit`（wiremock 假服务、engine 错误路径）、`tests/common/mocks` |
| DB 连接失败 | `tests/unit/infrastructure/database/dbnexus_connection_test.rs`（无效 URL/重试耗尽）、`repositories/*_test.rs` 错误路径 |
| Token 伪造/认证绕过 | `tests/integration/auth_garrison_test.rs`、`tests/unit/presentation`（middleware） |
| 并发竞争 | `tests/unit/queue`、`tests/unit/workers`（AtomicU32 调用计数、tokio test-util） |
| 配置错误 | `tests/unit/config`、confers 校验 |
| SSRF 防护 | `tests/unit`（security）、handler 前置校验 + 引擎路由双重校验 |

### 3.3 后端组合轴

- 数据库：PostgreSQL 16（唯一支持后端，见 §1.3）。testcontainers 自动拉起（`src/common/test_helpers.rs`，
  进程级共享容器 + 迁移自动应用），或外部 `TEST_DATABASE_URL`（docker-compose.test.yml 的 `test-db`）。
- 中间件需求结论：**仅需 PostgreSQL**；Chrome/FlareSolverr 为可选浏览器 profile；Redis/Keycloak 不适用。

## 4. 环境与清理

- 优先 testcontainers（自动、零残留，Docker 不可达时相关用例自动 skip）。
- 外部环境用 `docker/docker-compose.test.yml`：`docker compose -f docker/docker-compose.test.yml up -d test-db`
  → 测试 → `down -v`（含卷清理）。E2E 套件以 trap 保证异常路径也执行清理。

## 5. E2E 套件（固化编排）

`tests/e2e/e2e-suite.sh` 依次执行：

1. **Stage 1** 特性编译矩阵（`tests/e2e/feature-matrix.sh`，22 组合）
2. **Stage 2** 静态检查：`cargo fmt --check` → clippy（default/full/agent-lib，`--all-targets -D warnings`）→ `cargo deny check`
3. **Stage 3** 单元+mock 测试：`--lib`（default/full）、`--test main`、`--test sdk_api_test`、`--test route_diag_test`
4. **Stage 4** 集成测试：`--test integration_tests --include-ignored`（含 garrison 认证端到端；该目标
   `required-features = full,test-mocks`，test-mocks 门控全局态重置 API）
5. **Stage 5** 基准编译+短跑（`cargo bench` 缩短采样参数）；首次建立 `e2e-baseline`，
   之后自动对比基线检测回退
6. **Stage 6** 汇总报告 `test-results/e2e-report.txt`；EXIT trap 清理 compose 容器/卷并
   清扫泄漏的 testcontainers 容器

任一 Stage 失败即失败退出；日志集中在 `test-results/`；单阶段超时护栏
`E2E_STAGE_TIMEOUT`（默认 1800s）防并行挂起阻塞流水线。

## 6. 已定位并修复的关键问题（2026-09-14 校准轮）

| 问题 | 根因 | 修复 |
|---|---|---|
| 任意特性组合下 lib 测试偶发/批量失败（451→168→53→0） | dbnexus 池内每个 `DbConnection` 是完整 sea-orm/sqlx 池，其后台任务绑定**创建时 runtime**；`#[tokio::test]` 每用例一 runtime 且结束即销毁，跨用例共享池/预热池的连接在复用时后台任务已死，查询挂满 acquire_timeout | `create_test_db_pool` 改为**每调用独立池**：`min_connections=0`（禁预热）、`max_connections=4`、acquire 30s；5 个 handler 测试的本地 `make_test_db_pool` 副本统一委托该 helper |
| auth-only/teams-only 编译失败 | `garrison_interface` 无条件导入仅 platform 引入的 `dbnexus` | 该模块随 `platform` 门控（唯一构造方 bootstrap::services 同为 platform） |
| 集成测试 garrison 用例 "global DAO already injected" | garrison 单例同进程只允许注入一次，3 个 ignored 用例顺序运行第二个即失败 | `test-mocks` 门控暴露 `reset_garrison_global_state_for_integration_test()`；setup 持 `GARRISON_TEST_LOCK` 重置后初始化 |
| `test_rate_limit_returns_429` 恒 401 | ① 生产从未注入 garrison IP task-local（防火墙 fail-open 永不生效）；② 封禁在 `record_failure` 落库，但 `is_blocked` 只在下次请求预检 | 新增 `garrison_ip_context_middleware`（ConnectInfo → `with_current_ip`，挂载于认证中间件外侧，三处路由+SDK）；失败计数后立即复查封禁即拒；`FirewallBlocked` 对外映射 429（RFC 6585） |
| `tc_readiness_*` 偶发 `ConnectionClosed` | WSL2 Docker Desktop 高负载下端口转发持续断连 | `start_db_with_pool`：池创建连续失败→重启容器取新端口→仍失败显式 `[skip]` 降级 |
| clippy `--all-targets` 68 告警 | CI 历史仅 lint lib 面 | 全部修复（bool_assert/redundant_field_names/type_complexity/octal_escapes 等） |
| workspace/examples 编译失败 | DbConfig 结构演进（pool_config 拆分）、search 旧 API 残留 | 示例跟进新 API；删除已移除的 Parallel 搜索引擎引用 |

## 7. 运行手册速查

```bash
./tests/e2e/e2e-suite.sh                    # 全量（矩阵+静态+测试+集成+bench+报告）
./tests/e2e/e2e-suite.sh static unit        # 只跑指定阶段
./tests/e2e/e2e-suite.sh --skip matrix      # 跳过耗时阶段
TEST_DATABASE_URL=postgres://... ./tests/e2e/e2e-suite.sh   # 复用外部 PG（自动应用 migrations）
```
