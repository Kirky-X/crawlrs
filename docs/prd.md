# crawlrs - 产品需求文档 (PRD)

## 版本信息

- **文档版本**: v2.1.0
- **项目类型**: 全新 Rust 重构项目（无历史包袱）
- **最近更新**: 2024-12-20
- **目标版本**: Rust Edition 2021

---

## 1. 项目概述

### 1.1 背景

crawlrs 是一个面向开发者的企业级网页数据采集平台，提供搜索、抓取、爬取、映射与结构化提取能力。该项目采用 Rust 开发，旨在构建**高性能、高可靠、易扩展**的生产级系统。

### 1.2 核心目标

1. **性能提升**: 相比 Node.js 版本，吞吐量提升 **3-5 倍**，P99 延迟降低 **50%**
2. **架构简化**: 统一队列系统（Postgres NuQ），消除 Redis 队列依赖
3. **类型安全**: 利用 Rust 类型系统在编译期捕获 90% 以上的错误
4. **弹性部署**: 支持单机和集群两种部署模式，按需水平扩展
5. **可观测性**: 内置分布式追踪和指标采集，便于生产环境监控

### 1.3 非目标（本期不做）

- ❌ 不兼容 Node.js 版本的 v0 API（仅实现 v1 API）
- ❌ 不实现 UI 前端（仅提供 REST API）
- ❌ 不实现多语言 SDK（由社区或后续版本提供）
- ❌ Fire Engine (TLS/CDP) 引擎本期不纳入（使用 Fetch/Playwright 作为主力）
- ❌ WebSocket 实时订阅本期不纳入

---

## 2. 系统架构概览

### 2.1 架构原则

1. **单一职责**: 每个模块只负责一个核心功能
2. **开闭原则**: 对扩展开放（新增引擎），对修改关闭
3. **依赖倒置**: 依赖抽象（Trait）而非具体实现
4. **DDD 分层**: API → Service → Domain → Infrastructure

### 2.2 核心组件

```
┌─────────────────────────────────────────────────────┐
│              API Gateway (Axum)                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │ Rate     │ │  Team    │ │  Auth            │   │
│  │ Limiter  │ │ Semaphore│ │ Middleware       │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────┐
│            Business Services                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │ Scrape   │ │  Crawl   │ │  Extract         │   │
│  │ Service  │ │ Service  │ │  Service         │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────┐
│         Task Queue (Postgres + SeaORM)              │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │ Priority │ │ Backlog  │ │  Scheduler       │   │
│  │  Queue   │ │  Table   │ │                  │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────┐
│            Worker Pool (Tokio)                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │ Scrape   │ │ Extract  │ │  Webhook         │   │
│  │ Worker   │ │ Worker   │ │  Delivery        │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────┐
│       Scraper Engine Router (Strategy)              │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │  Fetch   │ │Playwright│ │  Fire Engine     │   │
│  │  Engine  │ │  Engine  │ │  (TLSClient/CDP) │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
│         Circuit Breaker + Health Monitor            │
└─────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────┐
│              Storage Layer                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │
│  │Postgres  │ │  Redis   │ │  GCS/S3          │   │
│  │(SeaORM)  │ │ (Cache)  │ │  (Results)       │   │
│  └──────────┘ └──────────┘ └──────────────────┘   │
└─────────────────────────────────────────────────────┘
```

---

## 3. 核心功能模块

### 3.1 搜索 (Search) ✅ 已实现

**功能描述**: 并发聚合多个搜索引擎（Google/Bing/Baidu/Sogou）获取结果，智能去重排序，可选批量抓取回填内容。

**实现状态**: ✅ 已实现
- [x] 并发聚合：通过 `SearchAggregator` 并发调用多个引擎并聚合结果。
🔴 高优先级 (需要立即处理)
团队白名单功能 - 安全相关，影响生产环境
对象存储完整集成 - 结果存储功能核心
🟡 中优先级 (近期处理)
Fire Engine 引擎测试 - 增强抓取能力
高级同步等待机制 - 提升用户体验🔴 高优先级 (需要立即处理)
团队白名单功能 - 安全相关，影响生产环境
对象存储完整集成 - 结果存储功能核心
🟡 中优先级 (近期处理)
Fire Engine 引擎测试 - 增强抓取能力
高级同步等待机制 - 提升用户体验- [x] 智能去重：在 `search/aggregator/deduplicator.rs` 中实现了基于 URL 和标题 Jaro-Winkler 相似度的去重（阈值 0.9）。
- [x] 计费系统：每个搜索请求消耗 1 Credit，已在 `SearchService` 中实现。
- [x] 缓存机制：已实现基于内存的缓存（`SearchAggregator` 中的 `DashMap`），TTL 为 5 分钟。
- [x] 回填抓取：支持 `crawl_results` 参数，自动为搜索结果创建爬取任务。
- [x] 同步等待与异步模式：基础实现完成，但缺少 PRD 描述的 `sync_wait_ms` 轮询与自动转异步的高级逻辑。

