# Changelog

当前版本：0.2.0-rc.2（与 `Cargo.toml` `version` 一致）。格式遵循 [Keep a Changelog](https://keepachangelog.com/)，版本号遵循 [语义化版本](https://semver.org/spec/v2.0.0.html)。面向用户的精简摘要见 [docs/CHANGELOG.md](docs/CHANGELOG.md)。

## [Unreleased]

### Fixed（遗留收敛——UaPool 消费面/locale/arch + 测试门禁补齐）

- **CriOS Client Hints 数据真相**：两个 iOS Chrome profile 的 `sec_ch_ua` 模板修正为空串（WKWebView 不发送任何 Client Hints，原 Chrome 品牌表系编造能力）——reqwest 等 UA 消费路径不再发出 "UA=WKWebView + 客户端提示=Chromium" 矛盾组合（数据层一修全修：pick()/pick_seeded/router 分支全量受益）；一致性单测把 CriOS 归入空串集
- **Playwright locale 覆盖**：页面设置追加 `Emulation.setLocaleOverride`（locale = accept_language 首段标签），闭合 zh-CN 等 profile 的 "请求头 zh-CN 而 Intl 默认 en-US" 二次矛盾（launch `--lang` 为进程级固定值的残留缺口）；CDP 失败 best-effort 不阻断
- **Sec-CH-UA-Arch 宿主一致**：architecture 由宿主 `std::env::consts::ARCH` 派生（x86_64→x86、aarch64→arm），替代硬编码 x86；Android profile 保持 arm 设备叙事
- **测试门禁补齐**：liveapi 新增 `crawlrs migrate` 子命令幂等双跑验证（此前 psql 手工迁移恰好绕过二进制迁移路径）、429 限流关卡（80×SSRF 拒绝请求 burst → 429 + rate-limit 错误信号，置于 402 后；限流 helper 走旧式包封——error 为字符串，产品内包封不一致已备案）；api_test.sh 补 garrison 探针冒烟（`/health/live` 200、`/health/ready` 200|503）

### Changed（Playwright stealth 指纹一致性升级）

- **补服务端可查的指纹矛盾（三路审查后收敛为四层）**：launch 改 New Headless（`new_headless_mode()`）+ `--disable-blink-features=AutomationControlled` + `--lang=en-US,en`（`playwright_pool.rs` 唯一 launch 点）；页面设置由裸 `set_user_agent` 升级为完整 `SetUserAgentOverrideParams`（UA + acceptLanguage + navigator.platform + userAgentMetadata）；brands 取 `UaProfile.sec_ch_ua` 逐版本权威模板（含 Edge/Samsung 正确品牌与 GREASE，与 wreq 发头同源——从 UA 串重解析会把 Edge 解析成 Google Chrome 品牌与 `Edg/` 标记自相矛盾）；**双词表拆分**：`navigator.platform`（Win32/MacIntel/iPhone/Linux armv8l）与 `Sec-CH-UA-Platform`（Windows/macOS/…）由 `build_ua_fingerprint` 各自映射，绝不同值双写（初版 UA 原文双写恰是待消除的矛盾类，审查捕获后修正）；Android 高熵字段按 UA 事实（architecture=arm、model 解析自 UA，`wv` WebView 标记置空）；CriOS（WKWebView，无 Client Hints 能力）从 `pick_chromium` 候选排除且 metadata 整体缺席——绝不发送空品牌 client hints；full_version_list 恒缺席（仅 Accept-CH 索取时发送，伪造比缺席危险）。无新增配置项
- **实测基线（test-results/antibot/）**：header 回声取证 `Sec-CH-UA` 由缺失/HeadlessChrome 变模板品牌（`Google Chrome`+`Chromium`+GREASE，版本与 UA 一致）、`Sec-CH-UA-Platform` 为 client-hints 词表（`macOS`，非 UA 原文 `Mac OS X`）；反爬矩阵 nowsecure.nl/g2.com × js_rendering 通过率 0%→0%（js/plain 均被分类器显式判 `cloudflare challenge structure matched`，兜底语义不变；Turnstile 穿透不在承诺内，见变更 Deferred）。冒烟位 npr-plain 受同机并行构建 CPU 争用致 reqwest 5s MRT 熔断（curl 直测 1.1s 正常），属取证环境噪声，非产品回归

### Changed（同作者依赖族 rc 线对齐）

- **dbnexus 依赖族对齐 rc.9 线**：移除 dbnexus 已删除的死特性 `config-toml`（rc.6 时即空壳——`= []` 不拉依赖不门控代码，crawlrs 的 TOML 解析来自直接依赖 confers，与该特性无关；上游 `../base/dbnexus@c59ae46` 清理）。vecboost（本地 patch）清单抬到 dbnexus rc.9 后锁失效重解析，各同作者库按既有「最低 rc」semver 要求自动落位最新满足版：dbnexus rc.9 / confers rc.10（rc.8/rc.9 已 yank）/ trait-kit rc.9 / inklog rc.9 / oxcache rc.8 / limiteron rc.7 / sdforge rc.8；garrison 维持 rc.2 不动。`cargo check` 一次通过零 API 破损；完整回归：clippy(default|full) 0 告警、lib 测试 6014+6184 通过、集成 152 通过、liveapi 完整构建全链绿（构建路径自 vecboost 清单变更后首次恢复）。`Cargo.lock` 同步入库

### Fixed（E2E 覆盖缺口补齐）

- **活体 E2E 覆盖 8 个零覆盖功能面**：`api_test.sh` SDK 断言改打真实挂载路径 `/api/v1/sdk/*`（修正 404 空洞通过与过时 `http feature` 注释，`(422)`/`(403)` 误命名对齐实际 400）；`api_semantics_test.py` 37→45 场景——SDK 语义（search 成功/引擎降级 500、空 query 400、scrape 创建 2xx）、`POST /v1/scrape/{id}/_cancel`（200+`message` 包封+终态轮询、未知 id 404）、`POST /v1/tasks/_cancel` 批量取消（`cancelled∪failed` 全覆盖+逐个终态）、`expires_in_secs=1` 短期 key 过期 401、webhook 签名创建（201+`data.secret` 回显、额外字段拒绝、列表脱敏；缺 `--webhook-secret` 时 SKIP）；`live-api-e2e.sh` 注入 `CRAWLRS__WEBHOOK__SECRET` 并透传，末尾新增 402 关卡（积分归零 → `POST /v1/scrape` 断言 402 `QUOTA_EXCEEDED`）；删除无套件引用的孤儿文件 `tests/e2e/test_scenarios.py`。注：webhook 额外字段拒绝实际为 400 `VALIDATION_ERROR`（handler 手工 `from_slice` 映射），非任务草拟的 422，以服务端现实为准

### Breaking（auth-off 安全语义恢复——文档审计缺陷 #6）

- **auth-off protected routes 恢复默认拒绝（401）**（`ee135748`）：恢复 v0.2.0-rc.1 收紧语义（rc.2 路由装配重构时意外丢失且无记录）。`auth` feature 关闭时，protected 路由默认 401；嵌入方须显式调用 `crawlrs::presentation::middleware::auth_middleware::allow_unauthenticated_protected()` 放行——注入匿名受限身份（`ApiKeyScope::denied()` scope、无 token_hash），一次 `error!` 日志声明降级。默认构建（platform 隐含 auth）不受影响
- **恢复 `web-axum` 构建面**：platform 隐含 auth，auth-off 的降级路径（`default_identity_middleware` / `allow_unauthenticated_protected`）在 rc.2 之后的特性矩阵中不可能编译（presentation 挂 platform → 必含 auth），系 rc.1 移除 web-axum 后的失修面。现按 rc.1 机制恢复：`--no-default-features --features web-axum,db-postgres --lib` 构建/测试 auth-off 语义（lib 门控展开至 presentation/bootstrap/workers/di/queue/infrastructure 等，`CrawlRsError::IntoResponse` 与 `Database` 变体双面覆盖），auth-off 中间件测试（默认 401 / opt-in 注入 denied 模板身份 / 无凭证放行 / 非法凭证忽略）首次可在该轴运行（5/5 通过）

### Added（文档审计缺陷清偿 #1-#5）

- **`crawlrs migrate` 子命令**（#1，`3b98167a`）：生产 schema 初始化一等入口，按序应用 `migrations/*.sql`（经 dbnexus `auto-migrate` 特性 `DbPool::run_migrations`）；目录缺省 `./migrations`，`CRAWLRS_MIGRATIONS_DIR` 可覆盖
- **`POST /v1/scrape/{id}/_cancel` 生产路由注册**（#2）：此前 handler 完整实现却仅注册在测试路由，生产 404 无法取消单个抓取任务（仅能经 `POST /v1/tasks/_cancel` 批量取消）；补路由组注册 + 注册探活测试。与 0.2.0 引入的 `DELETE /v1/crawl/{id}` 并存
- **搜索配置生效**（#3）：`search.default_engine`（单引擎兜底路径默认引擎，须已注册否则 warn 回退 bing）与 `[search.engines] bing/baidu/sogou_enabled` 注册开关（关闭的引擎不注册，smart 聚合与单引擎路径均不可用）此前为死配置无代码消费，现由 `SearchClient::with_configured_engines` 消费；default.toml / .env.example 同步（移除不存在的 google 键）；新增 i18n 日志键 4 个（en/zh）
- **`ScrapeOptionsDto` 暴露 `block_ads` / `block_media` / `needs_mllm`**（#4）：引擎层能力此前已在、仅 DTO 未暴露；use-case 与 worker 双路接线；`needs_mllm` 在无 `llm` 特性构建下提交期 422 显性拒绝（worker 侧任务级显性失败），不再静默降级；`session_id` 仍为内部字段
- **forge_api 注册面纳入编译**（#5，`6d295bd7`）：`presentation/forge_api`（sdforge inventory 单一注册面迁移，在途）此前未在模块树声明——不参与编译、随 handler 演进静默腐烂。新增 `forge-full` 特性停机坪：inventory 链接期全局收集，激活态与手写 protected/v2 路由组互斥（并存即启动 panic `Overlapping method route`），含路由等价性锚定测试（inventory 探活 + 生产路径子集 + 已知缺口钉住）；整体切换需三族中间件（protected/v2/sdk）统一排序的设计裁决，待后续变更实施

### Added（i18n / Fluent 接入）

- **42 处用户可见字符串与日志接入 Fluent**（`9fdcafeb`）：自上个版本 tag 以来的新增/改动字符串与日志统一走 `src/i18n` 查找（engines、handlers、repositories、mappers、observability、ssrf、limiteron、garrison listener、di/id 模块、utils、workers 等面）；en-US 默认，zh-CN 键位对齐（既有已发布字符串零改动）
- **ID 模块配置校验与 auth-off 降级日志接入 Fluent**（`72ddb07c`）：errors.ftl / logs.ftl 各增 2 键（IdModule 启动期算法校验、auth-off 降级模式运维日志），并登记 I18nBundle 键注册表（en/zh 双语）
- **IdModule 与 auth_middleware 改经 tr_log 输出**（`3e8997bb`）：算法校验两处与 auth-off allow/deny 降级日志的消息内容由 locales 双语资源承载，代码层不再持有用户可见字符串

### Added（功能吸收 F1–F12）

- **礼貌策略 AutoThrottle**（F1/F5，`1cc6a3c6`）：HostDiscipline `current_interval` 动态延迟收敛 + 429 指数退避 + Crawl-delay 双时钟 + 停滞检测；route_sequential 落 429 直通观测与 stalled→rotate_proxy；Retry-After 解析抽为 `utils::retry_after` 单实现（antibot 分类器共用）；`[politeness]` 配置面 10 键经 DI 注入
- **Session 身份三件套**（F2，`4b88673a`）：`SessionIdentity`（代理 + UA/TLS 档 + Cookie 缓存）与 `SessionManager`（DashMap 亲和池，error_score +1.0/−0.5 记分、uses/age 惰性退休、池满降级临时身份）；`[session]` 配置 6 键，`session.enabled` 默认 false（关闭时等价盲轮换现状）；挑战页/401/403/429 mark_bad、2xx mark_good
- **sitemap 流式一等请求源 + 软 404 指纹 + seen 预热差量**（F3/F11，`aaec5d8b`）：`SitemapFetcher::fetch_streaming`（quick-xml 增量解析 + mpsc 背压）与 `MapUseCase::execute_streaming`（sitemapindex 递归一层、并发上限、max_urls 护栏），`POST /v1/map` 端点（`sitemap` feature 门控）；每 crawl `sitemap_seed` 显式选择加入 + 主开关 422 拒绝 + depth=0 执行侧挂接；`Soft404Detector` 内容指纹学习/命中，200 壳页不落结果防重试风暴；`list_distinct_urls` keyset 分页差量预热去重器；`utils::glob` 统一通配匹配单实现
- **失败现场快照 + AIMD 内存压力联动**（F4/F10，`2c726cce`）：`FailureSnapshotter` 按错误签名键（sha256(url|status|reason)[:30] + 消毒 reason）去重存档响应现场（.html/.meta.json/截图），`ObservabilitySettings` 配置面（dir 空串=禁用）、antibot 检出分支 spawn_blocking 落盘不阻塞 worker；`AIMDController::on_memory_pressure`（Pressure 冻结 additive-increase / Critical CAS 半减）+ TeamSemaphore 广播，消除「门控暂停派发、AIMD 却加并发」的矛盾；另增 AIMD 并发 target 团队 gauge 与增减 debug 日志（`5f71dd92`）
- **Playwright XHR/Fetch 后台响应捕获**（F6，`23494fda`）：CDP Network 双事件流（ResponseReceived + LoadingFinished）内联取体（规避 Network.disable 丢弃缓冲），glob 白名单过滤 + max_bytes 截断 + max_responses 上限，四类丢弃计数 collect 时 warn 上报；`[engines.playwright]` 配置 4 键，`capture_xhr` 默认 false（零 Network 监听、零行为变化）
- **提取降级阶梯质量门 + LLM token 窗口分块并行 merge**（F7/F8，`985a56ca`）：`QualityGateConfig`（word_count 短结果升级 + 长度比裁决 + prose 硬门槛 sentence_avg/link_density）接入 `ContentExtractionFacade` 裁决式 try_extract_chain，`disabled()` 常量保持未接线调用点行为不变；chunker 零依赖确定性分块（budget=（窗口−开销−输出）×chars_per_token÷2、三级边界贪心切块、术语预检短路、JoinSet+Semaphore 并行、merge prompt 失败显性回退启发式）；`[llm.chunking]` 八键配置面
- **提取结果字符级溯源 + MLLM 混合 AX 树快照**（F9/F12，`aa58626b`）：provenance 三层对齐（贪心最早单调 + 锚定滑窗最紧 LCS + coverage/density 双门），`extract_data_with_provenance` 挂 `_provenance` 字段（`provenance_enabled` 默认关）；AX 树经 CDP `Accessibility.getFullAXTree` 采集、prune 折叠无名连接节点，decision Click 增 `ax_id`（DOM.resolveNode + Runtime.callFunctionOn(user_gesture) 定位），失败回退既有 selector 路径
- **视觉调用支持自定义 OpenAI 兼容端点**（F12，`2fbca6f7`）：视觉模型带 `openai:` 前缀且设置 `CRAWLRS__MLLM__VISION_API_BASE_URL` / `CRAWLRS__MLLM__VISION_API_KEY` 时绑定 OpenAI 适配器，`ServiceTargetResolver` 接管端点与鉴权（剥 `openai:` 前缀保留裸模型名；不绑定 adapter_kind 时 genai 对未识别模型名回退嗅探为 Ollama、POST /api/chat 打到网关必然 404）；未配置时 provider 原生路由，gemini/ollama 语义不变
- **文档同步**（`ed7f834a`、`3338341c`、`10338e4d`）：README 特性表与 API 文档同步 F1–F12 功能吸收，清理冗余 F1–F12 段落，注释与实现现状对齐
- **活体/验收测试适配**（`b3128b3c`、`4d6976e5`、`eba14e86`、`673bc6d1`、`7d847df3`、`bab5d770`、`57d5c6d8`、`2457bdf4`）：F12 AX 树 + Ollama 视觉导航活体验证（`#[ignore]` 门控、视觉模型环境变量覆盖 `CRAWLRS_TEST_VISION_MODEL`）、F4 滚动护栏验收测试（max_total 最旧优先淘汰）；修复活体门禁暴露的构造适配/过时断言/clippy 违例、WorkerManagerDeps 测试构造补字段、garrison 全局单例跨测试互斥统一；新增活体栈测试运行器 `scripts/run-live-test.sh`；e2e 阶段失败 PIPESTATUS 判定修正

### Added（引擎与智能增强）

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

### Added（引擎健康监控 garrison 化）

- **引擎健康监控吸收 garrison `HealthCheck`/`HealthRegistry`**（auth 门控，`6c154273`）：探活逻辑改写为 `EngineHealthCheck` 注册进 `HealthRegistry`，`/health/live` + `/health/ready` 探针路由经 `health_routes` 并入现有 Router（readiness 聚合 Unhealthy → 503 摘流语义由库承载；单项探测超时经库 check_timeout 护栏按 Unhealthy 聚合）；聚合口径与迁移前 `get_aggregate_status` 一致；auth-off 构建保留精简回退，不为健康面拉入 garrison 全家桶。详见 Changed → selfdev-lib-adoption-r1 S2

### Changed（selfdev-lib-adoption-r1：5 处自研实现下沉自研库矩阵，1 处判定不可吸收）

逐项复核 crawlrs 自研实现与自研库（limiteron/garrison/oxcache/sdforge）能力重叠面后吸收，库侧缺口按 docs/ARCHITECTURE.md「自研库反向需求」登记：

- **S3 错误机制下沉 sdforge**（`src/common/error.rs`）：`status_code` 手写 17 臂改经「变体→`ApiError` 归一 + `unified::http_status_for` 委托」——14 个变体的 HTTP 状态码由库映射表单点维护，映射结果与迁移前表逐项相等（测试钉住）；502/504（Network/Engine/Timeout）为 sdforge 覆盖集外残留，显式保留并登记缺口；`sanitize_engine_error` 正则脱敏保留为领域特例，Internal/ServiceUnavailable 整体遮蔽语义与 `ApiError::sanitized_message` 对齐（测试断言）；i18n 保留 crawlrs `I18nBundle`（sdforge `TranslationStore` 仅支持 JSON 装载、回退链不匹配，缺口已登记）
- **S5 DNS 单飞折叠 oxcache**（`src/infrastructure/dns/dns_cache.rs`）：`lookup_host` 的 miss/single-flight/回写编排折叠为 `Cache::get_or_with_ttl` 内建 leader/follower，删除自维 `in_flight` DashMap + `FlightSlot` + `resolve_single_flight`（约 120 行）；per-entry TTL 取解析器记录剩余有效期（`valid_until`，以 `default_ttl` 为上限），无记录 TTL 回落 `default_ttl`；hit/miss 指标口径不变
- **S4 布隆过滤器替换 oxcache `BloomFilter`**（`src/utils/dedup/bloom.rs`）：删除 mmap(MAP_HUGETLB)→mmap→heap 三级降级链与 `unsafe impl Send/Sync`（约 384 行），换得 `Arc<RwLock>` 线程安全与标准 FPR 参数化（`new(1_000_000, 0.01)`，假阳性率常量化）；**行为变化：mmap 大页驻留策略移除——大容量下内存驻留由 2MiB 大页 mmap 变为堆分配（1M @1% ≈ 1.2MB 位图）**；libc 目标依赖随删除
- **S2 引擎健康吸收 garrison**（`src/engines/health_monitor.rs`，`auth` feature 门控）：探活逻辑改写为 `EngineHealthCheck`（impl garrison `HealthCheck`）注册进 `HealthRegistry`，`/health/live` + `/health/ready` 探针路由经 `axum_routes::health_routes` 并入现有 Router（readiness 聚合 Unhealthy → 503，K8s 摘流语义由库承载；单项探测超时经库 check_timeout 护栏按 Unhealthy 聚合）；聚合口径与迁移前 `get_aggregate_status` 逐字一致（测试对账）；auth-off 构建保留精简回退，不为健康面拉入 garrison 全家桶
- **S6 熔断薄封装 limiteron `ManualCircuitBreaker`**（`src/engines/circuit_breaker.rs`）：per-engine 键控池由 crawlrs `DashMap` 承载（库无键控熔断器），`record_success`/`record_failure` 转 async 库调用（router 五处调用点转 async），`is_open` 保持同步签名——record 后镜像状态到 `AtomicU8`（+Open 时刻），selector 同步读镜像；`CircuitStats` 与 engine 标签指标保留 crawlrs 埋点；**行为变化：滑窗绝对失败计数 → 库累计比例触发（`total ≥ failure_threshold` 且失败占比 ≥ 0.5）——连续 N 次失败仍必跳闸，混合流量下灵敏度提高**；配置映射 `min_requests = failure_threshold`、`half_open_max_successes = 1` 保留「一次成功即闭合」语义
- **S1 团队信号量池：判定不可吸收**（`src/domain/services/team_semaphore.rs`）：复核发现库 `ConcurrencyLimiter` 无公开非阻塞入口（`try_acquire` 仅存于库 `#[cfg(test)]` 私有 impl）、键控池许可无法从 `&self` 逃逸借用（并发 map 交出 `&'a V` 必持局部 guard，自引用结构安全 Rust 不可构造）、manager 无注入入口——Fixed 模式保留 crawlrs 实现（签名/调用方零改动），与 S6 同类登记库侧缺口待二次收敛
- 验证：`cargo test --features default --lib` 6013 通过 / 0 失败；`cargo test --features auth --lib` 6013 通过；`cargo check --features platform,auth,rate-limit,openapi` 通过；`cargo clippy --features default|agent-lib --lib` 零告警；default（经 platform 隐含 auth）/ auth-only / agent-lib（auth-off）三构建面全绿

### Changed（自研库依赖特性扩容）

- limiteron features 不变（`manager` 原按方案补入，S1 判定不可吸收后无消费方，按 feature hygiene 纪律撤回）；oxcache features 补 `bloom`（URL 去重布隆过滤器，替代自研 MmapBloom）；移除 libc 目标依赖（MmapBloom mmap 面随 S4 删除，全仓零引用）

### Changed（依赖升级与特性显式化清洗）

- **自研 rc 链跟进**：dbnexus 0.6.0-rc.5→rc.6、confers 0.6.0-rc.5→rc.6、inklog 0.3.0-rc.5→rc.6（rc.7 fast-masking 在日志行 fancy_regex BacktrackLimitExceeded 时 panic，刻意不升、锁文件钉 rc.6，见 Cargo.toml inklog 注释）、trait-kit 0.5.0-rc.6→rc.7、oxcache 0.5.0-rc.5→rc.6、limiteron 0.3.0-rc.5→rc.6、sdforge 0.5.0-rc.5→rc.6；garrison 0.9.0-rc.2 已是最新 rc 不动；vecboost 0.3.0-rc.1 经 `[patch.crates-io]` 指向本地适配 confers rc.6 的修复版（`../vecboost`），上游发 rc.2 后可移除（见 Fixed → rag 特性编译修复）
- **三方依赖升最新稳定版**：quick-xml 0.41→0.42（`LocalName::as_ref` 改返 `&str`、`xml10_content` 改返 `Cow<str>`，map_use_case sitemap 解析适配）、rig-core 0.42→0.44（0.43 wire/driver 新架构：rig_embedding_provider 重写为 `OpenAIConfig::new` + `client.embedding(model, Some(ndims)).call(texts)`，并适配 0.43 引入、0.44 延续的显式 ndims 回包宽度校验——宽度不符显性报错而非静默返回）、uuid 1.26→1.27
- **全量特性显式化**：`[workspace.dependencies]` + 全部直接依赖 + dev-dependencies + examples 关闭默认特性、声明最小特性集；实质裁剪：axum 去 form/matched-path/original-uri/tower-log/tracing（全仓零引用）、sysinfo 仅留 system、genai 仅 rustls-tls、futures 去 executor、criterion 去 rayon/plotters、chromiumoxide 显式 bytes、base64 剥离 simd-unsafe、aho-corasick 显式 std+perf-literal、testcontainers/testcontainers-modules 剥离默认 ring；chrono 显式补 clock+std（此前 `Utc::now` 靠依赖图 feature 并集隐式成立）；plotters、tracing-futures、as-any 与 sysinfo 0.38 双版本退出依赖树
- **CI actions 升级**：codecov-action v7.0.0→v7.1.1、setup-rust-toolchain v1.17.0→v2.0.0、codeql-action v4.38.0→v4.38.2、action-gh-release v3.0.2→v3.0.3、taiki-e/install-action→v2.87.26、dtolnay/rust-toolchain master SHA 刷新；checkout v7.0.1 / rust-cache v2.9.2 / cargo-deny-action v2.1.1 已是最新未动
- **新增特性轴与库适配**：`web-axum`（auth-off 等价构建面）、`forge-full`（forge 注册面停机坪）、dbnexus `auto-migrate`（migrate 子命令前置）、`[patch.crates-io] vecboost`（本地修复版）；dbnexus rc.6 新增 `DbError` 变体（RateLimited/Unsupported/VersionConflict）仓库层补臂适配，inklog rc.6 `rate_limit`/`sampling` 字段 telemetry 装配补齐
- **钉版佐证维持**：wreq 6.0.0-rc.31（5.x 稳定版全部 yanked，rc.31 仍为最新可用）、testcontainers 0.27（testcontainers-modules 0.15.0 硬依赖 ^0.27.0，cargo outdated 强升 0.28 即版本冲突）、utoipa 5.5（自研 sdforge 0.5.0-rc.6 optional 依赖 ^5.5 并 `pub use utoipa` re-export，升 6.0 双 utoipa 共存触发 E0659 歧义、与 forge 宏生成代码断裂）
- **已知事项**：RUSTSEC-2023-0071（rsa）/2024-0436（paste）/2025-0119（number_prefix）维持 deny.toml 既有 ignore 与 CI 前提校验（rsa 随 jsonwebtoken 死依赖移除已进一步收敛，见 Removed）
- 验证：`cargo check`（no-default / default / full / platform / openapi / engine-tls-fingerprint）全绿；`cargo clippy --features default|full -- -D warnings` 全绿；`cargo fmt --all -- --check` 通过；`cargo check --features full,test-mocks --tests` 全部测试目标编译通过；`cargo test --lib`（standard 6112 / full 6163）0 失败；rag-remote 嵌入单测（wiremock 契约）4/4 通过；`cargo deny check advisories` ok

### Changed（ID 体系全量迁移至 NebulaId）

- **统一 ID 承载体**：新增 `domain::id::Id`（内部 `u128`，包裹 `nebulaid::core::Id` 语义），为迁移前 `uuid::Uuid` 的全项目替换物；补齐 `Copy/Hash/Default/Ord/FromStr` 并以 UUID 字符串 serde（绕开孤儿规则，保持 API JSON 契约不变）
- **生成引擎切换**：全项目 ID 生成经本地 `../NebulaId`（`nebulaid` path 依赖，`default-features=false, features=["sdk"]`）。默认算法 `uuid_v8`（生成 UUID 兼容值 → DB `UUID` 列与 API JSON 契约零改动、无 schema 迁移）；`snowflake` 为可配置备选（`[id].algorithm`）
- **生成入口**：`IdService::next_id().await`（async，经 DI 注入）+ 同步 `Id::generate()`（进程级预取池 + 后台补水的同步门面，覆盖 `impl Default`/结构体字面量/同步 mapper 等无法 await 的上下文，作为 `Uuid::new_v4()` 直接替换物）
- **装配**：`IdModule` 在启动期构建 `NebulaIdKit` 并注入 `CrawlRsState`（API 与 worker 两种 personality 共用同一装配点）；配置新增 `[id]` 段（`CRAWLRS__ID__*`）
- **持久化边界**：实体层保持 `uuid::Uuid`/`UUID` 列不变，仅 repository/mapper 边界做 `Id↔Uuid` 转换；`impl From<Id> for Value` 使 `Id` 成为 sea-orm 一等查询值
- **算法解析断言对齐**：NebulaId rc.3 算法别名反转后 `IdModule::resolve_algorithm` 断言同步对齐（`106f7592`）
- **测试收尾**：外部集成测试与多二进制唯一性适配（`edf68f7e`）；CWE-798 回归测试见 Security（`e466b1b6`）
- 验证：`cargo check --lib/--bins`（default + full）全绿；`cargo clippy -- -D warnings`（default + full）全绿；`cargo fmt --all -- --check` 通过；`cargo deny check`（advisories/bans/licenses/sources）全绿；`cargo test --lib --features default` 5981 通过 / 0 失败；外部 `tests/`、`benches/` 编译全绿

### Changed（代码架构）

- **代码架构优化**:
  - `router.rs` 拆分为 `engine_selector` / `route_sequential` / `route_race` 子模块
  - `scrape_worker.rs` 拆分为 `scrape_task` / `crawl_task` / `extract_task` + `builder` + `deps`
  - `ScrapeWorkerDeps` 参数对象化（消除 16 参数函数签名）
  - `webhook_service.rs` 拆分出 `webhook/management.rs`
- **测试代码提取**: 5 个模块内联测试提取到独立文件（scrape_worker/router/engine_client/webhook_service/task_repo）
- **统一测试 Mock**: `tests/common/mocks/` 共享 MockTaskRepository/MockScraperEngine/MockWebhookService 等

### Changed

- `/v1/search` 无 `engine` 参数时行为从"单引擎 bing"变为 smart 聚合（见 docs/API_REFERENCE.md Search API）

### Removed

- **jsonwebtoken 死依赖**（`bd44ea14`）：Cargo.toml 声明的 jsonwebtoken 在 src 全仓零引用（2026-10-03 审计），其 `rust_crypto` 后端无条件携带 rsa 0.9.10（RUSTSEC-2023-0071，无补丁），把 rsa 塞进全部消费方依赖树。JWT 能力由 garrison（auth feature）自带、不受影响；crawlrs 自身依赖树（default 含 auth）的 rsa 残余路径收敛为 garrison 单条，agent-lib 消费方（default-features=false 构建）的 rsa 彻底消失。docs/SECURITY.md 依赖安全基线已同步

### Fixed（fix-audit-mediums-r3：缓办项落地 + 评审 LOW 收尾）

- **HostDiscipline per-host 纪律组件**（`utils/host_discipline.rs`）：per-host 预约时刻表限速（并发同 host 相邻请求间隔 ≥ min_interval）、429 冷却（响应 Retry-After 头解析，缺省 60s）、裸 403 连击达阈值自动升级 TLS 指纹引擎（`needs_tls_fingerprint`）；classifier `Detection` 新增 `retry_after_secs`
- **per-task BrowserContext 隔离**（默认开启，`context_isolation` 开关）：cookie/localStorage/indexedDB 跨任务串扰一次性解决；TabPool 复用路径保留为逃生门
- **DNS per-key single-flight**：同 host 并发 miss 只放行一次实际解析，同批 waiter 共享结果（失败也共享，消除 K×超时的失败尾部延迟放大）
- **WebGL/canvas stealth**（`webgl_overrider.js`）：WebGL vendor/renderer 指纹覆盖（Proxy 包装抗 toString 检测）+ canvas 读取噪声；toDataURL 仅作用于已持有 2d context 的画布，不物化 context 类型
- **robots fail-closed 开关**（`robots.fail_closed`，默认 false 保持 fail-open）+ `/v1/scrape` 请求级 `respect_robots` 覆盖
- **exa/tavily 条件注册**：`CRAWLRS__SEARCH__EXA__API_KEY` / `CRAWLRS__SEARCH__TAVILY__API_KEY` 配置后自动注册进 SearchClient 单引擎路径（live 联调待真实 key）
- **baidu mu/data-url 链接解包**：真实 URL 参与跨引擎去重；属性缺失时保留中转链接（302 跟随经论证不做）
- **增量抓取条件请求**（scoped 切片，`options.conditional_request`）：同任务重跑时自动携带 If-None-Match / If-Modified-Since；304 复用历史结果内容（`x-crawlrs-not-modified: true` 标记），无 schema 变更
- **HostDiscipline 惰性清扫**：60s 间隔全表回收中性条目，修 state 只增不删的内存无界增长

### Fixed

- DNS single-flight 同批 waiter 结果共享（take→clone：此前第二个 waiter 读到空槽会重新解析）

### Fixed（rag 特性编译修复 + 迁移前置特性）

- **`--features rag` 恢复可编译**（`6d295bd7`）：vecboost 0.3.0-rc.1 与 confers 0.6.0-rc.6 加密 API 不匹配（`derive_field_key` 返回 `Zeroizing<[u8; 32]>`、`decrypt` 返回 `Zeroizing<Vec<u8>>`）导致该特性组合无法编译，`vecboost_provider` 同步适配 confers rc.6 后的 ModelConfig 新字段；本地一等仓库 vecboost 适配 rc.6（`../vecboost` 提交 6438c1d + hf-hub 非 http 构建警告清偿），crawlrs 以 `[patch.crates-io]` 指向本地修复版（Cargo.toml `[patch.crates-io] vecboost = { path = "../vecboost" }`），上游发 rc.2 后可移除
- `dbnexus` 依赖补 `auto-migrate` 特性（migrate 子命令前置，`migration` 之上纯增量）

### Fixed（LLM 活体提取路径接线）

- **分块/溯源接入活体提取路径**（`ae1a3bb0`）：LLM 分块与字符级溯源接入活体提取路径，直调 max_tokens 封顶与可配置超时

### Fixed（fix-audit-mediums-r2：审计中危改进项 7 项 + 独立复查修复 9 项）

- **smart fan-out per-engine 超时**：单引擎 hang 按失败降级（此前路由重试叠加可远超 30s 拖死整体聚合）
- **/v1/search 引擎层 locale/country 支持**：bing 引擎 layer 支持 mkt（lang+country 齐备）+ Accept-Language 联动与 `first=` 页码推导（commit 8e8ca660）。**服务层桥接尚未接通**：`perform_search` 的 `lang`/`country` 参数当前被忽略，API 传入值不生效（见 `docs/USER_GUIDE.md` 搜索节与 `docs/API_REFERENCE.md` 参数表）
- **跨引擎标题 SimHash 去重**：RRF 只按 URL 合并，同内容不同 URL 的跨引擎重复现在被兜底合并
- **出站 per-engine 最小间隔限速**（bing 1s / baidu 2s / sogou 2s）：防止高频聚合把搜索引擎自己打进封禁
- **URL 归一化剥除已知 tracking 参数**（utm_* / fbclid / gclid / msclkid / dclid / twclid / mc_eid / _ga）：营销变体 URL 不再重复抓取
- **robots 匹配携带 query**：`Disallow: /*?s=` 类规则恢复生效
- **host 反爬记忆**：检出挑战页的 host 在 TTL（10 分钟）内直接按 needs_js 路由，HTTP 引擎不再每次撞墙；过期自动重探
- **独立复查修复**：恢复被 typos hook 损坏的 `_abck` 测试与注释；tab 归还不再调用浏览器级清 cookie（防并发 tab 互毁会话）；DomStable 超预算降级为 Ok + wait_for 钳制 10s（此前 API 合法输入确定性失败）；DFS 键改 `priority-(depth+1)`（默认 priority=0 下不再塌缩回 BFS）；race 全候选被挑战时按 needs_js 重派一轮并保留 AntiBotDetected 诊断；Bing 挑战判定移到解析后（防正常结果页误判 Captcha）；trafilatura 正文 HTML 相对链接以页面 URL 绝对化；dedup 对 baidu.com/link 豁免 query 剥除（防全结果坍缩单 key）；PlainText 降级分支行首元字符最小转义

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

### Fixed（清理）

- 删除 `TextEncodingProcessor` dead code（`#[allow(dead_code)]` 标注的未使用结构体）
- 删除 `make_queue()` 测试 helper（clippy never used 告警）

### Security

- **SSRF 旁路接通**（`1ee28a93`）：`CRAWLRS_DISABLE_SSRF_PROTECTION` 开关常量与 config_validator 启动告警此前已就位，但 `SsrfValidator::validate` 仍按主机黑名单 + 内网判定拦截、开关形同虚设（活体探针联调本机 LLM 端点等无法放行）。本次在 Step 5 主机黑名单之前接通旁路——仍解析 DNS 维持 ValidatedUrl / 引擎 DNS-pin 形状，仅跳过黑名单与内网判定；重定向策略同步降级为普通跟随；进程内只 warn 一次，非 dev/test 环境由 config_validator 启动期显著告警
- **CWE-798 回归测试**（`e466b1b6`）：历史硬编码默认 webhook secret（`default-webhook-secret`）被原样配进生产同样视为未设置、必须阻断启动（`config_validator.rs` 回归用例钉住该分支；此前仅测空串/非空）

## [0.2.0-rc.2] - 2026-09-22

### Changed
- **自研依赖全面切换 crates.io registry**：confers/inklog/dbnexus/sdforge/oxcache/limiteron → 0.x.0-rc.5、trait-kit → 0.5.0-rc.6、garrison → 0.9.0-rc.2、vecboost → 0.3.0-rc.1；移除全部 `../base`、`../garrison` 跨仓 path 依赖与 `[patch.crates-io]` 段，CI 不再依赖兄弟仓 checkout
- 合并 github/main 双远端分叉历史（文档双远端对齐、BDD 验收线素材保留于历史）

### Fixed
- `audit_service` 错误映射：`RepositoryError::DatabaseError` 双态（platform 重建 `DbErr::Custom`，轻量面 `Database(String)`），保住 `Database` 变体语义
- i18n locale 测试环境竞态（`CRAWLRS_LANG` 进程级 env 经 ENV_MUTEX 串行化 + 显式清理）
- rustdoc 1.98 严格模式 88 处（裸链接转代码字体/全路径、懒续行缩进、裸泛型 HTML 转义）
- clippy 1.98：`result_large_err` 显式豁免、`cmp_owned` 消除

## [0.2.0-rc.1] - 2026-08-26

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
