# 🧩 crawlrs 特性（Cargo feature）组合矩阵

> 校准日期：2026-11-08。数据来源：`Cargo.toml [features]` 段、`config/default.toml`、
> `src/main.rs`（CLI 分发）、`src/bootstrap/route_groups/`（feature 门控路由）。
> 本文回答两个问题：**哪些组合能编译**（互斥面），**哪些组合在启动期改变行为**
> （fail-fast / fail-closed / 降级注入面）。

## 1. 特性清单

| Feature | 作用 | 依赖 | default | full |
|---|---|---|---|---|
| `platform` | 完整平台服务面（DB/HTTP/多租户/认证/限流/Webhook/metrics/content/sitemap） | teams auth rate-limit webhook metrics content sitemap + dbnexus/axum | ✅ | — |
| `teams` | 多租户（隐含 `auth`） | auth | ✅(经 platform) | — |
| `auth` | garrison 认证引擎（JWT） | dep:garrison dep:inventory | ✅(经 platform) | — |
| `rate-limit` | 限流开启；关闭时注入 NoopRateLimitingService | limiteron（非可选） | ✅ | — |
| `webhook` | webhook 路由与服务；关闭时移除 `/v1/webhooks` 并注入 Noop | — | ✅ | — |
| `metrics` | Prometheus 指标 | dep:metrics + prometheus exporter + sysinfo | ✅ | ✅(经 standard) |
| `content` | Markdown 转换链（moli-html2md 主 + htmd 兜底） | dep:aho-corasick 等 | ✅ | ✅(经 standard) |
| `sitemap` | sitemap 流式请求源 + `/v1/map` 路由 | dep:quick-xml | ✅ | ✅(经 standard) |
| `http` | `#[forge]` 路由注册使能（sdforge/http） | sdforge | ✅(经 platform) | — |
| `standard` | 引擎+内容标准面 | engine-playwright metrics content sitemap | — | ✅ |
| `full` | 全功能（standard + flaresolverr + extractors + llm） | standard engine-flaresolverr extractors llm | — | ✅ |
| `engine-playwright` | CDP 浏览器引擎 | dep:chromiumoxide | — | ✅(经 standard) |
| `engine-flaresolverr` | FlareSolverr 外置服务引擎（full/cdp/tls 三变体配置） | — | — | ✅ |
| `engine-moli` | 纯 Rust 无头引擎（CDP 端点模式，内存水位调度） | dep:sysinfo dep:chromiumoxide | — | — |
| `engine-tls-fingerprint` | wreq TLS 指纹引擎（BoringSSL，JA3/JA4 伪装） | dep:wreq | — | — |
| `engine-mllm` | 视觉 LLM 自主导航引擎 | engine-playwright + llm + gateway | — | — |
| `trafilatura` / `dom-smoothie` / `extractors` | 正文提取主/回退/聚合 | dep:rs-trafilatura / dep:dom_smoothie | — | ✅(extractors) |
| `llm` | LLM trait + 提取编排边界（零外部依赖） | — | — | ✅ |
| `rag` / `rag-local` | 嵌入+重排；local 走 vecboost 本地推理 | rag-local + gateway | — | — |
| `gateway` | fluxweir 统一 LLM/嵌入/重排出口（embedded/external） | dep:fluxweir dep:axum | — | — |
| `cache-redis` | oxcache Redis 后端（多实例共享） | oxcache/redis | — | — |
| `openapi` | OpenAPI 3.1 聚合（`GET /v1/sdk/openapi.json`） | http sdforge/openapi dep:utoipa | — | — |
| `forge-full` | inventory 注册面，与手写 protected/v2 路由**互斥** | http | — | — |
| `web-axum` | auth-off 等价构建面 | rate-limit webhook metrics content + dbnexus/axum | — | — |
| `agent-lib` | 嵌入式/agent 最小库面（无 DB/服务端） | content trafilatura dom-smoothie | — | — |
| `test-mocks` | 测试专用（garrison 全局态重置） | — | — | — |

## 2. 编译期互斥（违反即编译失败）

| 约束 | 违反后果 |
|---|---|
| `db-postgres` / `db-sqlite` / `db-mysql` 三选一 | dbnexus embedded×server-side 互斥 `compile_error!` |
| `forge-full` 与手写 protected/v2 路由**并存** | inventory 全局收集冲突 → **panic**（运行期首次收集时） |
| `engine-mllm` 依赖闭包 | 必须连带 `engine-playwright` + `llm` + `gateway`（feature 依赖自动满足） |
| `fluxweir` 引入 | 必须保持其 `default-features=false`（其默认 db-sqlite 会触发上面第 1 条） |

## 3. 启动期强制行为（配置 × 构建交叉）