**输入参数**:

- `query`: 搜索关键词（必填）
- `engines`: 搜索引擎列表（可选，默认使用配置文件设置）
  - 可选值：`["google", "bing", "baidu", "sogou"]`
  - 未指定时使用 `config.toml` 中的 `enabled_engines`
- `limit`: 结果数量（1-100，默认 10）
- `lang`: 搜索语言（默认 en）
- `country`: 国家代码（默认 US）
- `sync_wait_ms`: 同步等待时长（毫秒，默认 5000，最大 30000）
- `scrape_options`: 抓取配置（可选）
- `async_scraping`: 是否异步抓取（默认 false）

**输出响应（同步模式 - 5秒内完成）**:

```json
{
  "success": true,
  "status": "completed",
  "data": {
    "results": [
      {
        "title": "Example Title",
        "url": "https://example.com",
        "content": "Snippet text...",
        "source_engine": "google",
        "relevance_score": 0.95,
        "published_date": "2024-01-15T10:30:00Z"
      }
    ],
    "total": 15,
    "engines_used": ["google", "bing"],
    "cache_hit": false
  },
  "credits_used": 1,
  "response_time_ms": 234
}
```

**输出响应（异步模式 - 超时或主动指定）**:

```json
{
  "success": true,
  "status": "processing",
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "expires_at": "2024-12-11T00:00:00Z",
  "credits_used": 0
}
```

**业务规则**:

1. 每个搜索请求消耗 **1 Credit**（无论同步/异步）
2. 每个回填抓取额外消耗 **1-5 Credits**（视内容复杂度）
3. **并发聚合策略**：
   - 同时查询所有启用的引擎（配置化）
   - 单引擎超时时间 10 秒（可配置）
   - 至少 1 个引擎成功即返回结果
   - 失败引擎自动触发断路器
4. **去重算法**：
   - 基于 URL 完全去重
   - 基于标题 Jaro-Winkler 相似度去重（阈值 0.85）
5. **缓存机制**：
   - 缓存键：`hash(query + engines + lang + limit)`
   - TTL: 1 小时
   - 命中缓存不消耗 Credits
6. **智能等待**：
   - 默认等待 5 秒，期间持续轮询结果
   - 超时后返回异步响应
   - 后台任务继续执行并通过 Webhook 回调

**实现状态**: ✅ 已实现
- [x] 基于 URL 完全去重
- [x] 基于标题 Jaro-Winkler 相似度去重（在 `search/aggregator/deduplicator.rs` 中实现，阈值 0.85）

---

### 3.2 抓取 (Scrape) ✅ 已实现

**功能描述**: 对单个 URL 执行内容获取，支持多格式输出和页面交互。

**实现状态**: ✅ 已实现
- [x] 基础抓取：通过 `ReqwestEngine` 实现了高性能的静态内容抓取。
- [x] JS 渲染与截图：通过 `PlaywrightEngine` (基于 `chromiumoxide`) 实现了无头浏览器抓取、JS 渲染及截图功能。
- [x] 智能路由：通过 `EngineRouter` 根据请求需求（如是否需要 JS、截图）自动选择最优引擎。
- [x] 代理与安全：支持代理配置、跳过 TLS 校验及 SSRF 防护（在引擎层实现）。
- [x] 自定义配置：支持自定义 HTTP 头、移动端模拟、超时设置等。
- [x] 页面交互动作：已在 `PlaywrightEngine` 中实现了 `click`、`scroll`、`input` 等动作，并完全集成到 `ScrapeWorker` 的标准工作流中。
- [x] 同步等待与异步模式：已在 `ScrapeHandler` 中支持 `sync_wait_ms`，实现了完整的轮询逻辑。

**输入参数**:

- `url`: 目标 URL（必填）
- `formats`: 输出格式数组（markdown/html/rawHtml/json/screenshot/links）
- `actions`: 页面交互动作（wait/click/scroll/screenshot）
- `sync_wait_ms`: 同步等待时长（毫秒，默认 5000，最大 30000）
  - 指定后，API 会在该时间内轮询任务结果
  - 若任务在等待期内完成，直接返回结果（status: completed）
  - 若超时，返回任务 ID（status: processing）
- `options`: 
  - `headers`: 自定义 HTTP 头
  - `timeout`: 超时时间（秒，默认 30）
  - `mobile`: 是否模拟移动端（默认 false）
  - `proxy`: 代理配置（可选）
  - `skip_tls_verification`: 跳过 TLS 校验（默认 false）

