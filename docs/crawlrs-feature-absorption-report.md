# crawlrs 功能吸收调研报告

**日期**：2026-10-03

**调研方法**：以 crawlrs 仓库现状为基线，广度调研 41 个开源采集/抽取/反爬候选项目（其中 25 个为清单外新发现），对 15 个头部项目深潜拆解 150 个候选功能点（全部为已发布实现），逐条对照仓库生成差距矩阵（40 行，附于附录 D），收敛出 12 条功能吸收清单。

**验证状态**：12 条吸收点共 32 条来源引用经独立复核全部通过（verdict=verified）；基线已对照 crawlrs 仓库核实，漂移 10 处（列于附录 A）；另有 2 条候选吸收点因来源复核未通过被移出清单（列于 4.2）；成稿及修订时共做 14 项抽样复核（结果见附录 A 末）；本次修订据第 12 项复核将吸收点 #11 的差距定级由「部分有」修正为「缺失」（map_use_case.rs 为未接线孤儿文件）。第三节表格仅收录 factLevel=已发布 的内容；factLevel、verdict、S/M/L 成本分级等术语定义见附录 E。

---

## 一、候选总表

共 41 个候选项目；「（清单外新发现）」表示种子清单之外的调研新发现（共 25 个）。

| 项目 | 链接 | 语言 | Stars | 最近提交 | 定位 | 覆盖类别 |
|---|---|---|---|---|---|---|
| Scrapy | [scrapy/scrapy](https://github.com/scrapy/scrapy) | Python | 64.6k | 2026-10-03 | Python 生态事实标准的通用爬取框架，以调度器、下载器中间件与 item pipeline 为核心，适合大规模结构化抓取工程。 | 直接同类、采集运维 |
| Crawlee | [apify/crawlee](https://github.com/apify/crawlee) | TypeScript | 26k | 2026-10-02 | Node/TS 爬虫框架，统一 HTTP/Puppeteer/Playwright 三类 crawler 抽象，内置请求队列、代理轮换与反阻塞。 | 直接同类、采集运维 |
| colly | [gocolly/colly](https://github.com/gocolly/colly) | Go | 25.5k | 2026-09-28 | Go 轻量爬虫框架，基于回调链的 DOM 选择抓取，并发模型简洁、单二进制零外部依赖部署。 | 直接同类 |
| spider-rs | [spider-rs/spider](https://github.com/spider-rs/spider) | Rust | 2.8k | 2026-10-01 | Rust 全站爬虫库，异步高并发链接发现与抓取、可选 Chrome 渲染，主打低延迟全站数据采集，是 crawlrs 在 Rust 侧最近的同类。 | 直接同类、采集运维 |
| Firecrawl | [firecrawl/firecrawl](https://github.com/firecrawl/firecrawl) | TypeScript | 188.2k | 2026-10-03 | 网页转 LLM 就绪 Markdown 的抓取服务，可自托管（API + Playwright worker + Redis 队列），与 crawlrs 的平台化采集定位直接重叠。 | 直接同类、AI 原生抓取 |
| Scrapling | [D4Vinci/Scrapling](https://github.com/D4Vinci/Scrapling) | Python | 85.4k | 2026-10-03 | 自适应 Python 抓取框架（2024-10 创建），元素定位带智能回溯——页面结构变更后仍能重新定位目标，覆盖单请求到全站爬取。（清单外新发现） | 直接同类、AI 原生抓取 |
| AgenticCrawler | [Mingye-Lu/AgenticCrawler](https://github.com/Mingye-Lu/AgenticCrawler) | Rust | 12 | 2026-10-01 | 2026-03 创建的极早期 Rust 项目 acrawl（仓库名 AgenticCrawler）：自然语言描述→结构化数据的 LLM 爬虫，单二进制、内置 25 家 LLM provider 与 MCP server，与 crawlrs 的 LLM 提取方向同路。（清单外新发现） | 直接同类 |
| crawlee-python | [apify/crawlee-python](https://github.com/apify/crawlee-python) | Python | 9.6k | 2026-10-02 | Crawlee 官方 Python 版（2024-01 创建），Parsel/BeautifulSoup/Playwright 多引擎统一 API，带代理轮换，面向 AI/RAG 数据采集。（清单外新发现） | 直接同类、采集运维 |
| crawl4ai | [unclecode/crawl4ai](https://github.com/unclecode/crawl4ai) | Python | 84.7k | 2026-09-23（Release v0.9.4） | 开源 LLM 友好爬虫，任意网站转干净 Markdown（保留标题/表格/代码/引用提示）供 RAG 与 agent 使用，异步并行加结构化抽取且可完全自托管 | AI 原生抓取 |
| Scrapegraph-ai | [ScrapeGraphAI/Scrapegraph-ai](https://github.com/ScrapeGraphAI/Scrapegraph-ai) | Python | 31.5k | 2026-09-25 | 用 LLM 与有向图逻辑构建抓取管线的 Python 库，自然语言描述即可生成对网站和本地文档的抽取流程 | AI 原生抓取 |
| reader | [jina-ai/reader](https://github.com/jina-ai/reader) | TypeScript | 12.1k | 2026-05-22 | 给任意 URL 加 r.jina.ai/ 前缀即返回 LLM 友好文本的开源转换服务，零集成成本，适合喂给 RAG 与 agent | AI 原生抓取 |
| agentql | [tinyfish-io/agentql](https://github.com/tinyfish-io/agentql) | Python | 1.5k | 2026-10-02 | 用自然语言查询定位网页元素并抽取数据的工具套件，定位是给 LLM 和 AI agent 提供确定性的 Web 接入层 | AI 原生抓取 |
| langextract | [google/langextract](https://github.com/google/langextract) | Python | 38.9k | 2026-09-21 | Google 开源的 LLM 结构化信息抽取库，用 few-shot 示例定义抽取目标，每个抽取结果锚定到原文精确位置（source grounding）并可视化。（清单外新发现） | AI 原生抓取 |
| browser-use | [browser-use/browser-use](https://github.com/browser-use/browser-use) | Python | 117.1k | 2026-10-03 | 让 LLM 直接操控真实 Chromium 完成网页任务的 agent 框架，自然语言驱动点击/填表/导航来获取数据，是 agentic 采集路线的代表项目。（清单外新发现） | AI 原生抓取、浏览器自动化与编排 |
| curl-impersonate | [lwthiker/curl-impersonate](https://github.com/lwthiker/curl-impersonate) | C | 7.1k | 2024-03-03（此后停滞；仓库未 archived） | 打补丁的 curl 构建，TLS/HTTP2 握手与 Chrome/Edge/Safari/Firefox 逐字节一致，是指纹伪装 HTTP 客户端的基石；但原仓库 2024-03 后无提交，活跃延续已转移至 lexiforest fork 及其 Python 绑定 curl_cffi（活跃延续侧最近提交核实至 2026-10-02）。 | 反爬与隐身 |
| camoufox | [daijro/camoufox](https://github.com/daijro/camoufox) | C++ | 12.3k | 2026-09-30 | 基于 Firefox 引擎在 C++ 层做指纹伪造的开源反检测浏览器，不经 JS 注入因而规避最常见的注入检测向量，经 Python/TS 启动器配 Playwright 接口使用。 | 反爬与隐身 |
| wreq | [0x676e67/wreq](https://github.com/0x676e67/wreq) | Rust | 1.1k | 2026-10-02 | reqwest 的硬 fork，Rust 原生可定制 TLS/JA3/JA4/HTTP2 指纹并按 Chrome/Firefox/Safari 档位仿真，是与 crawlrs 技术栈最贴近的指纹客户端（原 rnet 项目已迁回/迁移至 wreq 体系，penumbra-x/rnet 现为 0 stars fork）。（清单外新发现） | 反爬与隐身 |
| tls-client | [bogdanfinn/tls-client](https://github.com/bogdanfinn/tls-client) | Go | 1.9k | 2026-09-04 | net/http 风格的 Go 客户端，可指定浏览器 TLS 指纹档位并支持 HTTP/1.1/2/3 与代理，是 Python hrequests 等工具的底层指纹引擎。（清单外新发现） | 反爬与隐身 |
| patchright | [Kaliiiiiiiiii-Vinyzu/patchright](https://github.com/Kaliiiiiiiiii-Vinyzu/patchright) | TypeScript | 4.8k | 2026-10-01 | Playwright 的 undetected 补丁版（Python/NodeJS/.NET 三发行），在驱动层消除 Runtime.enable 等 CDP 泄漏，API 与 Playwright 完全兼容可平替；仅支持 Chromium 内核。（清单外新发现） | 反爬与隐身 |
| nodriver | [ultrafunkamsterdam/nodriver](https://github.com/ultrafunkamsterdam/nodriver) | Python | 4.8k | 2026-05-13 | undetected-chromedriver 的后继者，Python 异步库不经 WebDriver 标志、直接以 CDP 驱动 Chrome 来绕过 Cloudflare/hCaptcha 类检测；AGPL-3.0 许可。（清单外新发现） | 反爬与隐身 |
| ddddocr | [sml2h3/ddddocr](https://github.com/sml2h3/ddddocr) | Python | 14.8k | 2026-03-06（约 7 个月前，略超 6 个月活跃窗口） | 离线通用验证码识别 Python 库，一个 API 覆盖文本 OCR、目标检测、滑块缺口定位三类场景，是中文互联网验证码识别的事实标准。（清单外新发现） | 反爬与隐身 |
| proxy_pool | [jhao104/proxy_pool](https://github.com/jhao104/proxy_pool) | Python | 23.7k | 2026-06-15 | 定期抓取并校验免费匿名代理、经 Flask API/CLI 供爬虫轮询取用的开源代理池，是该方向 stars 最高的实现；面向免费代理采集而非商业住宅代理上游编排。（清单外新发现） | 反爬与隐身 |
| trafilatura | [adbar/trafilatura](https://github.com/adbar/trafilatura) | Python | 6.9k | 2026-10-02 | 网页正文与元数据抽取的事实标准库，去噪算法经多轮学术基准对比验证，直接输出 XML/JSON/Markdown/CSV 等多格式。 | 结构化抽取与清洗 |
| readability | [mozilla/readability](https://github.com/mozilla/readability) | JavaScript | 11.5k | 2026-08-04 | Firefox 阅读视图背后的正文抽取库，DOM 节点评分算法是各语言 readability 移植版本的共同源头。（清单外新发现） | 结构化抽取与清洗 |
| defuddle | [kepano/defuddle](https://github.com/kepano/defuddle) | TypeScript | 9.6k | 2026-09-28 | Obsidian 作者 2024 年推出的正文抽取库，直接输出 Markdown，规则集比 Readability.js 更针对现代页面，附带 CLI。（清单外新发现） | 结构化抽取与清洗 |
| dom_smoothie | [niklak/dom_smoothie](https://github.com/niklak/dom_smoothie) | Rust | 223 | 2026-09-21 | Mozilla Readability 的 Rust 移植，构建在 dom_query/html5ever 解析栈上，适合直接嵌入 Rust 采集管道做主内容抽取。（清单外新发现） | 结构化抽取与清洗 |
| resiliparse | [chatnoir-eu/chatnoir-resiliparse](https://github.com/chatnoir-eu/chatnoir-resiliparse) | Rust | 150 | 2026-08-18 | Common Crawl 级别的网页存档解析与清洗工具集（Rust 内核），提供高性能宽容 HTML 解析、正文 html2text 抽取与语言检测。（清单外新发现） | 结构化抽取与清洗 |
| html-to-markdown | [JohannesKaufmann/html-to-markdown](https://github.com/JohannesKaufmann/html-to-markdown) | Go | 3.8k | 2026-08-03 | 规则可扩展的 HTML 转 Markdown 库，支持整站转换与 GFM 表格等插件规则，适合作为采集数据清洗导出层。（清单外新发现） | 结构化抽取与清洗 |
| AutoSchemaKG | [HKUST-KnowComp/AutoSchemaKG](https://github.com/HKUST-KnowComp/AutoSchemaKG) | Python | 829 | 2026-04-29 | 港科大 2025 年开源的自动知识图谱构建框架，LLM 抽取三元组后经概念化自动归纳 schema，无需预定义本体。（清单外新发现） | 结构化抽取与清洗 |
| tabula | [tsawler/tabula](https://github.com/tsawler/tabula) | Go | 29 | 2026-06-29 | 2025 年新出的纯 Go 文档抽取库，流式 API 从 HTML/PDF/Office 提取文本与表格，带布局分析与 RAG 就绪分块。（清单外新发现） | 结构化抽取与清洗 |
| browserless | [browserless/browserless](https://github.com/browserless/browserless) | TypeScript | 13.8k | 2026-10-02（pushed_at） | Docker 自托管 headless Chrome/Chromium 的 browser-as-a-service 实现，WebSocket/REST 驱动并内置并发池与限流，自托管 BaaS 的事实标准 | 浏览器自动化与编排 |
| Stagehand | [browserbase/stagehand](https://github.com/browserbase/stagehand) | TypeScript | 25.5k | 2026-10-02（pushed_at） | 在 Playwright 之上提供 act/extract/observe 三个 AI 原语的 SDK，LLM 操作可沉淀为可回放的确定性代码 | 浏览器自动化与编排 |
| Skyvern | [Skyvern-AI/skyvern](https://github.com/Skyvern-AI/skyvern) | Python | 23.1k | 2026-10-03（pushed_at） | 用 LLM 加计算机视觉（截图+DOM 双通道定位）把自然语言工作流变成浏览器自动化的 agent 平台，带自托管 Web UI 与工作流编排 | 浏览器自动化与编排 |
| Lightpanda | [lightpanda-io/browser](https://github.com/lightpanda-io/browser) | Zig | 35.9k | 2026-10-03（pushed_at） | Zig 从零实现的 headless 浏览器，只做自动化所需子集，比 Chromium 省内存且快，CDP 兼容可直接替换 Chrome 后端；注意正确仓库路径是 lightpanda-io/browser，seed 中 lightpanda-browser 路径已 404 | 浏览器自动化与编排 |
| chromiumoxide | [mattsse/chromiumoxide](https://github.com/mattsse/chromiumoxide) | Rust | 1.4k | 2026-04-03（pushed_at，恰好 6 个月，活跃度临界） | Rust 版 Puppeteer 风格的 CDP 客户端，async API 加全量 CDP 类型生成，是 Rust 生态里驱动 Chrome 的基础库，与 crawlrs 技术栈直接相关。（清单外新发现） | 浏览器自动化与编排 |
| notte | [nottelabs/notte](https://github.com/nottelabs/notte) | Python | 2k | 2026-10-03（pushed_at） | 把浏览器会话、凭证库与可脚本化 workflow 打包成单一 API 的 agent 基础设施，支持 serverless 部署浏览器自动化函数，会话/上下文复用维度直接对标。（清单外新发现） | 浏览器自动化与编排 |
| rod | [go-rod/rod](https://github.com/go-rod/rod) | Go | 7.1k | 2026-08-11（pushed_at） | Go 语言的 CDP 驱动库，内置请求劫持、Stealth 与常驻浏览器管理等实用模块，Go 生态浏览器自动化的主流选择。（清单外新发现） | 浏览器自动化与编排 |
| scrapyd | [scrapy/scrapyd](https://github.com/scrapy/scrapyd) | Python | 3.1k | 2026-09-21（master 分支） | Scrapy 官方调度守护进程，以 JSON API 提交、排队、管理 spider 任务，Scrapy 分布式部署与任务编排的事实标准。（清单外新发现） | 采集运维 |
| scrapy-redis | [rmax/scrapy-redis](https://github.com/rmax/scrapy-redis) | Python | 5.6k | 2026-09-17（master 分支） | Scrapy 分布式组件集，Redis 共享调度器加请求指纹去重，把单机 Scrapy 横向扩展成多机爬虫集群。（清单外新发现） | 采集运维 |
| browsertrix-crawler | [webrecorder/browsertrix-crawler](https://github.com/webrecorder/browsertrix-crawler) | TypeScript | 1.2k | 2026-10-02（main 分支） | Webrecorder 的浏览器级高保真爬取容器，带 scope 爬取编排、行为模拟与 WARC/WACZ 标准化归档导出，配套 Browsertrix 平台做调度监控。（清单外新发现） | 采集运维 |
| url-frontier | [crawler-commons/url-frontier](https://github.com/crawler-commons/url-frontier) | Java | 66 | 2026-09-22（master 分支） | crawler-commons 出品的 gRPC URL 队列服务规范与参考实现，把 frontier 调度与去重状态外置为独立服务，供 StormCrawler 等分布式爬虫接入。（清单外新发现） | 采集运维 |

---

## 二、头部项目功能详析

全部 15 个深潜项目各拆解 10 个候选功能点（共 150 个；功能点级明细存于调研工作底稿的 dives 数据，收敛产物为附录 D 差距矩阵与第三节吸收清单）。本节呈现吸收清单来源集中度最高的 8 个项目，每条吸收点（#1–#12）至少呈现其一个来源项目；3 个仅作次要来源的项目（#6 patchright、#7 defuddle、#12 browser-use）未设小节，其关键断言在第三节表格内附文件级来源链接。未设小节的另外 4 个深潜项目（Firecrawl、camoufox、Lightpanda、scrapy-redis）的结论体现在第三、四、五节。所有引用功能点 factLevel 均为「已发布」（定义见附录 E）。标注「#N」表示它是第三节吸收清单第 N 条的来源。

### 2.1 Crawlee（[apify/crawlee](https://github.com/apify/crawlee)）

- **定位**：Apify 开源 Node/TS 抓取与浏览器自动化库，统一 HTTP/Puppeteer/Playwright 三类 crawler 抽象，crawlrs 的直接同类。
- **License/活跃度**：Apache-2.0（无传染风险）；25,977 stars，master 最近推送 2026-10-02，稳定版 v3.18.2（2026-09-29），v4.0.0-rc.0（2026-08-13）。
- **关键功能点**（factLevel=已发布）：
  - Session 身份三件套：IP+CookieJar+指纹绑成单一身份轮换，errorScore 记分退休——#2 来源（[session.ts](https://github.com/apify/crawlee/blob/master/packages/basic-crawler/src/internals/session_pool/session.ts)）
  - ThrottlingRequestManager：429 指数退避与 robots Crawl-delay 双时钟取 max，含停滞检测——#5 来源（[throttling_request_manager.ts](https://github.com/apify/crawlee/blob/master/packages/basic-crawler/src/internals/throttling_request_manager.ts)）
  - SitemapRequestLoader：可背压、可断点续爬的流式 sitemap 一等请求源——#11 来源（[sitemap_request_loader.ts](https://github.com/apify/crawlee/blob/master/packages/basic-crawler/src/internals/sitemap_request_loader.ts)）
  - ErrorSnapshotter：sha1 错误签名键去重的失败现场快照（截图+HTML）——#4 来源（[error_snapshotter.ts](https://github.com/apify/crawlee/blob/master/packages/basic-crawler/src/internals/crawlers/error_snapshotter.ts)）
- **值得注意的设计**：AdaptivePlaywrightCrawler 的「渲染预测→10% 抽样实测→在线再训练」闭环中，探测尝试走永不提交的存储事务（rejected transaction），回滚避免污染请求队列（[adaptive-playwright-crawler.ts](https://github.com/apify/crawlee/blob/master/packages/playwright-crawler/src/internals/adaptive-playwright-crawler.ts)）；v4 用 5s/30s 双时间窗聚合 CPU/内存/事件循环/存储四类负载信号做自动伸缩。

### 2.2 Scrapling（[D4Vinci/Scrapling](https://github.com/D4Vinci/Scrapling)）

- **定位**：Python 自适应抓取框架（2024-10 创建）：解析器记忆元素位置、页面改版后自动重定位，覆盖 HTTP 请求、隐身浏览器到 Scrapy 式全规模爬取。
- **License/活跃度**：BSD-3-Clause（无风险）；85,415 stars / 8,750 forks，最新 release v0.4.15（2026-08-23），main 最近 push 2026-10-03，共 1,629 commits。
- **关键功能点**（factLevel=已发布）：
  - AutoThrottle 动态 per-domain 限速：EMA 收敛 + 封锁永不降延迟——#1 来源（[throttle.py](https://github.com/D4Vinci/Scrapling/blob/main/scrapling/spiders/throttle.py)）
  - CheckpointManager 暂停恢复 + 优先级调度器快照——#3 来源（[checkpoint.py](https://github.com/D4Vinci/Scrapling/blob/main/scrapling/spiders/checkpoint.py)）
  - capture_xhr 后台 XHR/Fetch 响应捕获——#6 来源（[_base.py](https://github.com/D4Vinci/Scrapling/blob/main/scrapling/engines/_browsers/_base.py)）
  - 自适应元素重定位：元素属性签名存 SQLite，选择器失配后全树相似度打分重定位（阈值 40%）——crawlrs 无此能力（[parser.py](https://github.com/D4Vinci/Scrapling/blob/main/scrapling/parser.py)）
- **值得注意的设计**：StealthyFetcher 底层 patchright，程序化求解 Cloudflare Turnstile（challenge 分类 + 像素偏移点击）；代理轮换模式下每请求新建 browser context 用后销毁，用 context 生命周期隔离 cookie 与指纹防跨请求串联。

### 2.3 crawl4ai（[unclecode/crawl4ai](https://github.com/unclecode/crawl4ai)）

- **定位**：AI 原生 Python 异步抓取库（库/Docker 自托管/MCP 三形态），全链路围绕「为 LLM 产出 fit markdown 与结构化数据」组织。
- **License/活跃度**：Apache-2.0（无传染性；要求下游保留 attribution 署名）；84,686 stars，v0.9.4（2026-09-23），共 1,697 commits。
- **关键功能点**（factLevel=已发布）：
  - MemoryAdaptiveDispatcher：psutil 每秒轮询，90%/85% 双阈值滞后暂停/恢复派发——#10 来源（[async_dispatcher.py](https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/async_dispatcher.py)）
  - AsyncUrlSeeder/DomainMapper：sitemap + Common Crawl + CT 日志免抓取预热，Soft404Fingerprint 四元组指纹剔除空壳——#11 来源（[domain_mapper.py](https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/domain_mapper.py)）
  - antibot_detector 分层阻断判定：Tier1 厂商指纹任意大小触发、Tier2 通用词仅 <10KB 短页或 4xx/5xx 参与、Tier3 四信号计数 ≥2 判 blocked——可校准 crawlrs 三层检测阈值（[antibot_detector.py](https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/antibot_detector.py)）
  - AdaptiveCrawler 信息充分性最优停止：confidence = 0.4×coverage + 0.3×consistency + 0.3×saturation（[adaptive_crawler.py](https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/adaptive_crawler.py)）
  - Best-first 深度爬取的 resume_state 崩溃恢复：visited/队列/深度全量快照经回调导出、崩溃后重建队列续爬——#3 来源（[bff_strategy.py](https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/deep_crawling/bff_strategy.py)）
- **值得注意的设计**：resolve_and_pin egress 代理一次解析钉扎 IP 拨号，从根上关死 DNS rebinding 窗口，并剥除 `--proxy-server` 等危险浏览器参数（[egress_broker.py](https://github.com/unclecode/crawl4ai/blob/main/deploy/docker/egress_broker.py)）；注：crawlrs `src/workers/crawl/adaptive.rs:28-48` 已实现 confidence/coverage/saturation 三信号停止，此项对 crawlrs 边际价值低（见附录 D 矩阵「信息充分性最优停止」行）。

### 2.4 Scrapy（[scrapy/scrapy](https://github.com/scrapy/scrapy)）

- **定位**：Python 生态事实标准的通用爬取框架：事件驱动 Engine 居中调度 Scheduler 队列，经双向中间件链驱动 Downloader 与 Spider 回调，Items 走可插拔 Pipeline。
- **License/活跃度**：BSD-3-Clause（无风险）；64.6k stars，11,569 commits，v2.19.0（2026-09-10），最近提交 2026-10-03。
- **关键功能点**（factLevel=已发布）：
  - AutoThrottle：target = download_latency/target_concurrency 的 EMA 向上有偏收敛，非 200 禁止降速——#1 来源（[throttle.py](https://raw.githubusercontent.com/scrapy/scrapy/master/scrapy/extensions/throttle.py)）
  - RFPDupeFilter/DiskDupeFilter：SHA-1 指纹 + JOBDIR 追加文件 / SQLite（WAL + WITHOUT ROWID）持久化——#3 来源（[dupefilters.py](https://raw.githubusercontent.com/scrapy/scrapy/master/scrapy/dupefilters.py)）
  - JOBDIR 可恢复调度 + 2.19.0 SQLite 后端队列（逐请求事务落盘，不干净关机可存活）（[scheduler.py](https://raw.githubusercontent.com/scrapy/scrapy/master/scrapy/core/scheduler.py)）
  - HTTP 缓存 RFC2616Policy：新鲜度计算、max-stale 宽限、ETag/Last-Modified 条件请求重验证（[官方文档](https://docs.scrapy.org/en/latest/topics/downloader-middleware.html)）
- **值得注意的设计**：AutoThrottle 防正反馈——非 200 且新 delay 变小时跳过调整，避免错误页越爬越快；2.19.0 RemoteControl 本地 introspection 端点因任意代码执行面被列入不建议吸收（见 4.1）；多版本「去 Twisted 化」验证了 crawlrs 直接以 tokio+reqwest 为底座不需兼容层的路线。

### 2.5 trafilatura（[adbar/trafilatura](https://github.com/adbar/trafilatura)）

- **定位**：网页正文与元数据抽取的事实标准库：四级级联抽取流水线，输出 TXT/Markdown/CSV/JSON/HTML/XML/XML-TEI 七格式；crawlrs 已在用（`trafilatura` feature）。
- **License/活跃度**：Apache-2.0（无风险；注意 v1.8.0 之前旧版本为 GPLv3+，参考实现应以 master 当前代码为准）；6,908 stars，v2.3.0（2026-10-02），最近提交 2026-10-02。
- **关键功能点**（factLevel=已发布）：
  - 四级级联 + recall 升级重试：结果 <3000 字符且不足全文 20% 才升级，替换按长度比 ≥1.5x（justext 需 2.0x）裁决——#7 来源（[core.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/core.py)）
  - 链接密度剪枝 + 过剪回滚：剩余文本不足原文 1/7 判过剪，整体恢复备份（[htmlprocessing.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/htmlprocessing.py)）
  - 中性 XML 中间表示单源渲染七格式；Markdown 序列化内置防注入转义（行内特殊字符转义、fence 长度自适应、GFM 表格转义）（[xml.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/xml.py)）
  - 双层去重：段级 LRU + 文档级 Simhash（64bit，blake2b 位投票 + Hamming 距离）（[deduplication.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/deduplication.py)）
  - 聚焦爬虫读 robots Crawl-delay 决定每页抓取间隔（spider.py:339）——#5 来源（[spider.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/spider.py)）
- **值得注意的设计**：下载层 SSRF 不做预解析 DNS 检查，而是在连接建立后对 `getpeername()` 复核 is_global 防 rebinding；baseline 兜底可从 JSON-LD（articleBody/reviewBody 等）直接救援 JS 渲染页正文（[baseline.py](https://github.com/adbar/trafilatura/blob/master/trafilatura/baseline.py)）。

### 2.6 Scrapegraph-ai（[ScrapeGraphAI/Scrapegraph-ai](https://github.com/ScrapeGraphAI/Scrapegraph-ai)）

- **定位**：LLM + 有向图编排的 Python 抓取库（YOSO，You Only Scrape Once：一次定义图即可完成抓取）：fetch→parse→LLM 提取组成可配置图，28 种图模板，本地开源库与托管云 API 双形态。
- **License/活跃度**：MIT（无风险）；31.5k stars，v2.3.0（2026-09-25），main 约 2,964 commits，活跃维护。
- **关键功能点**（factLevel=已发布）：
  - token 窗口感知分块：chunk_size 取模型上下文窗口（26 供应商×数百模型配置表驱动，查不到回退 8192 并显式标记），semchunk 按 token 切块留 10% 余量——#8 来源（[parse_node.py](https://github.com/ScrapeGraphAI/Scrapegraph-ai/blob/main/scrapegraphai/nodes/parse_node.py)）
  - chunk 级提问后单次 merge prompt 综合（复核注记：上游实际逐 chunk 顺序执行，crawlrs 侧可用 tokio 实现真并行）（[generate_answer_node.py](https://github.com/ScrapeGraphAI/Scrapegraph-ai/blob/main/scrapegraphai/nodes/generate_answer_node.py)）
  - LLM-free 术语预检：prompt 关键词 + schema 字段名拆词均不在分块文本中即告警，推理前零成本拦截「渲染失败白烧 token 得 NA」（[parse_node.py](https://github.com/ScrapeGraphAI/Scrapegraph-ai/blob/main/scrapegraphai/nodes/parse_node.py)）
- **值得注意的设计**：图执行器是纯确定性 while+邻接表；ConditionalNode 用 simpleeval AST 白名单求值防任意代码执行；已知缺陷——DepthSearchGraph 无抓取总量上限、only_inside_links 实现有疑似 bug，其深度爬取弱于 crawlrs 现有过滤链+优先级队列。

### 2.7 langextract（[google/langextract](https://github.com/google/langextract)）

- **定位**：Google 开源的 LLM 文本结构化抽取库：每条抽取结果锚定回源文本精确字符区间（char_interval）；本身无抓取/反爬能力，价值集中在 LLM 提取层之上的证据级溯源与长文召回。
- **License/活跃度**：Apache-2.0（无风险）；38.9k stars / 2.7k forks，v1.7.0（2026-09-13），最近提交 2026-09-21。
- **关键功能点**（factLevel=已发布）：
  - 三层字符级溯源对齐：token 级 DP 单调匹配 → 模糊 LCS 最紧区间，覆盖率 ≥0.75 与密度 ≥1/3 双门，全败置 None 供调用方过滤——#9 来源（[resolver.py](https://github.com/google/langextract/blob/main/langextract/resolver.py)）
  - 句子感知分块 ChunkIterator：按 max_char_buffer 累积完整句子成块，超长回退最近换行符断开，块间无重叠严格推进（[chunking.py](https://github.com/google/langextract/blob/main/langextract/chunking.py)）
  - 多轮独立提取 extraction_passes：首轮优先、字符区间不重叠才合并，提升长文「大海捞针」召回（[annotation.py](https://github.com/google/langextract/blob/main/langextract/annotation.py)）
- **值得注意的设计**：few-shot prompt 对齐预检——推理前用 WordAligner 反向校验示例文本，ERROR 档直接抛 PromptAlignmentError；抽取结果可渲染为单文件自包含交互式 HTML（高亮+播放控件，零外部资源），是审计/人工复核界面的直接参照。

### 2.8 Stagehand（[browserbase/stagehand](https://github.com/browserbase/stagehand)）

- **定位**：Browserbase 出品的 AI 浏览器代理 SDK：observe/act/extract 三原语配合可访问性树裁剪，让 LLM 低 token 可靠操作网页。
- **License/活跃度**：MIT（无风险）；25,525 stars / 1,752 forks，主分支最近提交 2026-10-02，npm 稳定线 3.7.x（v4 开发中）。
- **关键功能点**（factLevel=已发布）：
  - Hybrid DOM+AX 快照：CDP `DOM.getDocument` 与 `Accessibility.getFullAXTree` 以 backendNodeId 对齐成统一大纲，多层裁剪后序列化为 `[id] role: name` 文本——#12 来源（[capture.ts](https://github.com/browserbase/stagehand/blob/main/packages/extension/understudy/a11y/snapshot/capture.ts)；修正后的辅助来源 [a11yTree.ts](https://github.com/browserbase/stagehand/blob/main/packages/extension/understudy/a11y/snapshot/a11yTree.ts)）
  - 「LLM 定位、本地执行」分离：LLM 只在树文本上选元素，返回 elementId 映射为真实 selector 由确定性代码执行；凭据经 `%key%` 占位符本地替换，模型只见占位符（[actService.ts](https://github.com/browserbase/stagehand/blob/main/packages/extension/services/actService.ts)）
  - extract 的 URL EncodedId 占位：LLM 只输出数字 id，完成后换回真实 URL——省 token 且杜绝 LLM 抄错/编造 URL（[extractService.ts](https://github.com/browserbase/stagehand/blob/main/packages/extension/services/extractService.ts)）
- **值得注意的设计**：页面状态（含完整 CDP AX 树）做 key 的服务端结果缓存 + threshold 确认 + 确定性回放，模型配置刻意排除在 key 外；两步 act 间做行级树 diff，只把新增行喂给下一步推理。

---

## 三、功能吸收清单（主交付物）

12 条按优先级升序排列（顺序不改动）；每条来源均经独立复核（verdict=verified：复核者独立打开来源文件逐条核对断言对应的实现与行号，32 条来源引用全部通过）。「crawlrs 差距」中的「部分有/缺失」为对照仓库逐条核实的结论。成本 S/M 为本报告约定的实现成本相对分级（非上游项目口径）：S=单模块内改动、零或一个轻量新依赖；M=需新增内聚模块并与现有架构接线；L=跨模块结构性改动（本清单 12 条均为 S/M，无 L）。表内术语（MLLM、AX 树、AIMD、CT 日志、vecboost 等）见附录 E。

| 优先级 | 功能 | 来源项目 | 解决的问题 | crawlrs 差距 | Rust 实现思路 | 成本 | 来源 license |
|---|---|---|---|---|---|---|---|
| 1 | AutoThrottle 延迟观测自适应 per-host 限速 | [Scrapy](https://github.com/scrapy/scrapy) + [Scrapling](https://github.com/D4Vinci/Scrapling) | 固定 per-host 间隔对快站浪费吞吐、对慢站招封禁；两家头部项目以几乎相同的公式独立实现延迟观测收敛（target = latency/target_concurrency，EMA 向上有偏更新 max((cur+target)/2, target)），证明这是限速的正确形态 | 部分有：host_discipline.rs 仅有静态 min_interval 预约时刻表与 429 冷却（src/utils/host_discipline.rs:63-78），min_interval 不会因观测变快或变慢 | 在 HostDiscipline::observe 增加延迟输入：target = latency/target_concurrency，EMA 更新 max((cur+target)/2, target)，clamp 到 [min_interval, max_interval]；非 200 且新 delay 变小时跳过调整（防错误页越爬越快的正反馈）；顺带加按 host 配置的抖动比例打散请求指纹规律性；纯算法零新依赖 | S | BSD-3-Clause（Scrapy、Scrapling） |
| 2 | Session 身份三件套绑定与记分退休 | [Crawlee](https://github.com/apify/crawlee) | 反爬按「身份」（IP+Cookie+指纹组合）封禁而非按单项；crawlrs 有 UA 池+代理轮换+TLS 指纹但三者互不感知，403 后盲换 UA 不换代理等于白换 | 缺失：UA 池与代理池独立轮换（src/utils/ua_pool.rs、src/engines/proxy_pool.rs），封禁时盲轮换，无法定位哪个身份组合已死 | 新增 Session 结构 {proxy_id, cookie_jar, ua/tls 档}，DashMap 记 error_score：mark_bad +1、mark_good -0.5，score≥阈值或 usage≥上限或超龄即退休换新身份；401/403/429 触发 retire；cookie jar 随身份持久化；挂在现有 proxy_pool/ua_pool 之上，EngineRouter 执行请求时按 Session 取三元组 | M | Apache-2.0 |
| 3 | 爬取断点续爬：持久化 seen + 队列快照 | [Scrapy](https://github.com/scrapy/scrapy) + [crawl4ai](https://github.com/unclecode/crawl4ai) + [Scrapling](https://github.com/D4Vinci/Scrapling) | 大站爬取跑数小时后崩溃即从头再来，浪费全部配额与时间；三家项目各自用 SQLite 指纹库或快照文件解决同一痛点 | 缺失：Deduplicator 为进程内存态，workers 下 grep resume/checkpoint 零命中，worker 崩溃或重启后 seen 与 pending 队列全部丢失重爬 | seen 持久化用 rusqlite（WAL + WITHOUT ROWID，指纹 BLOB 主键 + INSERT OR IGNORE，对齐 Scrapy DiskDupeFilter）；pending 队列+seen 快照用 tempfile+rename 原子写（Scrapling checkpoint 模式），按快照恢复 frontier 出队顺序 | M | BSD-3-Clause（Scrapy、Scrapling）/ Apache-2.0（crawl4ai） |
| 4 | 失败现场快照（错误签名→截图+HTML） | [Crawlee](https://github.com/apify/crawlee) | 反爬坏 case 排障刚需：只有失败计数无法回答「这个站到底返回了什么」；Crawlee 用错误签名键去重存档，存储成本可控 | 缺失：src/infrastructure/observability/ 目录仅 metrics.rs，Prometheus 只有计数没有现场 | 键 = sha1(错误链)[:30] + 清洗后消息[:30]，DashMap 计数防重复存档；浏览器引擎路径存截图 jpg + page content html，HTTP 引擎路径存 response body；挂 metrics feature 同级的 observability 模块，任何内部失败静默返回不影响抓取 | S | Apache-2.0 |
| 5 | robots Crawl-delay 联动限速 + 429 指数连击 + 停滞检测 | [Crawlee](https://github.com/apify/crawlee) + [trafilatura](https://github.com/adbar/trafilatura) | 礼貌爬取的完整语义是「robots 声明的 Crawl-delay 优先 + 被 429 后指数退避 + 长期被限流就换身份」，crawlrs 三处各有半个实现未联动 | 部分有：429 已有固定冷却（连击不动），缺指数推进与同窗 burst 抑制；robots.rs 未把 Crawl-delay 喂给 per-host 时钟；无停滞转代理切换 | per-host 双时钟取 max(backoff_until, crawl_delay_until)；429 指数推进 base×2^n 封顶 60s，同窗多个 429 只推进一次、整窗成功后衰减清零；持续限速超阈值上报 stalled 触发 Session 退休换代理（与第 2 条联动）；Crawl-delay 首值生效不回退 | M | Apache-2.0（Crawlee）/ BSD-3-Clause（trafilatura） |
| 6 | XHR/Fetch 后台响应捕获 | [Scrapling](https://github.com/D4Vinci/Scrapling) + [patchright](https://github.com/Kaliiiiiiiiii-Vinyzu/patchright) | SPA 页面正文在 XHR 接口里，DOM 提取拿到的是空壳；捕获 XHR 可直接命中数据接口，比 MLLM 视觉导航便宜一个量级 | 缺失：src/engines/client/playwright.rs:611-635 已有 EventRequestPaused 事件流与 ResourceKind 分类，但仅做 FailRequest/ContinueRequest，无响应体读取与结果挂载 | 复用现有拦截事件流加 Fetch.getResponseBody 分支，或改订 Network.responseReceived 事件：resource_type ∈ {xhr, fetch} 且 URL 匹配任务配置 pattern 的响应体收进 ScrapeResult.captured_responses；主文档响应按 is_navigation 单独识别；feature 门控为可选能力（patchright 佐证捕获发生在 CDP Network/Fetch 事件层、其动因为反检测补丁，见 [crNetworkManagerPatch.ts](https://github.com/Kaliiiiiiiiii-Vinyzu/patchright/blob/main/driver_patches/crNetworkManagerPatch.ts)） | S | BSD-3-Clause（Scrapling）/ Apache-2.0（patchright） |
| 7 | 提取降级重试阶梯（短结果升级 + 长度比裁决） | [defuddle](https://github.com/kepano/defuddle) + [trafilatura](https://github.com/adbar/trafilatura) | 单轮抽取在坏页（索引页/评论页/懒加载页）上质量不可控且无从知晓；defuddle 的 wordCount 驱动阶梯与 trafilatura 的长度比裁决提供了可量化的升级判据 | 部分有：Trafilatura 库内部有级联，但 crawlrs 的 DomSmoothie/Trafilatura/LLM 三个提取通道互相独立，无「输出字数不足才升级、新结果需 ≥1.5x~2x 长度才替换」的跨通道编排 | 提取编排层加质量门：首轮 DomSmoothie 输出 word_count < 阈值时逐级放宽（关闭部分选择器移除→换 Trafilatura recall 模式→升 LLM）；候选替换按长度比 ≥1.5x 裁决防退化；预置 defuddle 的 prose 硬门槛（≥7 词、句末标点、链接密度 ≤70%，见 [content-boundary.ts](https://github.com/kepano/defuddle/blob/main/src/content-boundary.ts)）做起点判定 | M | MIT（defuddle）/ Apache-2.0（trafilatura） |
| 8 | LLM 提取 token 窗口分块 + 分块提问 + merge + 术语预检 | [Scrapegraph-ai](https://github.com/ScrapeGraphAI/Scrapegraph-ai) | 长文档超上下文窗口即截断丢失尾部内容，或强行塞入导致幻觉；「窗口驱动切块→分块提问→单次 merge→术语预检」是完整吞吐方案，术语预检零成本拦截白烧 token | 缺失：src/domain/services/llm/ 已验证仅 prompt_builder/provider_adapter/vision_adapter 三个文件（另 mod.rs 接口壳），无任何 chunk/分块逻辑，长文单 prompt 直发 | tiktoken-rs 计数按模型窗口收缩 chunk_size（留注入余量），tokio 并发提问后单次 merge prompt 综合（复核修正：上游实际逐 chunk 顺序执行，「并行提问」不成立，crawlrs 侧可用 tokio 实现真并行）；预检用停用词过滤 prompt 关键词+schema 字段拆词，均不在文本中则告警（确定性代码，不交给模型）；窗口表用配置驱动 | M | MIT |
| 9 | LLM 提取结果字符级溯源对齐 | [langextract](https://github.com/google/langextract) | RAG/知识图谱场景需要证据链：LLM 幻觉字段无法溯源即无法入库；langextract 把「精确字符区间」做成一等公民并用覆盖率/密度双门过滤未溯源结果 | 缺失：LLM 提取输出为纯内容级结果，无证据在原文的字符区间，RAG 入库后无法回链验证 | 纯字符串 DP 手写（百行级）：token 级单调递增非重叠匹配 → 失败走模糊 LCS 最紧区间，覆盖率 ≥0.75 且密度 ≥1/3 双门，全败置 None 由调用方过滤；配 regex tokenizer + unicode-segmentation；与 vecboost RAG 入库链直接衔接 | M | Apache-2.0 |
| 10 | 进程内存压力反馈派发门控 | [crawl4ai](https://github.com/unclecode/crawl4ai) | 浏览器引擎多实例内存超限是 OOM kills 的直接前兆；AIMD 管带宽利用、内存门控管存活，两者正交 | 部分有：adaptive_concurrency.rs 的 AIMD 按成功/失败信号调并发上限，但不感知内存水位——Playwright 多实例内存爆炸时 AIMD 只会看到失败率上升而滞后反应 | sysinfo 周期采样 + tokio::sync::watch 广播水位，≥90% 暂停派发、回落 ≤85% 恢复（双阈值滞后防抖），临界档在途任务带 retry+1 重新入优先队列；挂 worker 运行形态，feature 门控 | S | Apache-2.0 |
| 11 | sitemap 流式一等请求源 + 软 404 指纹 | [Crawlee](https://github.com/apify/crawlee) + [crawl4ai](https://github.com/unclecode/crawl4ai) | 大站 sitemap 数十万 URL 预载占内存且中途不可断；SPA 软 404 空壳污染爬取配额与索引 | 缺失（修订）：全仓无 sitemap 请求源——原判「部分有」依据的 src/application/use_cases/map_use_case.rs 经复核为未接线孤儿文件（`grep -rn "map_use" src/` 内容零命中；src/application/use_cases/mod.rs:8-11 仅声明 crawl_link_processor/crawl_state_machine/crawl_use_case/create_scrape 四模块；路由层无 map 端点）；软 404 指纹完全缺失 | quick-xml 流式解析 + tokio mpsc 有界通道背压，边解析边入 frontier，状态排入缓存支持重启续爬；软 404 用 {status_code, title, content_length, body_hash} 四元组指纹剔除空壳，接进现有 URL 过滤链 | M | Apache-2.0 |
| 12 | 混合 DOM+AX 树快照喂 MLLM | [Stagehand](https://github.com/browserbase/stagehand) + [browser-use](https://github.com/browser-use/browser-use) | 截图视觉导航每步 token 成本高且产出不可复用；AX 树文本大纲 token 少一个量级，且 selector 可直接回放给 Playwright 引擎复用 | 缺失：MLLM 引擎（src/engines/client/mllm.rs）走截图视觉通道，无 AX 树文本表示 | playwright/chromiumoxide CDP 会话发 Accessibility.getFullAXTree + DOM.getDocument 对齐 backendNodeId，结构裁剪（无 name 结构叶丢弃、单子提升）后序列化为 `[id] role: name` 文本；作为 MLLM 引擎的页面表示选项与截图并存，feature 门控（注：browser-use 为 DOM 可访问性元素树文本化、非 CDP AX 树，见 [serializer.py](https://github.com/browser-use/browser-use/blob/main/browser_use/dom/serializer/serializer.py)） | M | MIT（Stagehand、browser-use） |

---

## 四、不建议吸收清单

### 4.1 不建议吸收（10 条）

| 功能 | 原因 |
|---|---|
| Firecrawl fire-engine 云端引擎簇（chrome-cdp/tlsclient/scrapePDF） | 闭源云服务，仓库内只有调用壳；crawlrs 吸收不到实现，只能看接口定义。注意 Firecrawl 整体为 AGPL-3.0，engpicker/featureFlags 等条目只借鉴机制思想、禁止移植代码 |
| camoufox Firefox fork 本体（patches/ + LibreWolf 构建链） | crawlrs 是 Rust 单二进制，无法内嵌需要 Firefox 构建树的浏览器 fork，架构完全冲突；若需要其指纹能力，正确姿势是像 FlareSolverr 一样作为外部引擎进程接入 EngineRouter，而非合并源码 |
| Scrapy Spider 类体系 + genspider 模板 + Item Pipeline 代码扩展模型 | Scrapy 是「用户写 Python 类」的代码驱动框架，crawlrs 是任务参数+配置驱动的服务化 API 平台（多租户 RBAC、任务队列），把代码驱动模型搬过来与任务队列/引擎路由架构直接冲突 |
| Crawlee Dataset/KeyValueStore 本地文件存储层 | crawlrs 以 PostgreSQL（Sea-ORM/dbnexus）为主库，多租户存储更完整；Crawlee 的本地 JSON 文件存储是其无服务端场景产物，引入即架构冲突 |
| browser-use 云托管浏览器与云 CAPTCHA 求解 | 依赖 browser-use-sdk 商业 SaaS（约 $0.02/浏览器小时），与自托管成本模型冲突；其激进长程默认值（max_steps=500、ws frame 200MB）面向单 agent 长程任务，照搬会放大单任务故障半径（依据：[agent/service.py](https://github.com/browser-use/browser-use/blob/main/browser_use/agent/service.py)、[browser/session.py](https://github.com/browser-use/browser-use/blob/main/browser_use/browser/session.py)） |
| patchright driver 补丁构建流水线（ts-morph AST 编排 + 协议 YAML 突变 + 自动 diff） | crawlrs 不维护 Playwright fork，正确姿势是直接消费 patchright 预编译 driver 二进制；把补丁流水线搬进 crawlrs 是纯负资产，该流水线只在「自己长期维护上游 fork」时才有价值 |
| Lightpanda V8 自嵌与对外 CDP 服务端 | V8 集成要求自实现整套 DOM/Web API/事件循环（数十万行 Zig），与现有 Playwright 引擎分层冲突；对外提供 CDP/WebDriver 服务端方向也相反——crawlrs 是 REST 采集 API 提供方，可取的只是把 Lightpanda 作为独立进程引擎接入 |
| Scrapegraph-ai robots_node 用 LLM 判定 robots.txt | 抓 robots.txt 再让 LLM 按「no 子串」判断可否抓取，慢、贵且不可靠（返回 know 会误判）；crawlrs 已有 robots.rs 标准解析且 RobotsSettings 含 user_agent 配置，现有实现严格更优 |
| scrapy-redis pickle 请求序列化与无界去重 SET | pickle 是 Python 专属格式且反序列化有代码执行风险（Redis 内容被篡改即成注入面）；其去重 SET 无 TTL 无容量上限只增不减，crawlrs 若做分布式去重须用 serde 二进制 + EXPIRE/分代，机制可借鉴载体不可照抄 |
| Scrapy RemoteControl 运行时控制扩展 | POST /execute 接收源码 eval 执行的形态即使有 127.0.0.1 绑定+恒定时间 token，对多租户采集平台仍是不可接受的任意代码执行面；crawlrs 已有 Prometheus+审计日志承载运维观测，受限 introspection 需另行设计而非照搬 |

### 4.2 未验证-已排除（2 条）

来源复核阶段被移出吸收清单的候选功能点，未进入第三节表格：

| 功能 | 原因 |
|---|---|
| UA 钉扎实际浏览器驱动版本 | 修订时移出：来源复核未通过且无可替换来源 |
| WebBotAuth RFC 9421 请求签名 | 修订时移出：来源复核未通过且无可替换来源 |

---

## 五、更优实现参考

crawlrs 已有能力在候选项目中的更优实现，供比对补强（非新增功能）：

| 能力 | 参考项目 | 差异 |
|---|---|---|
| per-host 限速 | [Lightpanda Browser](https://github.com/lightpanda-io/browser)（src/network/RateLimiter.zig） | crawlrs d95013b2 的预约时刻表与 GCRA（Generic Cell Rate Algorithm，泛型信元速率算法的预约制限速）同构，但 Lightpanda 有 tau=(burst-1)*interval 突发容忍语义（空闲 host 可瞬间吸收 burst 再排开）与 256 条触发惰性 sweep、清完仍超则 sweep_at*=2 的倍增防退化策略，可直接比对补强 |
| DNS single-flight | [Lightpanda Browser](https://github.com/lightpanda-io/browser)（src/network/SingleFlight.zig） | crawlrs fbd27e37 已实现 waiter 共享 leader 结果；Lightpanda 额外约定「移除最后一个 waiter 后 map entry 仍保留（in-flight fetch owns it）」保护 leader 完成分发，且 take() 用 fetchRemove 整表摘除交调用方分发，错误传播职责划分更清晰 |
| 三层反爬检测 | [crawl4ai](https://github.com/unclecode/crawl4ai)（crawl4ai/antibot_detector.py） | crawlrs patterns.rs 的 Tier1/2/3 分层正确，但 crawl4ai 额外有「层级×页面大小门限×信号计数」完整阈值表：Tier1 厂商指纹任意大小触发、Tier2 通用词仅 <10KB 短页或 4xx/5xx 参与、Tier3 四信号计数 ≥2 判 blocked，且 403/503+非数据 HTML 一律判 blocked 的理由（现代 block 页是 >100KB SPA 壳）值得直接校准 crawlrs 阈值 |
| TabPool 标签页池 | [Scrapling](https://github.com/D4Vinci/Scrapling)（scrapling/engines/_browsers/_base.py） | crawlrs tab_pool 跨请求复用页面；Scrapling PagePool 在代理轮换模式下每请求新建 browser.new_context 并用后销毁，用 context 生命周期隔离 cookie 与指纹防跨请求串联，出错或 is_closed 剔除重建——对反指纹串联是更安全的池化策略，可作为代理任务的可选模式 |
| URL 过滤链 | [Firecrawl](https://github.com/firecrawl/firecrawl)（apps/api/native/src/crawler.rs） | crawlrs filters.rs 已有过滤链但被拒 URL 无结构化原因；Firecrawl 每条被拒 URL 带拒绝原因（DEPTH_LIMIT/FILE_TYPE/ROBOTS_TXT 等）回传，且对不可信用户正则加 unicode(false)/size_limit 256KiB/nest_limit 50/有界 DFA 缓存硬限制防 a{5}{5}{5} 展开爆炸——拒绝原因回传与正则硬限制两点更完善 |
| 错误驱动引擎升级 | [Firecrawl](https://github.com/firecrawl/firecrawl)（apps/api/src/scraper/scrapeURL/index.ts） | crawlrs host_discipline.rs:78 已有「裸 403 连击→TLS 指纹升级」单点；Firecrawl scrapeURL 是系统性 per-request featureFlags 重建：401/403/429 自动加 stealthProxy 标志、PDF 反爬错误丢 pdf 标志改浏览器引擎抓文件，外层 while 循环按 AddFeatureError/RemoveFeatureError 改写候选列表重试——单点升级 vs 全特征候选瀑布的差距 |
| 增量抓取缓存语义 | [Scrapy](https://github.com/scrapy/scrapy)（HttpCacheMiddleware 文档） | crawlrs 已有 If-None-Match/If-Modified-Since 条件请求；Scrapy RFC2616Policy 在其上有完整 HTTP 语义层：no-store/no-cache 处理、max-age/Expires 与 Last-Modified 启发式新鲜度、Age 头计算、max-stale 宽限、ignore-cache-controls 配置——条件请求只是重验证一步，新鲜度计算层缺失 |

---

## 六、结论（Top 3 推荐）

**Top 1：AutoThrottle 延迟观测自适应限速（吸收清单 #1，成本 S）**
Scrapy 与 Scrapling 两家头部项目独立演化出几乎相同的延迟观测收敛算法，是限速的公认正确形态。成本 S：纯算法零依赖，直接扩展 host_discipline.rs 的 observe 入口（该文件已有 429 冷却的 ObserveOutcome 管道，接入点现成）。防正反馈设计（非 200 且变小时跳过）是关键细节，照抄即可。契合度满分：不动架构，只给静态 min_interval 加动态收敛。

**Top 2：Session 身份三件套绑定与记分退休（吸收清单 #2，成本 M）**
反爬按身份组合封禁，crawlrs 的 UA 池+代理池+TLS 档互不感知导致盲轮换——这是当前反爬能力的最大结构性短板。成本 M：Session 结构+DashMap 记分，挂现有池之上，EngineRouter 消费。与既有 403→TLS 升级联动后形成完整身份生命周期。Apache-2.0 参考实现路径清晰。

**Top 3：失败现场快照（错误签名→截图+HTML）（吸收清单 #4，成本 S）**
成本 S 但排障价值普适：所有引擎的失败 case 有了可归因现场，反爬调优从猜变成看。observability 模块已存在只缺此机制，sha1 键+DashMap 去重实现极简，且设计上「内部任何失败静默返回不影响抓取」与现有架构零冲突。

**Top 3 的取舍说明**：Top 3 取自调研的 topPicks 结论（rank 1–3），口径是「单位成本收益」，与第三节的 priority 1–12（按问题严重度排列）口径不同。#3 断点续爬（priority 3）成本为 M、收益集中于数小时级长任务场景；#4 失败现场快照（priority 4）成本仅 S、对所有引擎的失败 case 普适——成本 S<M 且普适性更高，故 Top 3 取 #4 而非 #3；#3 不受影响，仍按 priority 3 实施。

**建议实施顺序与依赖**：#1 与 #5 同落在 host_discipline（先加延迟观测、后接 Crawl-delay 双时钟与停滞检测）；#5 的停滞检测依赖 #2 的 Session 退休机制；#3 的持久化与 #11 的 sitemap 断点续爬共享 frontier 快照机制，宜同批实施；#7/#8/#9 同属提取与 LLM 链，可成组推进；#4、#6、#10、#12 相互独立，可并行。

---

## 附录 A：基线漂移说明

基线描述与 crawlrs 仓库实际情况共核实出 10 处漂移：

1. **仓库有、基线漏：SSRF 防护**——handler 前置 + 引擎路由双重校验（src/infrastructure/security/ssrf/），平台级安全能力基线未列。
2. **仓库有、基线漏：Smart 聚合搜索引擎**——独立 SearchEngineType（并发 Baidu/Bing/Sogou + SimHash + RRF 融合），基线只列了六个具体引擎。
3. **仓库有、基线漏：i18n 国际化**——Fluent（src/i18n/ + locales/）。
4. **仓库有、基线漏：地理位置限制**——src/infrastructure/geolocation.rs、domain/services/geo_location.rs、DB geo restriction 仓储。
5. **仓库有、基线漏：OpenAPI 3.1 规范输出与 SDK 生成**——openapi feature + sdforge，GET /v1/sdk/openapi.json。
6. **仓库有、基线漏：进程内事件总线**——src/infrastructure/events/event_bus.rs、broadcast.rs。
7. **基线写了但仓库没有：Google 搜索引擎**——SearchEngineType 枚举（src/search/types.rs:8-15）只有 Bing/Baidu/Sogou/Smart/Exa/Tavily，且 types.rs 测试断言 from_name("google") 返回 None；search/client/ 下无 google.rs（shared_utils.rs:57 注释提及的 google.rs 已不存在，html_parser.rs 的「Google 风格 parser」实为 Bing/Baidu 复用的通用解析器）；README.md:62 仍宣称 Google 聚合，文档与代码不一致。
8. **基线写了但仓库没有：Redis 运行时依赖**——Cargo.toml 零 redis crate；oxcache/mod.rs:265 注释明确「unified cache — no Redis backend」；docker/docker-compose.yml 无 Redis 服务；README.md:322 路线图将「Redis 共享缓存层（多实例部署）」列为 📋 计划中。
9. **基线口径偏宽：SQLite/MySQL 仅为编译级支持**——README.md:321 路线图明示「db-sqlite / db-mysql 已实现编译级覆盖，运行时 schema 供给待补（当前 migrations/*.sql 为 PG 专用 DDL）」，基线「PostgreSQL（主）/ SQLite / MySQL」未加此限定。
10. **基线口径偏宽：「多层缓存」**——仓库内可见的仅是 oxcache 单实例 Cache<String,String>（oxcache/cache_service.rs:5-7，仅启用 memory L1 特性，Cargo.toml:277）+ DNS 缓存，未见 L2/分布式层实现。

**成稿抽样复核（2026-10-04 在 crawlrs 工作区实际执行，11 项全部与上述记录一致）**：

- `sed -n '1,20p' src/search/types.rs`：SearchEngineType 枚举确为 Bing/Baidu/Sogou/Smart/Exa/Tavily，无 Google（印证第 7 条）；
- `sed -n '145,152p' src/search/types.rs`：存在 `from_name("google")`/`from_name("GOOGLE")` 返回 None 的测试断言（印证第 7 条）；
- `ls src/search/client/`：baidu.rs/bing.rs/exa.rs/html_parser.rs/shared_utils.rs/sogou.rs/tavily.rs，无 google.rs（印证第 7 条）；
- `grep -in redis Cargo.toml`：零命中（印证第 8 条）；
- `grep -rn "no Redis backend" src/`：命中 src/infrastructure/oxcache/mod.rs:265（印证第 8 条）；
- `sed -n '62p' README.md`：确有「Google、Bing、百度、搜狗多引擎聚合」宣称（印证第 7 条的文档-代码不一致）；
- `sed -n '55,85p' src/utils/host_discipline.rs`：`ObserveOutcome::RateLimited` 注释为「按 max(retry_after, cooldown_default) 进入冷却（连击不动）」（印证吸收点 #1/#5 的差距描述）；
- `awk` 提取 Cargo.toml `[features]` 段：28 个 feature，与附录 C 清单逐一吻合；
- `ls src/domain/services/llm/`：mod.rs + prompt_builder.rs + provider_adapter.rs + vision_adapter.rs（印证吸收点 #8 差距描述）；
- `find src -type d -name observability`：src/infrastructure/observability/ 仅 metrics.rs + mod.rs（印证吸收点 #4 差距描述）；
- `grep -rn EventRequestPaused src/engines/`：命中 src/engines/client/playwright.rs:610-616（印证吸收点 #6 差距描述）；
- `grep -rn "map_use" src/`：全仓内容零命中；`src/application/use_cases/mod.rs:8-11` 仅声明 crawl_link_processor/crawl_state_machine/crawl_use_case/create_scrape 四模块，路由层（src/presentation/routes/mod.rs）无 map 端点——map_use_case.rs（文件存在，约 17KB）为未接线孤儿文件（本次修订将吸收点 #11 差距由「部分有」改为「缺失」的依据）；
- `grep -rln "vecboost" src/`：命中 src/config/rag.rs、src/bootstrap/services.rs、src/infrastructure/services/vecboost_provider.rs、src/domain/services/rerank.rs 等 5 处（vecboost 为仓库内本地嵌入/重排服务，定义见附录 E）；
- `sed -n '28,48p' src/workers/crawl/adaptive.rs`：confidence/coverage/saturation 三信号 StrategyResult 结构实测存在（印证附录 D 矩阵「信息充分性最优停止 = 已有」行）。

## 附录 B：统计

- 候选项目：41 个，其中清单外新发现 25 个；
- 深潜项目：15 个，共 150 个候选功能点（全部 factLevel=已发布）；
- 差距矩阵（gapMatrix）：调研统计口径为 41 行，实际附于附录 D 的矩阵为 40 行（以原始 gapMatrix 数组为准）；
- 吸收清单：12 条，32 条来源引用全部经独立复核通过（verdict=verified）；
- 不建议吸收：10 条；更优实现参考：7 条；
- 仓库复核：8 处（grep/read src/utils/dedup、host_discipline、adaptive.rs、intercept.rs、adaptive_concurrency.rs、playwright.rs、observability/、llm/）；「4 项候选因基线已有而剔除或降级、4 项 gap 从缺失修正为部分有」为原始调研记录的汇总结论，记录未逐条具名，可对照附录 D 矩阵交叉核对——「已有」3 行（URL 排列去重锁、信息充分性最优停止、广告/追踪域名拦截）即候选被剔除/降级的直接证据，11 行「部分有」中包含被修正的行；
- 修订：吸收清单经来源复核修订后定为 12 条——移除 UA 钉扎与 WebBotAuth 两条（前者仅剩 Scrapling fingerprints.py 一条可核查来源、后者仅剩实现文件一条且官方文档面零支撑，均不满足 2~4 条来源硬约束）；AutoThrottle 收缩为 Scrapy+Scrapling 双来源；XHR 捕获换入 patchright 的 crNetworkManagerPatch.ts 作为佐证来源（链接见第三节 #6）；混合 AX 修正的是辅助来源 a11yTree.ts 的路径（packages/extension/understudy/a11y/snapshot/a11yTree.ts，已列入 2.8；主来源 capture.ts 经验证存在）；本次修订另依据成稿复核将 #11 的 sitemap 差距由「部分有」改为「缺失」（map_use_case.rs 为未接线孤儿文件，证据见附录 A）；
- 成稿抽样复核：14 项（见附录 A 末）——前 11 项与调研记录全部一致；后 3 项为修订新增复核（map_use_case 孤儿文件、vecboost、adaptive.rs 三信号结构），并据第 12 项修订 #11 差距定级。

## 附录 C：Cargo feature 清单（28 个）

`default`、`platform`、`db-postgres`、`db-sqlite`、`db-mysql`、`http`、`openapi`、`standard`、`full`、`llm`、`rag-local`、`rag-remote`、`rag`、`engine-playwright`、`engine-flaresolverr`、`engine-tls-fingerprint`、`engine-mllm`、`content`、`trafilatura`、`dom-smoothie`、`extractors`、`agent-lib`、`metrics`、`teams`、`auth`、`rate-limit`、`webhook`、`test-mocks`

（经 `awk '/^\[features\]/{f=1;next} /^\[/{f=0} f' Cargo.toml` 实测，与调研数据 cargoFeatures 一致。）

## 附录 D：差距矩阵（40 行）

调研对 150 个候选功能点逐条对照 crawlrs 仓库生成的完整差距矩阵（「状态」∈ 已有 / 部分有 / 缺失；第三节吸收清单 12 条全部出自「缺失 / 部分有」行，并经复核修订）：

| # | 功能 | 来源 | 状态 | 备注 |
|---|---|---|---|---|
| 1 | AutoThrottle 延迟观测自适应限速 | Scrapy / Scrapling | 部分有 | host_discipline.rs 只有静态 min_interval 预约时刻表 + 429 冷却，缺成功响应延迟 EMA 向下收敛（快站提速、慢站降速） |
| 2 | 429 指数连击退避 | Crawlee | 部分有 | 已有 max(retry_after, cooldown_default) 固定冷却且连击不动，缺 consecutive429 指数推进与 burst 抑制（同窗多 429 只推进一次） |
| 3 | robots Crawl-delay 联动 per-host 限速 | Crawlee / trafilatura | 缺失 | robots.rs 是可选遵从开关，Crawl-delay 未作为限速时钟输入，缺 max(backoffUntil, crawlDelayUntil) 双时钟 |
| 4 | 限速停滞检测转代理/会话切换 | Crawlee | 缺失 | 持续 rate-limit 超阈值上报 stalled 转 PersistentRateLimitError 换身份的机制不存在 |
| 5 | Session 身份三件套绑定与记分退休 | Crawlee | 缺失 | ua_pool 与 proxy_pool 各自独立轮换，代理+Cookie+UA/TLS 档未绑成身份单元按 errorScore 记分退休 |
| 6 | 失败现场快照（错误签名→截图+HTML） | Crawlee | 缺失 | observability/ 仅 metrics.rs，无按 sha1(错误栈) 键去重的失败现场存档 |
| 7 | 爬取错误统计直方图与重试归因 | Crawlee | 部分有 | Prometheus 有聚合指标，缺 requestRetryHistogram（下标=重试次数）与最终错误/中途错误分开归因 |
| 8 | per-request 间隔抖动可配置 | Scrapy 2.19.0 | 缺失 | backoff.rs 的 jitter 只在重试路径，per-host 时刻表是确定性的，无按 host 配置的抖动比例 |
| 9 | 调度级请求指纹（method+body+canonical URL） | Scrapy | 部分有 | Deduplicator 已有 URL 归一化+等价变体+Bloom，但不含 method/body 维度，POST 场景无法去重 |
| 10 | 持久化 seen 集合（跨重启去重） | Scrapy | 缺失 | Deduplicator 是进程内存态（Bloom+HashSet），无 SQLite/WAL 落盘，重启后 seen 丢失 |
| 11 | 爬取断点续爬（pending 队列快照恢复） | crawl4ai / Scrapling | 缺失 | workers 下 grep 无 resume/checkpoint 命中，frontier 内存队列崩溃即丢 |
| 12 | 队列内容原子落盘（.tmp+rename） | Scrapling / Scrapy JOBDIR | 缺失 | 无任何队列状态快照写盘机制 |
| 13 | XHR/Fetch 后台响应体捕获 | Scrapling | 缺失 | playwright.rs:611 已有 RequestPaused 事件流但只做 block/continue，无 getResponseBody 捕获 XHR 数据接口 |
| 14 | 站点级 DOM 定向抽取注册表 | defuddle | 缺失 | 无 URL pattern→专用提取器注册表 + canExtract 二次验证 + contentSelector 重入的第三层提取配置 |
| 15 | 自适应元素重定位（页面改版后相似度找回） | Scrapling | 缺失 | CSS 规则选择器失配即失败，无元素属性签名存储与相似度打分重定位 |
| 16 | 提取降级重试阶梯（短结果升级+长度比裁决） | defuddle / trafilatura | 部分有 | Trafilatura 库内部有级联，但 DomSmoothie→Trafilatura→LLM 之间无「输出字数不足才升级、新结果需 ≥1.5x 长度才采纳」的编排 |
| 17 | 移动端媒体查询信号识别桌面噪声 | defuddle | 缺失 | 无 CSS media query 规则提取+内联评估来识别仅桌面布局显示的噪声元素 |
| 18 | 提取删除审计轨迹（per-step 归因） | defuddle | 缺失 | 无 debug 模式记录每个被删元素的 step/selector/reason/预览，坏 case 无法定位到管道步骤 |
| 19 | LLM 提取 token 窗口感知分块 | Scrapegraph-ai | 缺失 | src/domain/services/llm/ 仅 prompt_builder/provider_adapter/vision_adapter，无按模型上下文窗口切块 |
| 20 | chunk 级并行 LLM 提问 + 单次 merge | Scrapegraph-ai | 缺失 | 长文提取单 prompt 直发，无并行分块提问后 LLM 综合 merge |
| 21 | LLM 提取前确定性术语预检 | Scrapegraph-ai | 缺失 | 无「prompt 关键词+schema 字段名均不在文本中→告警」的零成本预检，渲染失败会白烧 token 得 NA |
| 22 | LLM 提取结果字符级溯源对齐 | langextract | 缺失 | 提取结果无 char_interval 证据区间，无 DP 单调匹配+模糊 LCS+覆盖率/密度门，RAG 入库无证据链 |
| 23 | 多轮独立提取 + 首轮优先合并 | langextract | 缺失 | 无 extraction_passes 多轮重切块提升长文召回、区间不重叠合并 |
| 24 | 混合 DOM+AX 树快照喂 MLLM | Stagehand / browser-use | 缺失 | MLLM 引擎走截图视觉通道，无 CDP Accessibility.getFullAXTree 文本大纲（省 token 且产出可复用 selector） |
| 25 | LLM-as-judge 任务完成度独立验证 | browser-use | 缺失 | MLLM 导航自报完成即结束，无独立 judge 模型评审轨迹防 premature done |
| 26 | 可点击元素启发式分类器 | browser-use | 部分有 | MLLM 视觉导航靠模型看图判断可点区域，无 CDP listener/AX 属性/标签/cursor:pointer 优先级短路判定链兜底 |
| 27 | sitemap 流式请求源（背压+增量持久化） | Crawlee | 部分有* | map_use_case 是预载式站点映射清单，非可背压、断点续爬、可与 frontier 组合的一等请求源 |
| 28 | 外源 URL 预热（Common Crawl/CT 日志/Wayback） | crawl4ai | 缺失 | 种子仅来自页面发现与站点映射，无免抓取多源 URL 预热 |
| 29 | 软 404 指纹剔除（status+title+length+body_hash） | crawl4ai | 缺失 | 无 SPA 软 404 空壳判定指纹 |
| 30 | 进程内存压力反馈派发门控 | crawl4ai | 部分有 | adaptive_concurrency.rs 已有 AIMD 按成功/失败信号调并发，但无按内存水位双阈值滞后暂停/恢复整个派发环 |
| 31 | GCRA burst 容忍语义（tau 突发窗口） | Lightpanda | 部分有 | d95013b2 预约时刻表与 GCRA 同构，缺 tau=(burst-1)*interval 空闲突发吸收与 host 表惰性 sweep 倍增 |
| 32 | WebBotAuth RFC 9421 请求签名 | Lightpanda | 缺失 | 无 Ed25519 HTTP Message Signatures 自证 bot 身份通道，反爬只剩伪装路线 |
| 33 | 闭式 Shadow DOM 元素访问 | patchright / Stagehand | 缺失 | Playwright 官方 API 拿不到 closed shadow root，缺 CDP DOM.getDocument(pierce=true)+resolveNode 通路 |
| 34 | 错误驱动引擎特征升级链（featureFlags 重建重试） | Firecrawl | 部分有 | 已有 403 连击→TLS 指纹升级单点（host_discipline.rs:78），缺 401/403/429 系统性加 stealth 代理/换引擎重建候选链 |
| 35 | per-domain TLS/浏览器实证基准选路 | Firecrawl engpicker | 缺失 | SmartHybrid 静态策略+SPA 探测，无抽样实测+相似度判定+verdict 持久化的实证选路 |
| 36 | HTTP 缓存完整语义层（freshness/max-stale/负缓存/variantKey） | Scrapy / Firecrawl | 部分有 | oxcache TTL 层+条件请求增量已有，缺 Cache-Control 新鲜度计算、max-stale 宽限、失败负缓存、变体键防串缓存 |
| 37 | 渲染类型预测+抽样验证自学习闭环 | Crawlee | 部分有 | upgrade_probe 是规则式 SPA 探测，缺相似度特征+逻辑回归预测+10% 抽样实测回标再训练 |
| 38 | URL 排列去重锁（www/https/index.html 变体） | Firecrawl / crawl4ai | 已有 | url_normalizer.rs 去 fragment+剥 tracking 参数+permutations 变体已覆盖，无需吸收 |
| 39 | 信息充分性最优停止（confidence/coverage/saturation） | crawl4ai | 已有 | adaptive.rs:32-45 已实现三信号停止条件，crawl4ai 增量仅剩 consistency(Jaccard) 组件与 embedding 策略，边际价值低 |
| 40 | 广告/追踪域名请求拦截 | Scrapling | 已有 | intercept.rs 已内置 block_ads 域名黑名单+Fetch 拦截机制，仅名单规模差异（20 条 vs 3500 条），不值得为名单规模吸收 |

注 *：第 27 行备注所引 map_use_case 经成稿复核确认为未接线孤儿文件（证据见附录 A），对应吸收点 #11 的差距定级已修订为「缺失」——本行保留原矩阵记录并加星标注。

## 附录 E：术语说明

| 术语 | 含义 |
|---|---|
| factLevel | 调研对每个功能点标注的事实层级；「已发布」= 断言可在候选项目已发布的源码/官方文档中逐行核实。factLevel 非「已发布」的功能点不得进入第三节吸收清单（本次 150 个功能点全部为已发布） |
| verdict=verified | 来源复核结论：复核者独立打开来源文件，逐条核对断言对应的实现与行号；12 条吸收点共 32 条来源引用全部 verified |
| 成本 S/M/L | 本报告约定的实现成本相对分级（非上游项目口径）：S=单模块内改动、零或一个轻量新依赖；M=需新增内聚模块并与现有架构接线；L=跨模块结构性改动（本清单无 L） |
| MLLM | 多模态大语言模型（视觉语言模型）；crawlrs 的 `engine-mllm` 引擎用其做截图视觉导航 |
| AX 树 | Accessibility Tree（浏览器无障碍树）；CDP `Accessibility.getFullAXTree` 可获取其文本大纲 |
| AIMD | Additive Increase / Multiplicative Decrease，加性增、乘性减的并发/速率调节策略（crawlrs adaptive_concurrency.rs 采用） |
| GCRA | Generic Cell Rate Algorithm，泛型信元速率算法；以「理论到达时间预约」实现限速，天然支持突发容忍（见第五节 Lightpanda 行） |
| CT 日志 | Certificate Transparency（证书透明度）公开日志；可按域名枚举子域（crawl4ai 经 crt.sh 使用） |
| JOBDIR | Scrapy 的任务目录配置项：设置后调度队列与去重指纹持久化到该目录，支持爬取中断后恢复 |
| YOSO | You Only Scrape Once，Scrapegraph-ai 的图式抓取范式：一次定义有向图即可完成 fetch→parse→提取 |
| vecboost | crawlrs 仓库内的本地向量服务（嵌入/重排推理）：src/infrastructure/services/vecboost_provider.rs 为服务接线，src/config/rag.rs 为配置，src/domain/services/rerank.rs 为重排领域服务 |