| 条件 | 行为 | 依据 |
|---|---|---|
| `auth` 开启 + `jwt_secret` 空 / < 32 字节 | **启动 panic**（CWE-326 fail-fast） | `default.toml` [auth] 注释 |
| `auth` 关闭构建 | 注入固定 AuthState；`[auth]` 段仍须存在但值被忽略 | 同上 |
| `[cache] backend=redis` 但未编译 `cache-redis` | 启动 **fail-closed** 拒绝 | Cargo.toml features 注释 |
| `CRAWLRS_DISABLE_SSRF_PROTECTION` 在非 test/development 环境 | SSRF 失效 + 显著 warn | `default.toml` 头部注释 |
| `[id] algorithm=snowflake` 但未编译对应 feature | 启动期 `error-id-algorithm-uncompiled` 显性拒绝 | `default.toml` [id] 注释 |
| `[i18n] default_locale` 未显式改写 | 按检测链 `CRAWLRS_LANG → LC_ALL → LC_MESSAGES → LANG → sys-locale` 决议；非法值回退 en-US | `default.toml` [i18n] 注释 |
| `search.searxng.enabled=true` | 追加 2 个周期 worker（collector 6h / health 300s）+ 探测第三方公共实例（明文查询外发） | `default.toml` [search.searxng] |
| `search.searxng.enabled=false`（默认） | `/v1/searxng/*` 三端点统一 503 | `management_routes.rs` 注释 |
| `[rag]` 全关（默认） | 嵌入/重排链路整体旁路 | `default.toml` [rag] |
| `fluxweir.enabled=true` + `mode=embedded` | 进程内起服并共享 crawlrs dbnexus 连接池 | `default.toml` [fluxweir] |

## 4. 组合矩阵（构建命令 × 预期结果）

| # | 组合 | 命令 | 预期 |
|---|---|---|---|
| 1 | 默认（平台全套 + PG） | `cargo build` | API 服务全路由；auth/webhook/rate-limit/metrics/content/sitemap 开 |
| 2 | 全引擎 | `cargo build --features full` | + playwright、flaresolverr、双 extractor、llm、openapi 无 |
| 3 | 平台 + openapi | `cargo build --features openapi` | `GET /v1/sdk/openapi.json` 可用 |
| 4 | 纯引擎库面 | `cargo build --no-default-features --features agent-lib` | 无 DB/HTTP/服务端；content+extractors 编译 |
| 5 | web-axum（auth-off 面） | `cargo build --no-default-features --features web-axum,db-postgres` | 无认证（固定 AuthState）；限流/webhook/metrics/content 开 |
| 6 | 换 SQLite | `--no-default-features --features web-axum,db-sqlite` | 方言 DDL 走 `migrations/sqlite/` |
| 7 | 换 MySQL | 同上 + `db-mysql` | 方言 DDL 走 `migrations/mysql/` |
| 8 | TLS 指纹引擎 | `--features full,engine-tls-fingerprint` | + wreq 引擎；构建需 cmake + C 编译器；二进制 +~8MB |
| 9 | moli 引擎 | `--features full,engine-moli` | + 内存水位调度器；需外部 `moli serve --layout` 端点 |
| 10 | MLLM 引擎 | `--features full,engine-mllm` | + 视觉 LLM 导航；隐含拉入 gateway |
| 11 | RAG 本地 | `--features full,rag` | + vecboost 本地推理 + fluxweir 兜底链 |
| 12 | 共享缓存 | `--features full,cache-redis` | `[cache] backend=redis` 才生效；未编译时该配置 fail-closed |
| 13 | 表驱动测试 | `cargo test --test main --features test-mocks` | L2/L3 单测面（tests/unit 70 文件入口） |
| 14 | BDD 验收 | `CRAWLRS_ACCEPTANCE_OFFLINE=1 cargo test --test acceptance --features platform` | cucumber 套件（harness=false；Docker 需可用） |
| 15 | 集成 | `scripts/run-live-test.sh integration_tests` | testcontainers PG :5499；`full,test-mocks` + `--include-ignored` |

**合法引擎特性叠加**：`full,engine-moli,engine-tls-fingerprint,engine-mllm` 可同时
编译（各引擎在 EngineRouter 按 `[engines.*] enabled` 运行时开关注册）。

## 5. 服务/测试轴（docker）

`docker/docker-compose.test.yml` 实际服务轴：`test-db`（PostgreSQL）、`chrome`、
`flaresolverr`、`test-runner`。**无 MinIO、无独立搜索服务**——
`tests/feature-matrix/test_config.py` 曾引用 MINIO/SEARCH 轴，已于 2026-11-08
与本文件同步修正（删除不存在服务的轴）。

## 6. 运行时行为开关（feature 编译后仍受配置门控）