**输出响应**:

```json
{
  "success": true,
  "data": {
    "markdown": "# Page Title\n...",
    "html": "<html>...",
    "metadata": {
      "title": "Page Title",
      "description": "...",
      "status_code": 200,
      "content_type": "text/html",
      "response_time_ms": 234
    }
  },
  "credits_used": 3
}
```

**输出响应（异步模式 - 超时）**:

```json
{
  "success": true,
  "status": "processing",
  "task_id": "550e8400-e29b-41d4-a716-446655440000",
  "expires_at": "2024-12-11T00:00:00Z",
  "credits_used": 0  // 任务完成后才扣费
}
```

**业务规则**:

1. 基础抓取消耗 **1 Credit**
2. 截图/PDF 生成额外消耗 **2 Credits**
3. 使用代理额外消耗 **1 Credit**
4. 失败自动重试最多 **3 次**（指数退避）

**实现状态**: ✅ 已实现

- [x] 实现了基本抓取功能
- [x] 支持自定义HTTP头
- [x] 支持超时设置
- [x] 支持移动端模拟
- [x] 支持多种输出格式（包括截图）
- [x] 支持失败重试机制
- [x] 支持截图功能
- [x] 实现了代理配置（在ReqwestEngine中实现）
- [x] 实现了跳过TLS校验（在ReqwestEngine中实现）
- [x] 实现了页面交互动作（在PlaywrightEngine中集成到ScrapeWorker）
- [x] 实现了同步等待与异步模式（完整的轮询逻辑）

---

### 3.3 爬取 (Crawl) ✅ 已实现

**功能描述**: 全站递归爬取，支持深度控制、路径过滤和并发限制。

**实现状态**: ✅ 已实现
- [x] 递归爬取：通过 `CrawlService` 实现了基于 HTML 链接提取的递归爬取逻辑。
- [x] 深度与过滤：支持 `max_depth` 深度控制，以及基于 glob/regex 的 `include_patterns` 和 `exclude_patterns` 路径过滤。
- [x] Robots 协议：通过 `RobotsChecker` 实现了 robots.txt 的解析与遵守，支持 `Crawl-delay`。
- [x] 任务去重：在爬取过程中通过 `TaskRepository` 检查 URL 是否已存在，防止重复爬取。
- [x] 策略支持：支持 BFS/DFS 爬取策略（通过调整任务优先级实现）。
- [x] 并发控制：通过 `RateLimitingService` 限制团队并发任务数。
- [x] 积压处理：通过 `BacklogWorker` 实现了当并发受限时任务进入积压队列并定期重试的机制。
- [x] 过期机制：任务和积压项均支持过期时间设置。

**业务规则**:

- `url`: 起始 URL（必填）
- `crawler_options`:
  - `max_depth`: 最大爬取深度（默认 2，最大 10）
  - `limit`: 最大页面数（默认 100，最大 10000）
  - `include_paths`: 包含路径正则（数组）
  - `exclude_paths`: 排除路径正则（数组）
  - `ignore_robots`: 是否忽略 robots.txt（默认 false）
  - `crawl_delay_ms`: 请求间隔（毫秒，默认 500）
- `scrape_options`: 同抓取接口
- `max_concurrency`: 最大并发数（默认 5，最大 20）

**输出响应**:

```json
{
  "success": true,
  "id": "crawl-uuid",
  "status": "processing",
  "total": 0,
  "completed": 0,
  "expires_at": "2024-12-11T00:00:00Z"
}
```

**业务规则**:

1. 按团队套餐限制并发数（免费版 5，专业版 20，企业版不限）
2. 遵循 robots.txt 和 crawl-delay 规则（除非显式忽略）
3. 同一域名的任务共享去重集合（Redis）
4. 超过 24 小时未完成的任务自动过期

**实现状态**: ✅ 已实现
- [x] 实现了基本的爬取功能
- [x] 实现了深度控制
- [x] 实现了路径过滤（包含/排除模式）
- [x] 实现了robots.txt遵守
- [x] 支持同步等待模式
- [x] 实现了并发限制（通过团队信号量 TeamSemaphore 统一控制）
- [x] 实现了任务过期机制（Task 结构体包含 expires_at 字段，BacklogWorker 负责处理过期任务）---

### 3.4 提取 (Extract) ✅ 已实现

**功能描述**: 基于 LLM 对页面集合进行结构化数据提取。

