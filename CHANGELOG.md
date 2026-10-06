# Changelog

## [Unreleased]

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
