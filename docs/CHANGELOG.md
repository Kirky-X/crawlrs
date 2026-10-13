# 📋 Crawlrs 更新日志

本文件记录 crawlrs 面向用户的重要变更摘要。格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/spec/v2.0.0.html)。

> 📌 完整历史（全部条目、行为变化与验证记录）见仓库根 [CHANGELOG.md](../CHANGELOG.md)；本文件仅保留面向用户的摘要。

## 📋 目录

- [Unreleased](#unreleased)
- [0.2.0-rc.2 - 2026-09-21](#020-rc2---2026-09-21)
- [0.2.0-rc.1 - 2026-08-25](#020-rc1---2026-08-25)
- [0.2.0 - 2026-07-29](#020---2026-07-29)
- [0.1.0 - 2026-07-22](#010---2026-07-22)

---

## [Unreleased]

### Breaking（auth-off 安全语义恢复——文档审计缺陷 #6）

- **auth-off protected routes 恢复默认拒绝（401）**：恢复 v0.2.0-rc.1 语义（rc.2 路由装配重构时意外丢失且无记录）。`auth` feature 关闭时，protected 路由默认 401；嵌入方须显式调用 `crawlrs::presentation::middleware::auth_middleware::allow_unauthenticated_protected()` 放行（注入匿名受限身份：`denied()` scope、无 token_hash，一次 `error!` 日志声明降级）。默认构建（platform 隐含 auth）不受影响
- **恢复 `web-axum` 构建面**：platform 隐含 auth，auth-off 的降级路径（`default_identity_middleware` / `allow_unauthenticated_protected`）在 rc.2 之后的特性矩阵中**不可能编译**（presentation 挂 platform → 必含 auth），系 rc.1 移除 web-axum 后的失修面。现按 rc.1 机制恢复：`--no-default-features --features web-axum,db-postgres --lib` 构建/测试 auth-off 语义，auth-off 中间件测试首次可在该轴运行

### Added（文档审计缺陷清偿 #1-#5）

- **`crawlrs migrate` 子命令**（#1）：生产 schema 初始化一等入口，按序应用 `migrations/*.sql`（经 dbnexus `auto-migrate` 特性 `DbPool::run_migrations`）；目录缺省 `./migrations`，`CRAWLRS_MIGRATIONS_DIR` 可覆盖
- **`POST /v1/scrape/{id}/_cancel` 生产路由注册**（#2）：此前 handler 完整实现却仅注册在测试路由，生产 404 无法取消单个抓取任务（仅能批量取消）；补路由组注册 + 注册探活测试。与 `DELETE /v1/crawl/{id}`（0.2.0 引入）并存
- **搜索配置生效**（#3）：`search.default_engine` 与 `[search.engines] bing/baidu/sogou_enabled` 注册开关此前为死配置无代码消费，现由 `SearchClient::with_configured_engines` 真实消费（关闭的引擎不注册）；default.toml / .env.example 同步
- **`ScrapeOptionsDto` 暴露 `block_ads` / `block_media` / `needs_mllm`**（#4）：引擎层能力此前已在、仅 DTO 未暴露；`needs_mllm` 在无 `llm` 特性构建下提交期 422 显性拒绝，不再静默降级
- **forge_api 注册面纳入编译**（#5）：新增 `forge-full` 特性停机坪，模块经 `--features forge-full` 编译并接受路由等价性锚定测试

### Added（i18n 与功能吸收）

- **i18n / Fluent 接入**：42 处用户可见字符串/日志接入 Fluent（en 默认 + zh 键位对齐），ID 模块配置校验与 auth-off 降级日志双语化并经 tr_log 输出，代码层不再持有用户可见字符串
- **功能吸收 F1–F12**：AutoThrottle 礼貌策略（延迟收敛 + 429 指数退避 + Crawl-delay 双时钟 + 停滞检测）、Session 身份三件套、sitemap 流式一等请求源 + 软 404 指纹 + seen 预热差量、失败现场快照 + AIMD 内存压力联动、Playwright XHR/Fetch 后台响应捕获、提取降级阶梯质量门 + LLM token 窗口分块并行 merge、提取结果字符级溯源 + MLLM 混合 AX 树快照、视觉调用自定义 OpenAI 兼容端点
- **引擎健康监控 garrison 化**（auth 门控）：`/health/live` + `/health/ready` 探针（readiness 聚合 Unhealthy → 503 摘流语义）

### Fixed

- **`--features rag` 恢复可编译**：vecboost 0.3.0-rc.1 与 confers 0.6.0-rc.6 加密 API 不匹配（`derive_field_key` 返回 `Zeroizing<[u8; 32]>`、`decrypt` 返回 `Zeroizing<Vec<u8>>`）导致该特性组合无法编译；本地一等仓库 vecboost 适配 rc.6（`../vecboost`），crawlrs 以 `[patch.crates-io]` 指向本地修复版，上游发 rc.2 后可移除
- `dbnexus` 依赖补 `auto-migrate` 特性（migrate 子命令前置）
- **分块/溯源接入活体提取路径**：直调 max_tokens 封顶与可配置超时

### Changed（自研库矩阵、依赖与 ID 体系）

- **自研库吸收沉淀（selfdev-lib-adoption-r1）**：错误机制下沉 sdforge unified 表、DNS miss/single-flight 编排折叠 oxcache `get_or_with_ttl`、布隆过滤器替换 oxcache `BloomFilter`（**行为变化**：mmap 大页驻留策略移除）、引擎健康吸收 garrison `HealthCheck`/`HealthRegistry`、熔断薄封装 limiteron `ManualCircuitBreaker`（**行为变化**：滑窗绝对失败计数 → 库累计比例触发）；团队信号量池（S1）判定不可吸收、保留 crawlrs 实现
- **依赖升级与特性显式化清洗**：自研 rc 链跟进至 rc.6/rc.7、三方依赖升最新稳定版（quick-xml 0.42 / rig-core 0.44 / uuid 1.27）、全量特性显式化（关默认特性 + 最小特性集）、CI actions 升级；新增 `web-axum` / `forge-full` 特性轴与 vecboost `[patch.crates-io]`
- **ID 体系全量迁移至 NebulaId**：默认算法 `uuid_v8` 生成 UUID 兼容值，API JSON 契约与 DB `UUID` 列零改动、无 schema 迁移；`snowflake` 为可配置备选；配置新增 `[id]` 段

> 以上各节完整条目、钉版佐证与验证记录（含 fix-audit-mediums-r3 / r2 / defects-r1 审计修复全量清单）见仓库根 [CHANGELOG.md](../CHANGELOG.md)。

### Security

- **SSRF 旁路接通**：`CRAWLRS_DISABLE_SSRF_PROTECTION` 启动告警此前已就位但 `SsrfValidator` 仍按黑名单 + 内网判定拦截、开关形同虚设，现已接通（仍解析 DNS 维持 ValidatedUrl / 引擎 DNS-pin 形状，仅跳过黑名单与内网判定，重定向策略降级为普通跟随；进程内 warn 一次 + 非 dev/test 环境启动期显著告警）
- **CWE-798 回归测试**：历史硬编码默认 webhook secret 被原样配进生产同样视为未设置、必须阻断启动

### Removed

- **jsonwebtoken 死依赖移除**：src 全仓零引用，连带 rsa 0.9.10（RUSTSEC-2023-0071）退出 agent-lib 消费方依赖树；JWT 能力由 garrison（auth feature）自带，不受影响

## [0.2.0-rc.2] - 2026-09-21

### Changed
- **自研依赖全面切换 crates.io registry**：confers/inklog/dbnexus/sdforge/oxcache/limiteron → 0.x.0-rc.5、trait-kit → 0.5.0-rc.6、garrison → 0.9.0-rc.2、vecboost → 0.3.0-rc.1；移除全部 `../base`、`../garrison` 跨仓 path 依赖与 `[patch.crates-io]` 段，CI 不再依赖兄弟仓 checkout
- 合并 github/main 双远端分叉历史（文档双远端对齐、BDD 验收线素材保留于历史）

### Fixed
- `audit_service` 错误映射：`RepositoryError::DatabaseError` 双态（platform 重建 `DbErr::Custom`，轻量面 `Database(String)`），保住 `Database` 变体语义
- i18n locale 测试环境竞态（`CRAWLRS_LANG` 进程级 env 经 ENV_MUTEX 串行化 + 显式清理）
- rustdoc 1.98 严格模式 88 处（裸链接转代码字体/全路径、懒续行缩进、裸泛型 HTML 转义）
- clippy 1.98：`result_large_err` 显式豁免、`cmp_owned` 消除

## [0.2.0-rc.1] - 2026-08-25

### Added
- `web-axum` feature：auth-off 的 Web 中间件构建面（platform 隐含 auth 导致原 feature 矩阵中该路径不可构建，现可独立构建/测试）
- `allow_unauthenticated_protected()` 显式 opt-in（auth-off 面）

### Changed
- **auth-off 安全语义收紧**：protected routes 默认拒绝（401），移除 `DEFAULT_IDENTITY_TOKEN_HASH` 占位哈希与固定全量身份注入；opt-in 后注入匿名受限身份（`denied()` scope、无 token_hash）
- `NoopRateLimitingService`/`NoopWebhookService` 装配日志升级为 error 级一次性（安全语义降级显性化）
- sdk 路由测试收敛至单元层（tests/sdk_api_test.rs 与 src/presentation/sdk/tests.rs 语义重复用例移除）
- `wreq` 依赖声明改 `0.16` 占位（上游事故：5.x 全部 yank、6.0.0-rc 构建破裂；`engine-tls-fingerprint` 启用需按 0.16 API 适配）

### Fixed
- `exe_path` 跨平台命名（Linux 面 `_exe_path` 引用错误）
- `WorkerManagerDeps` 三个新字段的测试构造缺失（补齐最小测试替身）
- clippy `-D warnings`：result-large-err / struct update no-op / unused variable
- rustdoc 链接债务（crate 级 lint 声明）
- cargo-deny：lru 升级 0.18.2；paste 传递依赖豁免登记（RUSTSEC-2024-0436/2026-0258）

### Breaking
- auth-off 下 protected routes 默认 401（曾依赖未鉴权全开的嵌入方需显式 opt-in；注：rc.2 装配重构期间该收紧一度丢失，已于 Unreleased 恢复，见 Unreleased → Breaking）

## [0.2.0] - 2026-07-29

### Added

- Garrison RBAC 集成：使用 garrison `ApiKeyHandler::verify_with_namespace` 直接获取 `login_id` 与 `scopes`，废弃 `GarrisonUtil::check_api_key` + session 查找链路（解决 garrison 内存 DAO 下 session 缺失导致的 login_id=None 401 问题）
- `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 环境变量：服务启动时注入 bootstrap admin key，同时写入 garrison DAO 与 crawlrs `api_keys` 表 + 初始化团队，解决 garrison 内存 DAO 的跨进程 key 丢失
- Axum 中间件链路校准：`create_v2_routes_with_state` 中先 `.layer(team_semaphore_middleware)` 再 `.layer(auth_middleware_inner)`（后 .layer = outer = 先执行），确保 `AuthState.team_id` 在信号量提取前已注入
- `team_semaphore_middleware`：`extract_team_id` 改为从 `AuthState` 读取 `team_id`（替代直接读取裸 `Uuid` Extension），修复 v2 路由 401
- API 端点 `DELETE /v1/crawl/{id}`（取消 crawl）；`POST /v1/scrape/{id}/_cancel` 此阶段退出生产路由（后于 Unreleased 重新注册，见 Unreleased → Added 文档审计缺陷 #2）
- 自研依赖版本统一对齐（共享 rc.43 链）：confers 0.6.0-rc.2 / dbnexus 0.6.0-rc.2 / oxcache 0.5.0-rc.2 / trait-kit 0.5.0-rc.2 / sdforge 0.5.0-rc.2 / inklog 0.3.0-rc.2 / limiteron 0.3.0-rc.2 / garrison 0.9.0-rc.1

### Changed

- 认证失败路径：所有 protected routes 不再走 garrison `TokenSession` 会话查找（因 garrison `check_api_key` 不创建 session）
- SDK router (sdforge 0.5) 的 auth 中间件统一使用 `from_fn_with_state(db_pool, auth_middleware_inner)`，与 protected/v2 三端一致
- 公开端点 `PUBLIC_ENDPOINTS` 比较时已规范化尾部斜杠（`/health/` 也命中）

### Fixed

- v2 路由 401 Unauthorized：组合根因——(a) team_semaphore 在 auth 前执行 (b) team_id 从裸 `Uuid` Extension 读取而非 `AuthState`
- garrison `INVALID_TOKEN`：gen_admin_key 工具写入的 key 在另一个进程不可见（内存 DAO 进程隔离），改为 bootstrap key 从环境变量注入
- `create_protected_routes_with_state` 与 `routes/handlers.rs` 的并行 `routes()` 路由表解耦：当前 API 以 create_protected_routes_with_state / create_v2_routes_with_state 为权威（禁止向后兼容）

### Security

- 暴力破解防护（CWE-307）：所有 protected 请求中继续应用 garrison IP 级失败计数与锁定期
- auth_middleware_inner 的 cache、key_expiration、scope 校验均保持严格模式，无降级分支

## [0.1.0] - 2026-07-22

### Added

- Enterprise-grade web scraping and crawling platform built with Rust
- Multi-engine support: Reqwest (HTTP), Playwright (JS rendering via chromiumoxide), FlareSolverr, WreqEngine (TLS fingerprint), MllmEngine (Vision LLM navigation)
- Search engine aggregation (Google, Bing, Baidu, Sogou)
- Garrison-based authentication (RBAC + JWT + firewall-bruteforce + audit-log)
- API key scope system (Read / Write / Admin) with team isolation
- Credits-based billing system for search/scrape/crawl/extract operations
- Geographic restriction enforcement (country allow/block lists, IP whitelist)
- Comprehensive SSRF protection with DNS resolution validation
- Task queue with priority scheduling and retry support
- Webhook delivery system for event notifications
- Audit logging for all API operations
- Rate limiting with circuit breaker support (limiteron)
- LRU cache for API key validation (TTL 120s, capacity 10000)
- Extraction engine with LLM support (genai integration)
- Admin CLI tools for credits management

### Security

- Constant-time comparison for HMAC signature verification
- SSRF validation on all URL-accepting endpoints (scrape/crawl/extract)
- Proxy URL SSRF validation in search and crawl paths
- IDOR protection on audit logs endpoint (CWE-862)
- IP-based brute-force lockout (5 failures → 15min lockout)