**实现状态**: ✅ 已实现
- [x] **多模态提取**: 在 `ExtractionService` 中实现了基于 CSS 选择器和 LLM 的混合提取模式。
- [x] **LLM 集成**: `LLMService` 支持 OpenAI 兼容接口，支持自定义 prompt 和 JSON Schema。
- [x] **Token 追踪**: 实现了 `TokenUsage` 追踪和 Redis 实时记录。
- [x] **智能等待**: `ExtractHandler` 支持 `sync_wait_ms` 同步等待。
- [x] **并发控制**: 通过 `TeamSemaphore` 实现团队并发限制。
- [x] **自动扣费**: 已实现 Token-to-Credits 转换逻辑 (10 Credits / 1000 tokens)，并在复杂工作流中全面覆盖自动触发扣费。

**输入参数**:

- `urls`: 目标 URL 数组（必填，最多 100 个）
- `prompt`: 提取指令（与 schema 二选一）
- `schema`: JSON Schema 定义（与 prompt 二选一）
- `options`:
  - `agent`: LLM 模型（gpt-4/claude-3/gemini-pro）
  - `enable_web_search`: 是否允许联网搜索
  - `max_concurrency`: 最大并发数（默认 5）

**输出响应**:

```json
{
  "success": true,
  "id": "extract-uuid",
  "data": [
    {
      "url": "https://example.com",
      "extracted": {
        "title": "...",
        "price": 99.99,
        "availability": "in_stock"
      }
    }
  ],
  "tokens_used": 1234,
  "credits_used": 50
}```

**业务规则**:
1. 每 1000 tokens 消耗 **10 Credits**
2. 并发受团队配额限制
3. 提取失败的 URL 不扣除 Credits

**实现状态**: ✅ 已实现（支持基础提取、LLM集成、并发控制、Tokens计费和自动扣费）
- [x] 实现了基本的提取功能（基于CSS选择器的结构化数据提取）
- [x] 支持同步等待模式
- [x] 集成了LLM模型（支持通过ExtractionService调用LLMService）
- [x] 实现了Prompt/schema提取（ExtractionRule支持llm_prompt）
- [x] 实现了max_concurrency参数（在ExtractionService集成中通过并发限制实现）
- [x] 实现了Tokens计费（TokenUsage已在LLMService中定义并返回）
- [x] 实现了自动扣费（Token-to-Credits转换逻辑在复杂工作流中全面覆盖）
---

### 3.5 状态查询与取消 ✅ 已实现

**功能描述**: 查询异步任务状态、获取分页结果、取消进行中的任务。

**实现状态**: ✅ 已实现
- [x] **统一任务管理**: 实现了 `TaskHandler` 处理 `/v2/tasks/query` 和 `/v2/tasks/cancel`。
- [x] **智能轮询**: `wait_for_tasks_completion` 实现了基于任务完成率的动态间隔轮询（500ms - 2000ms）。
- [x] **批量操作**: 支持一次性查询或取消多个任务。
- [x] **同步/异步切换**: 通过 `sync_wait_ms` 参数支持从异步模式无缝切换到同步等待模式。

**端点**:
- ❌ `GET /v1/scrape/:id` - 获取 Scrape 状态 (已废弃)
- ❌ `GET /v1/crawl/:id` - 获取 Crawl 状态 (已废弃)
- `GET /v1/crawl/:id/results?page=1&limit=50` - 分页结果
- ❌ `DELETE /v1/crawl/:id` - 取消任务 (已废弃)
- `POST /v2/tasks/query` - 统一查询 (✅ 已实现)
- `POST /v2/tasks/cancel` - 统一取消 (✅ 已实现)

**状态枚举**:
- `queued`: 排队中
- `processing`: 进行中
- `completed`: 已完成
- `failed`: 失败
- `cancelled`: 已取消

---

### 3.6 统一任务管理 ✅ 已实现

#### 3.6.1 任务查询（替代旧接口） ✅ 已实现

**端点**: `POST /v2/tasks/query`

**功能描述**: 统一的任务状态查询接口，支持批量查询和高级过滤，替代以下旧接口：
- ❌ `GET /v1/scrape/:id` (已废弃)
- ❌ `GET /v1/crawl/:id` (已废弃)

**输入参数**:
```json
{
  "task_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660e8400-e29b-41d4-a716-446655440001"
  ],
  "include_results": true,            // 是否返回完整结果（默认 true）
  "filters": {
    "status": ["completed", "failed"], // 可选状态过滤
    "task_type": ["scrape", "search"]  // 可选任务类型过滤
  }
}
```

**输出响应**:

```json
{
  "success": true,
  "tasks": [
    {
      "task_id": "550e8400-...",
      "task_type": "scrape",
      "status": "completed",
      "created_at": "2024-12-10T10:00:00Z",
      "completed_at": "2024-12-10T10:00:05Z",
      "result": {
        "markdown": "# Page content...",
        "metadata": {...}
      },
      "credits_used": 3
    },
    {
      "task_id": "660e8400-...",
      "task_type": "search",
      "status": "processing",
      "created_at": "2024-12-10T10:01:00Z",
      "progress": 0.6
    }
  ]
}
```