| 配置开关 | 默认 | 关闭时行为 |
|---|---|---|
| `[engines.memory_scheduler] enabled` | true | 无内存水位调度（Critical 冻结/Tight 收敛失效） |
| `[concurrency] adaptive_enabled` | false | 固定并发（无 AIMD 回填） |
| `[politeness] autothrottle_enabled` | false | min_interval 恒定 1000ms（无 EMA 收敛） |
| `[session] enabled` | false | 盲轮换代理/UA（无身份三件套记分退休） |
| `[engines.playwright] capture_xhr` | false | 不读 XHR/Fetch 响应体 |
| `[sitemap] enabled` | false | `sitemap_seed: true` 的 crawl 请求被显式拒绝 |
| `[crawl] soft404_enabled` | true | 不学习软 404 指纹 |
| `[observability] failure_snapshot_dir=""` | 空 | 失败快照禁用 |
| `[retention] scrape_results_days=0` | 30 | 0 = 永不清理（旧行为） |
| `[robots] scrape_respect_robots` | false | `/v1/scrape` 默认不遵从 robots（请求级可覆盖；fail-open） |

## 7. 功能面补遗（独立审计 2026-11-08）

穷举清单最初遗漏、经审计确认的用户可达功能面（覆盖现状一并标注）：

| 功能面 | 入口 | 覆盖现状 | 分类（2026-11-08 审计后） |
|---|---|---|---|
| credits 计费（`CreditFlow` 扣费 + 402 QUOTA_EXCEEDED 付费墙） | `src/presentation/handlers/credits.rs:99-104`；scrape/crawl/extract 入口扣费 | 3 条内联 + **e2e 已锚定 402 契约**（`tests/e2e/live-api-e2e.sh:297-302`） | 非缺口——契约已锚定；BDD 复建需外部 limiteron 配额表种子，属重复建设，不做 |
| `/v1/tasks/_cancel` 批量取消（含按 crawl_id） | `src/presentation/routes/task.rs:41-44` | handler 内联 + **BDD 已补**（platform.feature 批量取消场景，断言 `cancelled_tasks`×2） | 已修复并验证 |
| WS 任务状态 `/v1/tasks/{id}/ws` | `src/bootstrap/routes.rs:485-492` | `tests/ws_task_status.rs` 6 tests | 非缺口——清单遗漏，本文档补锚定 |
| 探针语义 `/health/live`（恒 200）vs `/ready`（Unhealthy→503 摘流） | `src/bootstrap/routes.rs:130-138` | health.feature 仅 happy path | 覆盖增强项（非缺陷）：失败分支需 mock registry 注入 |
| SSRF 旁路开关 `CRAWLRS_DISABLE_SSRF_PROTECTION` | `ssrf/mod.rs:318`（`pub(crate)`，OnceLock 缓存 + warn 一次） | 双向均已隐式锚定：unset→false 由全部 SSRF 拒绝类单测看护，set→true 由验收套件全量看护（旁路失效则全部 127.0.0.1 场景皆红） | 非缺口——唯 warn 日志行无独立断言，log 捕获基建 + OnceLock 全局缓存使专项单测不成比例，不做 |
| 本地模型路径遍历防护（`../` / `~` 拒绝） | 声称位于**外部依赖 vecboost**（`default.toml:316` 注释），crawlrs 侧无独立校验（`vecboost_provider.rs:65` 直通 PathBuf） | 本地无 vecboost crate 源缓存，无法验证 | **不可修复项申报（硬外部阻塞）**：解锁前提=本机拉取 vecboost crate 源（`cargo fetch`）或访问上游仓库 |
| searxng collector/health 两个周期 worker | `src/bootstrap/workers.rs:10-11` | 各 2-3 条测试 | 覆盖增强项（非缺陷） |
| lock_heartbeat 锁续期 | `src/workers/lock_heartbeat.rs` | 2 条测试 | 覆盖增强项（非缺陷） |
| CLI bootstrap 分支解析 | `src/main.rs:23` | **已补** `tc_parse_service_type_bootstrap` + 文档注释补齐 bootstrap/migrate 两行 | 已修复并验证 |

## 8. 契约勘误（2026-11-08 审计）

| 项 | 勘误 |
|---|---|
| crawl `max_depth` | 真实上限 **5**（handler 级 `validate_crawl_request`，`crawl_commands.rs:195`）；DTO `range(max=100)` 在裸 `Json` extractor 路径**从不运行**。任何 >5 的值 → 422 "must be between 0 and 5" |
| extract LLM 兜底阈值 | 实际常量 `LLM_FALLBACK_CONFIDENCE = 0.8`（`content_extractor/facade.rs:32`），非 0.7 |
| search `limit` 超限 | **静默钳制**（`search_service.rs:205` `min(max_results)`），非 422 拒绝 |
| scrape vs extract 校验顺序 | scrape：sync_wait → 限流 → SSRF（`scrape_commands.rs:116`）；extract：SSRF → sync_wait（`extract_handler.rs:88-102`）；两域 `sync_wait_ms` 超限状态码不对称（scrape 422 / extract 400） |
| 任务创建类端点 202 | `/v1/crawl`、`/v1/extract`（及 scrape 同步模式）默认带 5s 同步等待；任务未在窗口内完成 → **202 Accepted**（异步续行，body 仍含 `data.id`），非 201。内联锚点：`scrape_handler.rs:735` sync_mode_timeout_returns_accepted |
