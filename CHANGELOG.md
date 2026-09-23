# Changelog

## [Unreleased]

### Fixed（fix-audit-defects-r1：2026-09-24 面试题对照审计的 16 处实锤缺陷）

反爬 / HTTP 层：
- **代理对 HTTPS 静默失效**（高危）：全部代理构建点 `Proxy::http()` → `Proxy::all()`——原实现只拦截 `http://`，https 目标被绕过代理直连（真实 IP 泄漏）
- **wreq TLS 伪装落地**：`emulation_provider` 从"忽略入参全落默认"改为 Chromium 系真实仿真（浏览器默认头 + Chrome H2 SETTINGS 帧参数）；WreqEngine 的 UA 候选集过滤为 Chromium 系 profile，消除"Firefox UA + Chromium 系 JA4"指纹矛盾
- **wreq 会话/压缩**：启用 cookies/gzip/brotli/zstd/charset/stream feature + `cookie_store(true)`；新增响应体大小上限读取（对齐 reqwest 侧防 OOM 双保险）
- **race 竞速模式绕过反爬检测**：胜出响应现在必须通过 antibot 分类，挑战页作废并继续竞速剩余候选（与顺序模式防护语义对齐）
- **`_back\b` 正则误报 Akamai**：替换为 `_abck`/`ak_bmsc`/`bm_sv` 真实标志——正常页面含 `go_back()` 等命名不再被无谓升级浏览器

去重 / 布隆过滤器：
- **布隆假阳性永久漏抓**（高危）：DB 确认"不存在"的 URL 此前会被二次 `check_and_insert` 误判丢弃（`to_db_enqueue` 分支恒空），现改为无条件 insert + 入队
- **去重状态零持久化**：worker 启动时从 tasks 表分页（5000/页）灌入 distinct URL 预热布隆——重启不再全站重爬
- **布隆容量无扩容**：装载率 ≥90% 时以权威集合自动重建 2× 容量布隆（误判率回到目标值，无假阴性）

元搜索：
- **SmartSearchEngine 接入生产 API**：`/v1/search` 无 `engine` 参数（或 `"smart"`）时走多引擎 fan-out + RRF 聚合（此前聚合层构建后从未被路由消费）；显式引擎名单引擎路径；未知引擎名返回 400 而非静默替换为 bing
- **RRF 融合量纲失衡**：RRF 与 relevance 分量按集合内 min-max 归一化后再加权——多引擎共识信号不再被单引擎相关性淹没
- **Bing 验证码检测**：解析前检测挑战标记（bnp.bing.com 等），显性返回 `Captcha` 而非静默"0 结果成功"
- **Bing 跟踪链接解包接线**：`decode_bing_url` 此前是死代码，现于解析出口统一解包 `bing.com/ck/a` 重定向——跨引擎去重/聚合对 Bing 结果生效

内容处理 / 浏览器引擎：
- **`only_main_content` Markdown 结构退化**（高危）：正文抽取新增 `html` 输出（trafilatura `include_links: true`），Markdown 转换优先用结构化正文 HTML——链接/标题结构不再丢失；无结构化 HTML 时显性降级（warn 日志）
- **DFS 爬取策略语义反转**：子链接 priority 由 `+1` 改 `−1`（出队 priority ASC 下深层先出）——此前 DFS 实际是 BFS
- **stealth 注入导航后失效**（高危）：改用 CDP `Page.addScriptToEvaluateOnNewDocument` 注册到所有后续文档——此前 `page.evaluate` 只作用于 about:blank，`navigator.webdriver` 覆盖形同虚设
- **假 NetworkIdle**：`wait_network_idle` 改为 CDP 网络事件驱动的 in-flight 计数 + quiet window（此前是固定 sleep 500ms）；CDP 失败 fail-open 到 sleep
- **`wait_for` API 接线**：worker 路径此前硬编码 `None` 丢弃调用方参数，现映射为 `DomStable` 等待策略（与 use-case 层一致）
- **池路径 argv 注入 + tab 会话串扰**：池启动代理 URL 校验前置（复用 `validate_proxy_url`，fail-fast）；Playwright UA 仅取 Chromium 系 profile；tab 归还时清空 cookies

### Changed
- `/v1/search` 无 `engine` 参数时行为从"单引擎 bing"变为 smart 聚合（见 docs/API_REFERENCE.md Search API）

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
- **auth-off 安全语义收紧**：protected routes 默认拒绝（401），移除 `DEFAULT_IDENTITY_TOKEN_HASH` 占位哈希与 `full_access()` 身份注入；opt-in 后注入匿名受限身份（`denied()` scope、无 token_hash）
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
- auth-off 下 protected routes 默认 401（曾依赖未鉴权全开的嵌入方需显式 opt-in）