**业务规则**:

1. 最多支持一次查询 **100 个任务**
2. `include_results: false` 时仅返回元数据，节省带宽
3. 过滤条件为 AND 关系（同时满足）
4. 不存在的 task_id 会被忽略（不报错）

---

#### 3.6.2 任务取消（替代旧接口） ✅ 已实现

**端点**: `POST /v2/tasks/cancel`

**功能描述**: 统一的任务取消接口，支持批量取消，替代以下旧接口：

- ❌ `DELETE /v1/crawl/:id` (已废弃)

**输入参数**:

```json
{
  "task_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660e8400-e29b-41d4-a716-446655440001"
  ],
  "force": false  // 是否强制取消（即使任务已完成，默认 false）
}
```

**输出响应**:

```json
{
  "success": true,
  "results": [
    {
      "task_id": "550e8400-...",
      "cancelled": true,
      "previous_status": "processing"
    },
    {
      "task_id": "660e8400-...",
      "cancelled": false,
      "reason": "Task already completed",
      "previous_status": "completed"
    }
  ]
}
```

**业务规则**:

1. 仅 `queued` 和 `processing` 状态的任务可被取消
2. 已完成/失败的任务取消操作返回 `cancelled: false`
3. `force: true` 时可强制取消任何状态（用于清理）
4. 取消的任务不扣除 Credits
5. Crawl 任务取消会同时取消所有子任务

---

### 3.7 地理访问限制 (Geographic Restrictions) ✅ 已实现

**功能描述**: 基于 IP 地址的地理访问控制，支持国家代码、IP 地址和 CIDR 范围的白名单管理。

**实现状态**: ✅ 已实现
- [x] **IP 地理定位**: 集成 IP 地理定位服务，准确识别请求来源地理位置。
- [x] **多格式支持**: 支持国家代码 (ISO 3166-1 alpha-2)、IP 地址 (IPv4/IPv6) 和 CIDR 范围配置。
- [x] **实时验证**: 在 crawl 和 extract 请求处理时实时验证地理访问权限。
- [x] **团队级配置**: 支持按团队配置地理限制规则，灵活管理访问权限。
- [x] **缓存优化**: 实现地理定位结果缓存，减少重复查询开销。

**API 端点**:
- `GET /v1/teams/geo-restrictions` - 获取团队地理限制配置
- `PUT /v1/teams/geo-restrictions` - 更新团队地理限制配置

**输入参数 (更新配置)**:
```json
{
  "allowed_countries": ["US", "CA", "GB"],
  "allowed_ips": ["192.168.1.0/24", "10.0.0.1"],
  "enabled": true
}
```

**输出响应**:
```json
{
  "success": true,
  "data": {
    "allowed_countries": ["US", "CA", "GB"],
    "allowed_ips": ["192.168.1.0/24", "10.0.0.1"],
    "enabled": true,
    "updated_at": "2024-12-23T10:00:00Z"
  }
}
```

**业务规则**:

1. **地理验证流程**:
   - 首先检查 IP 地址是否在白名单中
   - 然后检查国家代码是否在允许列表中
   - 任一匹配即视为有权限访问
2. **IP 格式支持**:
   - 单个 IP 地址: `192.168.1.1`
   - CIDR 范围: `192.168.1.0/24`
   - IPv6 支持: `2001:db8::1` 和 `2001:db8::/64`
3. **错误处理**:
   - 地理限制验证失败返回 `403 Forbidden`
   - 包含详细的拒绝原因和请求 IP 信息
4. **性能优化**:
   - 地理定位结果缓存 1 小时
   - 团队配置变更实时生效

---

### 3.8 团队白名单管理 (Team Whitelist) ✅ 已实现

**功能描述**: 企业级访问控制，支持基于 IP 地址和地理区域的精细化权限管理。

**实现状态**: ✅ 已实现
- [x] **IP 白名单**: 支持 IPv4/IPv6 地址和 CIDR 范围的白名单配置。
- [x] **地理白名单**: 支持基于国家代码的地理访问控制。
- [x] **实时验证**: 所有 API 请求都经过白名单验证，确保访问安全。
- [x] **动态更新**: 支持运行时更新白名单配置，无需重启服务。
- [x] **审计日志**: 记录所有访问控制决策，便于安全审计。

**输入验证**:
- IP 地址和 CIDR 格式自动验证
- 国家代码格式验证 (ISO 3166-1 alpha-2)
- 重复项自动去重
- 无效格式返回详细错误信息

**业务规则**:

1. **访问控制优先级**:
   - IP 白名单优先于地理限制
   - 禁用状态时所有请求都允许
   - 空白名单视为允许所有访问
