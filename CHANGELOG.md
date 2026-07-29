# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
