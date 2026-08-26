# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **HTTP 路由单一注册面（sdforge inventory）**: 全部 19 个业务端点（scrape/search/crawl/webhook/extract/teams/audit/admin/tasks）从 `bootstrap/route_groups` 手写装配迁移至 `presentation::forge_api` 经 sdforge inventory 编译期注册——15 个端点走 `RouteRegistration` 直注（既有 handler 原样组装，覆盖 JSON body 与同路径多方法资源如 `GET+DELETE /v1/crawl/{id}`），4 个无 body GET 端点走 `#[forge]` 宏；`build_forge_router()` 为进程内唯一 `sdforge::http::build()` 调用点。URL/方法/状态码/响应体 100% 向后兼容；team 信号量改为路径条件中间件在 `/v1/tasks/*` 上保序执行

### Added

- **TLS 指纹伪装引擎** (`engine-tls-fingerprint`): WreqEngine 基于 BoringSSL 实现真实 JA3/JA4 指纹伪装，支持 Chrome/Firefox/Safari/Edge 指纹模拟
  - `UaProfile` 新增 `tls_emulation` 字段，UA↔TLS 指纹一致性绑定
  - `needs_tls_fingerprint` 请求参数和 Scrape API options 字段
  - `[engines.tls_fingerprint]` 配置段
- **MLLM 自主导航引擎** (`engine-mllm`): 视觉 LLM agentic loop 实现自主页面导航
  - `MllmDecision` 枚举（Click/Scroll/Input/Wait/Extract/Done）+ JSON 解析器
  - `VisionAdapter` 视觉适配器（截图→视觉模型请求）
  - `ActionExecutor` 动作执行器（MLLM 决策→CDP 操作）
  - `MllmEngine` struct + `ScraperEngine` trait 实现
  - `[engines.mllm]` 配置段（vision_model/max_iterations/screenshot_quality/max_token_budget/mrt_seconds）
- **RAG 增强提取**: DOM 语义分块 + 向量嵌入 + 余弦相似度检索 + LLM 精确提取
  - `SemanticChunker`: 按 DOM 边界（article/section/div/table）分块，表格/列表不截断
  - `EmbeddingProvider` trait + `VectorStore` 内存向量存储
  - `RagExtractionStrategy` 作为第三种提取模式（`extract_with_rag`）
- **知识图谱覆盖感知爬取**: `KnowledgeGraphAccumulator` + Chao1 覆盖率估计 + 结构空洞检测
  - `KgBoostScorer` 实现 `UrlScorer` trait，结构空洞区域 URL 获得更高优先级
  - `StopCondition` 新增 `CoverageReached` 停止原因 + `min_coverage` 条件
- **DRL 自适应爬取策略**: `DrlPolicy` ONNX 推理 + `HeuristicPolicy` 启发式退化
  - `CrawlState`/`CrawlAction` 状态-动作定义
  - `DrlConfig` 配置开关（默认关闭）
  - Python 训练脚本 `scripts/drl/train_policy.py`（gymnasium + stable-baselines3 + ONNX 导出）
- **可观测性增强**: 5 个新 Prometheus 指标
  - `crawlrs_queue_depth` Gauge（任务队列深度）
  - `crawlrs_engine_success_total` Counter（引擎成功/失败计数）
  - `crawlrs_engine_duration_seconds` Histogram（引擎请求耗时分布）
  - `crawlrs_cache_hit_total` Counter（缓存命中/未命中）
  - `crawlrs_webhook_delivery_total` Counter（Webhook 投递成功/失败）
- **基准测试扩展**: URL 验证/SSRF 检测/RegexCache/引擎路由基准（`benches/benchmark.rs`）

### Changed

- **代码架构优化**:
  - `router.rs` 拆分为 `engine_selector` / `route_sequential` / `route_race` 子模块
  - `scrape_worker.rs` 拆分为 `scrape_task` / `crawl_task` / `extract_task` + `builder` + `deps`
  - `ScrapeWorkerDeps` 参数对象化（消除 16 参数函数签名）
  - `webhook_service.rs` 拆分出 `webhook/management.rs`
- **测试代码提取**: 5 个模块内联测试提取到独立文件（scrape_worker/router/engine_client/webhook_service/task_repo）
- **统一测试 Mock**: `tests/common/mocks/` 共享 MockTaskRepository/MockScraperEngine/MockWebhookService 等

### Fixed

- 删除 `TextEncodingProcessor` dead code（`#[allow(dead_code)]` 标注的未使用结构体）
- 删除 `make_queue()` 测试 helper（clippy never used 告警）

## [0.2.0] - 2026-07-29

### Added

- Garrison RBAC 集成：使用 garrison `ApiKeyHandler::verify_with_namespace` 直接获取 `login_id` 与 `scopes`，废弃 `GarrisonUtil::check_api_key` + session 查找链路（解决 garrison 内存 DAO 下 session 缺失导致的 login_id=None 401 问题）
- `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 环境变量：服务启动时注入 bootstrap admin key，同时写入 garrison DAO 与 crawlrs `api_keys` 表 + 初始化团队，解决 garrison 内存 DAO 的跨进程 key 丢失
- Axum 中间件链路校准：`create_v2_routes_with_state` 中先 `.layer(team_semaphore_middleware)` 再 `.layer(auth_middleware_inner)`（后 .layer = outer = 先执行），确保 `AuthState.team_id` 在信号量提取前已注入
- `team_semaphore_middleware`：`extract_team_id` 改为从 `AuthState` 读取 `team_id`（替代直接读取裸 `Uuid` Extension），修复 v2 路由 401
- API 端点 `DELETE /v1/crawl/{id}`（取消 crawl）替代已废弃 `POST /v1/scrape/{id}/_cancel`

### Changed

- 认证失败路径：所有 protected routes 不再走 garrison `TokenSession` 会话查找（因 garrison `check_api_key` 不创建 session）
- SDK router (sdforge) 的 auth 中间件统一使用 `from_fn_with_state(db_pool, auth_middleware_inner)`，与 protected/v2 三端一致
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
- Multi-engine support: Reqwest (HTTP), Playwright (JS rendering), FlareSolverr
- Search engine aggregation (Google, Bing, DuckDuckGo, SearXNG)
- Bearer Token authentication with bcrypt hashing and brute-force protection
- API key scope system (Read / Write / Admin) with team isolation
- Credits-based billing system for search/scrape/crawl/extract operations
- Geographic restriction enforcement (country allow/block lists, IP whitelist)
- Comprehensive SSRF protection with DNS resolution validation
- Task queue with priority scheduling and retry support
- Webhook delivery system for event notifications
- Audit logging for all API operations
- Rate limiting with circuit breaker support
- LRU cache for API key validation (TTL 120s, capacity 10000)
- Extraction engine with LLM support (genai integration)
- Admin CLI tools for credits management

### Security

- Constant-time comparison for HMAC signature verification
- SSRF validation on all URL-accepting endpoints (scrape/crawl/extract)
- Proxy URL SSRF validation in search and crawl paths
- IDOR protection on audit logs endpoint (CWE-862)
- IP-based brute-force lockout (5 failures → 15min lockout)