2. **配置验证**:
   - IP 地址必须格式正确
   - CIDR 前缀必须有效 (IPv4: ≤32, IPv6: ≤128)
   - 国家代码必须为 2 字母大写格式
3. **安全策略**:
   - 配置变更记录审计日志
   - 拒绝请求包含详细原因
   - 支持紧急禁用所有限制
4. **性能考虑**:
   - 白名单查询 O(1) 时间复杂度
   - 配置缓存减少数据库查询
   - 批量验证支持并发处理

---

## 4. 并发与限流策略

### 4.1 两层限流模型 ✅ 已实现

#### Layer 1: API 速率限制 (Rate Limiter) ✅ 已实现

- **目的**: 防止 API 滥用和 DDoS 攻击
- **算法**: 基于 Redis 的计数器算法（支持按分钟限流）
- **粒度**: 按 API Key 限制
- **实现**: 在 `RateLimiter` 中实现，通过 `check(api_key)` 进行校验。

#### Layer 2: 团队并发控制 (Team Semaphore) ✅ 已实现

- **目的**: 保证公平分配系统资源，防止单用户耗尽 Worker 线程
- **实现**: `TeamSemaphore` 提供基于团队 ID 的信号量控制，支持动态调整许可数。
- **策略**: 免费版 5 并发，专业版 20 并发，企业版不限（通过 `ConcurrencyConfig` 配置）。
  - 免费版: 100 RPM (Requests Per Minute)
  - 专业版: 1000 RPM
  - 企业版: 10000 RPM

#### Layer 2: 团队并发限制（Team Semaphore）

- **目的**: 控制同时执行的任务数（并发槽位）
- **算法**: 分布式信号量（Redis Counter）
- **粒度**: 按 Team ID 限制
- **配额**:
  - 免费版: 5 并发
  - 专业版: 20 并发
  - 企业版: 100 并发

**实现状态**: ✅ 已实现
- [x] 实现了基于Redis的API速率限制（令牌桶算法，支持不同RPM配置）
- [x] 实现了团队并发限制（基于信号量，支持不同并发数配置）
- [x] 支持不同套餐的配额设置
- [x] 实现了队列积压机制（TasksBacklog实体、仓储及BacklogWorker已实现）

### 4.2 队列与积压机制 (Queue Backlog) ✅ 已实现

**实现状态**: ✅ 已实现
- [x] **任务积压**: 当团队并发达到上限时，任务进入 `TasksBacklog` 存储（`ConcurrencyResult::Queued`）。
- [x] **后台调度**: `BacklogWorker` 定期（默认 10s）检查积压任务，尝试重新激活（`reactivate_task`）。
- [x] **自动过期**: 积压任务支持过期检查（`is_expired`），超时后自动标记为失败，不予执行。
- [x] **重试机制**: 积压处理失败时支持指数退避或计数重试（`can_retry`）。

---

## 5. 引擎选择与回退

### 5.1 引擎能力矩阵 ✅ 已实现

**实现状态**: ✅ 已实现
- [x] **Fetch Engine (Reqwest)**: 极速静态网页抓取，支持代理、TLS 跳过。
- [x] **JS Engine (Playwright)**: 支持复杂 JS 渲染、截图、PDF 生成（基于 `chromiumoxide`）。
- [x] **Fire Engine (TLS/CDP)**: 预留 `fire_engine_tls` 和 `fire_engine_cdp` 模块。
- [x] **能力评估**: 每个引擎通过 `support_score(request)` 动态上报其对特定请求的处理能力。

### 5.2 智能路由策略 ✅ 已实现

**实现状态**: ✅ 已实现
- [x] **多维评分**: `EngineRouter` 基于 `support_score`、成功率、响应时间计算综合评分。
- [x] **负载均衡**: 支持 `RoundRobin`、`WeightedRoundRobin`、`FastestResponse`、`Random` 及 `SmartHybrid` (默认) 策略。
- [x] **自动熔断**: `CircuitBreaker` 实时监控失败率，达到阈值（默认 5 次失败）后自动熔断。
- [x] **健康监控**: `EngineHealthMonitor` 定期（默认 60s）执行拨测，维护引擎健康状态（Healthy/Degraded/Unhealthy）。
- [x] **自动降级**: 路由过程中自动跳过熔断或不健康的引擎，确保高可用。

### 5.3 断路器保护 ✅ 已实现

当某个引擎连续失败超过 5 次时：

1. 自动开启断路器（Circuit Breaker）
2. 后续请求直接跳过该引擎
3. 30 秒后进入半开状态（Half-Open）尝试恢复
4. 成功后关闭断路器，恢复正常

**实现状态**: ✅ 已实现

- [x] 实现了断路器机制（`CircuitBreaker`）
- [x] 支持失败计数和状态管理（阈值为 5 次）
- [x] 支持半开状态恢复
- [x] 实现了半开状态的30秒恢复延迟

