# 📋 Crawlrs 更新日志

本文件记录 crawlrs 的全部重要变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/spec/v2.0.0.html)。

## 📋 目录

- [Unreleased](#unreleased)
- [0.2.0-rc.2 - 2026-09-21](#020-rc2---2026-09-21)
- [0.2.0-rc.1 - 2026-08-25](#020-rc1---2026-08-25)
- [0.2.0 - 2026-07-29](#020---2026-07-29)
- [0.1.0 - 2026-07-22](#010---2026-07-22)

---

## [Unreleased]

### Breaking（auth-off 安全语义恢复——文档审计缺陷 #6）

- **auth-off protected routes 恢复默认拒绝（401）**：恢复 v0.2.0-rc.1 语义（rc.2 路由装配重构时意外丢失且无记录）。`auth` feature 关闭时，protected 路由默认 401；嵌入方须显式调用 `crawlrs::presentation::middleware::auth_middleware::allow_unauthenticated_protected()` 放行（注入匿名受限身份，模板 scope 由 `full_access()` 改为 `denied()`，一次 `error!` 日志声明降级）。默认构建（platform 隐含 auth）不受影响
- **恢复 `web-axum` 构建面**：platform 隐含 auth，auth-off 的降级路径（`default_identity_middleware` / `allow_unauthenticated_protected`）在 rc.2 之后的特性矩阵中**不可能编译**（presentation 挂 platform → 必含 auth），系 rc.1 移除 web-axum 后的失修面。现按 rc.1 机制恢复：`--no-default-features --features web-axum,db-postgres --lib` 构建/测试 auth-off 语义（presentation/bootstrap/workers/di/queue/infrastructure 等 lib 门控同步展开，`CrawlRsError::IntoResponse` 与 Database 变体双面覆盖），并在该轴补跑此前从未能运行的 auth-off 中间件测试

### Added（文档审计缺陷清偿 #1-#4）

- **`crawlrs migrate` 子命令**（#1）：生产 schema 初始化一等入口，按序应用 `migrations/*.sql`（经 dbnexus `auto-migrate` 特性 `DbPool::run_migrations`）；目录缺省 `./migrations`，`CRAWLRS_MIGRATIONS_DIR` 可覆盖；README 双语同步
- **`POST /v1/scrape/{id}/_cancel` 生产路由注册**（#2）：此前 handler 完整实现却仅注册在测试路由，生产无法取消单个抓取任务（仅能批量取消）；补路由组注册 + 注册探活测试
- **搜索配置生效**（#3）：`search.default_engine`（单引擎兜底路径默认引擎，须已注册否则 warn 回退 bing）与 `[search.engines] bing/baidu/sogou_enabled` 注册开关（关闭的引擎不注册，smart 聚合与单引擎路径均不可用）此前为死配置无代码消费，现由 `SearchClient::with_configured_engines` 消费；default.toml / .env.example 同步（移除不存在的 google 键）；新增 i18n 日志键 4 个（en/zh）
- **`ScrapeOptionsDto` 暴露 `block_ads` / `block_media` / `needs_mllm`**（#4）：引擎层能力此前已在，仅 DTO 未暴露（CHANGELOG 曾宣称未落地）；use-case 与 worker 双路接线；`needs_mllm` 在无 `llm` 特性构建下提交期 422 显性拒绝（worker 侧兜底任务级显性失败），不再静默降级到普通引擎；`session_id` 仍为内部字段（文档注明）
- **forge_api 注册面纳入编译**（#5）：`presentation/forge_api`（sdforge inventory 单一注册面迁移，在途）此前未在模块树声明——不参与编译、随 handler 演进静默腐烂。新增 `forge-full` 特性作为迁移停机坪：模块经 `--features forge-full` 编译并接受路由等价性锚定测试（inventory 探活 + 生产路径子集断言 + 已知缺口钉住）。inventory 链接期全局收集，激活态与手写 protected/v2 路由组互斥（并存即启动 panic `Overlapping method route`），整体切换需三族中间件（protected/v2/sdk）统一排序的设计裁决，待后续变更实施

### Fixed（rag 特性编译断裂 + 安全收尾）

- **`--features rag` 恢复可编译**：vecboost 0.3.0-rc.1 与 confers 0.6.0-rc.6 加密 API 不匹配（`derive_field_key` 返回 `Zeroizing<[u8; 32]>`、`decrypt` 返回 `Zeroizing<Vec<u8>>`）在 HEAD 即断裂；本地一等仓库 vecboost 适配 rc.6（`../vecboost` 提交 6438c1d + hf-hub 非 http 构建警告清偿），crawlrs 以 `[patch.crates-io]` 指向本地修复版，上游发 rc.2 后可移除
- `dbnexus` 依赖补 `auto-migrate` 特性（migrate 子命令前置，`migration` 之上纯增量）

### Changed（依赖升级与特性显式化清洗）

- **自研 rc 链跟进**：dbnexus 0.6.0-rc.5→rc.6、confers 0.6.0-rc.5→rc.6、inklog 0.3.0-rc.5→rc.6（rc.7 fast-masking 在日志行 fancy_regex BacktrackLimitExceeded 时 panic，刻意不升、锁文件钉 rc.6，见 Cargo.toml inklog 注释）、trait-kit 0.5.0-rc.6→rc.7、oxcache 0.5.0-rc.5→rc.6、limiteron 0.3.0-rc.5→rc.6、sdforge 0.5.0-rc.5→rc.6；garrison 0.9.0-rc.2 / vecboost 0.3.0-rc.1 已是最新 rc 不动
- **三方依赖升最新稳定版**：quick-xml 0.41→0.42（`LocalName::as_ref` 改返 `&str`、`xml10_content` 改返 `Cow<str>`，map_use_case sitemap 解析适配）、rig-core 0.42→0.44（0.43 wire/driver 新架构：rig_embedding_provider 重写为 `OpenAIConfig::new` + `client.embedding(model, Some(ndims)).call(texts)`，并适配 0.43 引入、0.44 延续的显式 ndims 回包宽度校验——宽度不符显性报错而非静默返回）、uuid 1.26→1.27
- **全量特性显式化**：`[workspace.dependencies]` + 全部直接依赖 + dev-dependencies + examples 关闭默认特性、声明最小特性集；实质裁剪：axum 去 form/matched-path/original-uri/tower-log/tracing（全仓零引用）、sysinfo 仅留 system、genai 仅 rustls-tls、futures 去 executor、criterion 去 rayon/plotters、chromiumoxide 显式 bytes、base64 剥离 simd-unsafe、aho-corasick 显式 std+perf-literal、testcontainers/testcontainers-modules 剥离默认 ring；chrono 显式补 clock+std（此前 `Utc::now` 靠依赖图 feature 并集隐式成立）；plotters、tracing-futures、as-any 与 sysinfo 0.38 双版本退出依赖树
- **CI actions 升级**：codecov-action v7.0.0→v7.1.1、setup-rust-toolchain v1.17.0→v2.0.0、codeql-action v4.38.0→v4.38.2、action-gh-release v3.0.2→v3.0.3、taiki-e/install-action→v2.87.26、dtolnay/rust-toolchain master SHA 刷新；checkout v7.0.1 / rust-cache v2.9.2 / cargo-deny-action v2.1.1 已是最新未动
- **钉版佐证维持**：wreq 6.0.0-rc.31（5.x 稳定版全部 yanked，rc.31 仍为最新可用）、testcontainers 0.27（testcontainers-modules 0.15.0 硬依赖 ^0.27.0，cargo outdated 强升 0.28 即版本冲突）、utoipa 5.5（自研 sdforge 0.5.0-rc.6 optional 依赖 ^5.5 并 `pub use utoipa` re-export，升 6.0 双 utoipa 共存触发 E0659 歧义、与 forge 宏生成代码断裂）
- **已知事项**：`--features rag` 组合预存不可编译——vecboost 0.3.0-rc.1 内部代码与 confers 0.6.0-rc.6 `derive_field_key` 返回 `Zeroizing<[u8;32]>` 不匹配（HEAD 即复现，非本次引入，待 vecboost 发版）；RUSTSEC-2023-0071（rsa）/2024-0436（paste）/2025-0119（number_prefix）维持 deny.toml 既有 ignore 与 CI 前提校验
- 验证：`cargo check`（no-default / default / full / platform / openapi / engine-tls-fingerprint）全绿；`cargo clippy --features default|full -- -D warnings` 全绿；`cargo fmt --all -- --check` 通过；`cargo check --features full,test-mocks --tests` 全部测试目标编译通过；`cargo test --lib`（standard 6112 / full 6163）0 失败；rag-remote 嵌入单测（wiremock 契约）4/4 通过；`cargo deny check advisories` ok

### Changed（ID 体系全量迁移至 NebulaId）

- **统一 ID 承载体**：新增 `domain::id::Id`（内部 `u128`，包裹 `nebulaid::core::Id` 语义），为迁移前 `uuid::Uuid` 的全项目替换物；补齐 `Copy/Hash/Default/Ord/FromStr` 并以 UUID 字符串 serde（绕开孤儿规则，保持 API JSON 契约不变）
- **生成引擎切换**：全项目 ID 生成经本地 `../NebulaId`（`nebulaid` path 依赖，`default-features=false, features=["sdk"]`）。默认算法 `uuid_v8`（生成 UUID 兼容值 → DB `UUID` 列与 API JSON 契约零改动、无 schema 迁移）；`snowflake` 为可配置备选（`[id].algorithm`）
- **生成入口**：`IdService::next_id().await`（async，经 DI 注入）+ 同步 `Id::generate()`（进程级预取池 + 后台补水的同步门面，覆盖 `impl Default`/结构体字面量/同步 mapper 等无法 await 的上下文，作为 `Uuid::new_v4()` 直接替换物）
- **装配**：`IdModule` 在启动期构建 `NebulaIdKit` 并注入 `CrawlRsState`（API 与 worker 两种 personality 共用同一装配点）；配置新增 `[id]` 段（`CRAWLRS__ID__*`）
- **持久化边界**：实体层保持 `uuid::Uuid`/`UUID` 列不变，仅 repository/mapper 边界做 `Id↔Uuid` 转换；`impl From<Id> for Value` 使 `Id` 成为 sea-orm 一等查询值
- 验证：`cargo check --lib/--bins`（default + full）全绿；`cargo clippy -- -D warnings`（default + full）全绿；`cargo fmt --all -- --check` 通过；`cargo deny check`（advisories/bans/licenses/sources）全绿；`cargo test --lib --features default` 5981 通过 / 0 失败；外部 `tests/`、`benches/` 编译全绿

### Added（fix-audit-mediums-r3：缓办项落地 + 评审 LOW 收尾）

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

### Fixed（fix-audit-mediums-r2：审计中危改进项 7 项 + 独立复查修复 9 项）

- **smart fan-out per-engine 超时**：单引擎 hang 按失败降级（此前路由重试叠加可远超 30s 拖死整体聚合）
- **/v1/search lang/country/offset 端到端生效**：bing first= 翻页 + mkt（lang+country 齐备）+ Accept-Language 联动；baidu 页码推导（此前参数被丢弃、bing/baidu 固定首页）
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

### Changed

- `/v1/search` 无 `engine` 参数时行为从"单引擎 bing"变为 smart 聚合（见 docs/API_REFERENCE.md Search API）
- **auth-off 语义回退说明**：0.2.0-rc.1 曾将 auth-off 的 protected routes 收紧为默认 401 + `allow_unauthenticated_protected()` 显式 opt-in；rc.2 期间路由装配重构后未保留该收紧，当前实现恢复为直通注入 `full_access()` 固定身份（`bootstrap/routes.rs::build_default_identity_template`）

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

### Changed（代码架构）

- **代码架构优化**:
  - `router.rs` 拆分为 `engine_selector` / `route_sequential` / `route_race` 子模块
  - `scrape_worker.rs` 拆分为 `scrape_task` / `crawl_task` / `extract_task` + `builder` + `deps`
  - `ScrapeWorkerDeps` 参数对象化（消除 16 参数函数签名）
  - `webhook_service.rs` 拆分出 `webhook/management.rs`
- **测试代码提取**: 5 个模块内联测试提取到独立文件（scrape_worker/router/engine_client/webhook_service/task_repo）
- **统一测试 Mock**: `tests/common/mocks/` 共享 MockTaskRepository/MockScraperEngine/MockWebhookService 等

### Fixed（清理）

- 删除 `TextEncodingProcessor` dead code（`#[allow(dead_code)]` 标注的未使用结构体）
- 删除 `make_queue()` 测试 helper（clippy never used 告警）

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
- auth-off 下 protected routes 默认 401（曾依赖未鉴权全开的嵌入方需显式 opt-in；注：该收紧在后续 Unreleased 中回退，见 Unreleased → Changed）

## [0.2.0] - 2026-07-29

### Added

- Garrison RBAC 集成：使用 garrison `ApiKeyHandler::verify_with_namespace` 直接获取 `login_id` 与 `scopes`，废弃 `GarrisonUtil::check_api_key` + session 查找链路（解决 garrison 内存 DAO 下 session 缺失导致的 login_id=None 401 问题）
- `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` 环境变量：服务启动时注入 bootstrap admin key，同时写入 garrison DAO 与 crawlrs `api_keys` 表 + 初始化团队，解决 garrison 内存 DAO 的跨进程 key 丢失
- Axum 中间件链路校准：`create_v2_routes_with_state` 中先 `.layer(team_semaphore_middleware)` 再 `.layer(auth_middleware_inner)`（后 .layer = outer = 先执行），确保 `AuthState.team_id` 在信号量提取前已注入
- `team_semaphore_middleware`：`extract_team_id` 改为从 `AuthState` 读取 `team_id`（替代直接读取裸 `Uuid` Extension），修复 v2 路由 401
- API 端点 `DELETE /v1/crawl/{id}`（取消 crawl）替代已废弃 `POST /v1/scrape/{id}/_cancel`
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