---

## 6. Webhook 可靠投递

### 6.1 Outbox 模式 ✅

所有需要投递的事件先持久化到 `webhook_events` 表：

```sql
CREATE TABLE webhook_events (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    payload JSONB NOT NULL,
    webhook_url VARCHAR(512) NOT NULL,
    status VARCHAR(20) NOT NULL CHECK (status IN ('pending', 'delivered', 'failed', 'dead')),  -- pending/delivered/failed/dead
    retry_count INT DEFAULT 0,
    max_retries INT DEFAULT 5,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

**实现状态**: ✅ 已实现

- [x] 实现了webhook_events表结构
- [x] 实现了Webhook事件的持久化
- [x] 实现了Webhook状态管理

### 6.2 重试策略 ✅ 已实现

- **算法**: 指数退避（Exponential Backoff）
- **间隔**: 2s → 4s → 8s → 16s → 32s（带随机抖动）
- **最大重试**: 5 次（符合PRD要求）
- **死信处理**: 超过最大重试次数后标记为 `dead`，等待人工介入

**实现状态**: ✅ 已实现

- [x] 实现了指数退避重试算法（2的n次方秒，带随机抖动）
- [x] 最大重试次数为5次，符合PRD要求
- [x] 实现了死信处理机制（超过最大重试次数后标记为dead状态）

### 6.3 签名校验 ✅

使用 HMAC-SHA256 签名保证完整性：

```http
POST /webhook HTTP/1.1
Host: customer-server.com
X-crawlrs-Signature: sha256=abc123...
X-crawlrs-Event: crawl.completed
Content-Type: application/json

{"crawl_id": "...", "status": "completed"}
```

**实现状态**: ✅ 已实现

- [x] 实现了HMAC-SHA256签名机制
- [x] 在请求头中添加了X-crawlrs-Signature
- [x] 在请求头中添加了X-crawlrs-Event头部

---

## 7. 安全与合规

### 7.1 SSRF 防护 ✅

在建立连接前检查目标 IP：

- ❌ 拒绝 `127.0.0.0/8` (Loopback)
- ❌ 拒绝 `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` (Private)
- ❌ 拒绝 `169.254.0.0/16` (Link-Local)
- ❌ 拒绝 `::1` (IPv6 Loopback)
- ❌ 拒绝 `fc00::/7` (IPv6 Unique Local)
- ❌ 拒绝 `fe80::/10` (IPv6 Link-Local)

**实现状态**: ✅ 已实现

- [x] 实现了URL验证和SSRF防护
- [x] 拒绝了私有网络IP地址的访问
- [x] 拒绝了回环地址的访问

### 7.2 Robots.txt 遵守 ⚠️

1. 首次访问域名时解析 `/robots.txt`
2. 缓存规则到 Redis（24 小时 TTL）
3. 每次请求前检查 User-Agent 和路径
4. 遵守 `Crawl-delay` 指令

**实现状态**: ✅ 已实现

- [x] 实现了Robots.txt解析工具
- [x] 实现了Robots.txt缓存机制（1小时TTL）
- [x] 在请求前检查Robots.txt规则
- [x] 实现了Crawl-delay指令遵守

### 7.3 访问控制 ✅

- **地域限制**: 支持按国家/地区屏蔽（基于 GeoIP）
- **域名黑名单**: 内置高危域名列表（恶意软件、钓鱼站点）
- **团队白名单**: 企业版支持静态 IP 白名单

**实现状态**: ✅ 已实现

- [x] 实现了地域限制功能
- [x] 实现了域名黑名单功能
- [x] 实现了团队白名单功能

---

## 8. 性能指标（SLO） ⚠️

### 8.1 目标指标

| 指标                 | 目标值         | 测量方式         |
| -------------------- | -------------- | ---------------- |
| **API 吞吐量**       | 10000 RPS      | 压力测试         |
| **P50 延迟**         | < 50ms         | Prometheus       |
| **P99 延迟**         | < 200ms        | Prometheus       |
| **任务处理速度**     | 1000 tasks/min | Worker 指标      |
| **成功率**           | > 99.9%        | 错误率统计       |
| **可用性**           | 99.95%         | Uptime 监控      |
| **搜索并发查询耗时** | < 10s          | 并发引擎测试     |
| **缓存命中率**       | > 60%          | Redis 监控       |
| **同步返回成功率**   | > 70%          | 任务完成时间分布 |

### 8.2 降级策略

当系统负载超过 80% 时：

1. 自动增加队列积压超时时间
2. 暂停低优先级任务（如预爬取）
3. 限制新爬取任务的最大深度
4. 触发告警通知运维团队

**实现状态**: ✅ 已实现

- [x] 实现了Prometheus指标采集
- [x] 实现了结构化日志记录
- [x] 实现了/metrics端点
- [x] 实现了基于系统负载的降级策略（深度限制、优先级调整）
- [x] 实现了基础告警触发逻辑（高负载日志告警）

## 9. 部署架构

### 9.1 单机部署（开发/测试）

```
┌─────────────────────────────┐
│   Docker Compose            │
│  ┌────────┐  ┌────────────┐ │
│  │  API   │  │  Worker    │ │
│  │ Server │  │   Pool     │ │
│  └────────┘  └────────────┘ │
│  ┌────────┐  ┌────────────┐ │
│  │Postgres│  │   Redis    │ │
│  └────────┘  └────────────┘ │
└─────────────────────────────┘
```

### 9.2 集群部署（生产）

```
       ┌──────────┐
       │  Load    │
       │ Balancer │
       └──────────┘
            │
    ┌───────┴────────┐
    ↓                ↓
┌─────────┐    ┌─────────┐
│ API Pod │    │ API Pod │
│  (x3)   │    │  (x3)   │
└─────────┘    └─────────┘
    ↓                ↓
┌─────────────────────────┐
│   Postgres Cluster      │
│   (Primary + Replicas)  │
└─────────────────────────┘
    ↓                ↓
┌─────────┐    ┌─────────┐
│ Worker  │    │ Worker  │
│  Pod    │    │  Pod    │
│  (x5)   │    │  (x5)   │
└─────────┘    └─────────┘
```

**水平扩展规则**:

- API Pod: 根据 CPU 使用率（> 70% 时扩容）
- Worker Pod: 根据队列深度（> 1000 任务时扩容）
- Postgres: 读写分离，读副本按需扩展

---

## 10. 监控与告警

### 10.1 关键指标

- **业务指标**: 任务成功率、平均处理时间、Credits 消耗速率
- **系统指标**: CPU/内存使用率、数据库连接数、队列积压数量
- **网络指标**: 请求延迟、带宽使用、错误率

### 10.2 告警规则

| 告警级别        | 触发条件         | 响应时间 |
| --------------- | ---------------- | -------- |
| **P0-Critical** | 服务完全不可用   | 5 分钟   |
| **P1-High**     | 成功率 < 95%     | 15 分钟  |
| **P2-Medium**   | 队列积压 > 10000 | 30 分钟  |
| **P3-Low**      | 单个引擎故障     | 1 小时   |

---

## 11. 术语表

| 术语                | 定义                                              |
| ------------------- | ------------------------------------------------- |
| **NuQ**             | Node-Unique Queue，基于 Postgres 的自定义队列系统 |
| **Backlog**         | 并发受限时的任务积压表                            |
| **Semaphore**       | 信号量，用于控制并发数量                          |
| **Circuit Breaker** | 断路器，防止级联故障                              |
| **Outbox Pattern**  | 先持久化后投递的可靠消息模式                      |
| **SSRF**            | Server-Side Request Forgery，服务端请求伪造攻击   |
| **Credits**         | 平台计费单位，1 Credit ≈ 1 次基础操作             |
| **sync_wait_ms**    | 同步等待时长，接口在该时间内轮询结果后返回        |
| **并发聚合**        | 同时查询多个搜索引擎并合并结果的策略              |
| **Jaro-Winkler**    | 字符串相似度算法，用于搜索结果去重                |

---

## 12. 未来规划

### 12.1 Phase 2（Q2 2025）

- [ ] 支持更多 LLM 模型（Llama 3、Mistral）
- [ ] 实现分布式追踪（OpenTelemetry）
- [ ] 优化大规模爬取的内存占用
- [ ] 支持自定义 JavaScript 注入
- [ ] 搜索结果语义去重（Sentence Transformers）
- [ ] 智能引擎选择（基于查询语言和历史成功率）
- [ ] 搜索质量评分（NDCG/MAP 指标）

### 12.2 Phase 3（Q3 2025）

- [ ] 提供官方 Python/Go/Java SDK
- [ ] 支持实时数据流（WebSocket Push）
- [ ] 引入机器学习反爬检测模型
- [ ] 支持多租户隔离

---

## 13. 变更记录

| 版本   | 日期       | 变更内容                                 | 作者       |
| ------ | ---------- | ---------------------------------------- | ---------- |
| v2.1.0 | 2024-12-20 | 新增搜索聚合、统一任务管理、同步等待优化 | 架构团队   |
| v2.0.0 | 2024-12-10 | Rust 重构初始版本                        | 架构团队   |
| v1.5.0 | 2024-01-15 | Node.js 版本最终版                       | 原开发团队 |

---

**批准人**: CTO  
**生效日期**: 2024-12-10