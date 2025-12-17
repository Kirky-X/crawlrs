# crawlrs - 产品需求文档 (PRD)

## 版本信息
- **文档版本**: v2.0.0
- **项目类型**: 全新 Rust 重构项目（无历史包袱）
- **最近更新**: 2024-12-10
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

### 3.1 搜索 (Search)
**功能描述**: 调用外部搜索引擎（Google/Bing）获取结果，可选批量抓取回填内容。

**输入参数**:
- `query`: 搜索关键词（必填）
- `sources`: 来源类型（web/news/images，默认 web）
- `limit`: 结果数量（1-100，默认 10）
- `scrape_options`: 抓取配置（可选）
- `async_scraping`: 是否异步抓取（默认 false）

**输出响应**:
```json
{
  "success": true,
  "data": {
    "web": [
      {
        "title": "Example Title",
        "url": "https://example.com",
        "description": "...",
        "content": "..."  // 仅在 async_scraping=false 时返回
      }
    ]
  },
  "scrape_ids": ["uuid-1", "uuid-2"],  // 异步任务 ID
  "credits_used": 15
}
```

**业务规则**:
1. 每个搜索请求消耗 **1 Credit**
2. 每个回填抓取额外消耗 **1-5 Credits**（视内容复杂度）
3. 异步模式下立即返回任务 ID，结果通过 Webhook 回调

---

### 3.2 抓取 (Scrape) ⚠️
**功能描述**: 对单个 URL 执行内容获取，支持多格式输出和页面交互。
**输入参数**:
- `url`: 目标 URL（必填）
- `formats`: 输出格式数组（markdown/html/rawHtml/json/screenshot/links）
- `actions`: 页面交互动作（wait/click/scroll/screenshot）
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

**业务规则**:
1. 基础抓取消耗 **1 Credit**
2. 截图/PDF 生成额外消耗 **2 Credits**
3. 使用代理额外消耗 **1 Credit**
4. 失败自动重试最多 **3 次**（指数退避）

**实现状态**: ⚠️ 部分实现
- [x] 支持基本抓取功能
- [x] 支持自定义HTTP头
- [x] 支持超时设置
- [x] 支持移动端模拟
- [x] 支持多种输出格式（包括截图）
- [x] 支持失败重试机制
- [x] 支持截图功能
- [ ] 未实现代理配置
- [ ] 未实现跳过TLS校验
---

### 3.3 爬取 (Crawl) ⚠️
**功能描述**: 全站递归爬取，支持深度控制、路径过滤和并发限制。**输入参数**:
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

**实现状态**: ⚠️ 部分实现
- [x] 实现了基本的爬取功能
- [x] 实现了深度控制
- [x] 实现了路径过滤（包含/排除模式）
- [x] 实现了robots.txt遵守
- [ ] 未实现并发限制
- [ ] 未实现任务过期机制---

### 3.4 提取 (Extract) ⚠️
**功能描述**: 基于 LLM 对页面集合进行结构化数据提取。

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
}
```

**业务规则**:
1. 每 1000 tokens 消耗 **10 Credits**
2. 并发受团队配额限制
3. 提取失败的 URL 不扣除 Credits

**实现状态**: ⚠️ 部分实现
- [x] 实现了基本的提取功能（基于CSS选择器的结构化数据提取）
- [ ] 未集成LLM模型
- [ ] 未实现Prompt/schema提取
- [ ] 未实现Tokens计费
---

### 3.5 状态查询与取消
**功能描述**: 查询异步任务状态、获取分页结果、取消进行中的任务。

**端点**:
- `GET /v1/scrape/:id` - 获取 Scrape 状态
- `GET /v1/crawl/:id` - 获取 Crawl 状态
- `GET /v1/crawl/:id/results?page=1&limit=50` - 分页结果
- `DELETE /v1/crawl/:id` - 取消任务

**状态枚举**:
- `queued`: 排队中
- `processing`: 进行中
- `completed`: 已完成
- `failed`: 失败
- `cancelled`: 已取消

---

## 4. 并发与限流策略

### 4.1 两层限流模型 ⚠️

#### Layer 1: API 速率限制（Rate Limiter）
- **目的**: 防止 API 滥用和 DDoS 攻击
- **算法**: 令牌桶（Token Bucket）
- **粒度**: 按 API Key 限制
- **配额**: 
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

**实现状态**: ⚠️ 部分实现
- [x] 实现了基于Redis的API速率限制
- [ ] 未实现团队并发限制
- [x] 支持不同套餐的配额设置

### 4.2 队列积压机制（Backlog）
当 Team Semaphore 耗尽时，任务不会直接失败，而是：
1. 进入 `tasks_backlog` 表等待
2. 设置过期时间（默认 1 小时）
3. Worker 定期轮询并尝试获取信号量
4. 过期任务自动标记为 `expired`

---

## 5. 引擎选择与回退

### 5.1 引擎能力矩阵 ⏳

| 引擎 | JS 渲染 | 截图 | TLS 指纹 | 速度 | 成本 |
|------|---------|------|---------|------|------|
| **Fetch** | ❌ | ❌ | ❌ | ⚡⚡⚡ | 💰 |
| **Playwright** | ✅ | ✅ | ❌ | ⚡ | 💰💰💰 |
| **Fire Engine (TLS)** | ❌ | ❌ | ✅ | ⚡⚡ | 💰💰 |
| **Fire Engine (CDP)** | ✅ | ✅ | ✅ | ⚡ | 💰💰💰💰 |
*注：Fire Engine 系列暂列为未来规划*

**实现状态**: ⏳ 部分实现
- [x] 实现了Fetch引擎
- [x] 实现了Playwright引擎
- [ ] 未实现Fire Engine系列引擎

### 5.2 智能路由策略 ✅
系统根据请求特征自动选择最优引擎：

```rust
// 伪代码示意
fn route_engine(request: &ScrapeRequest) -> EngineType {
    // As Fire Engine is not yet implemented, Playwright is used for advanced features.
    if request.needs_js || request.needs_screenshot {
        return EngineType::Playwright;
    }
    // `detect_anti_bot` logic will be added once Fire Engine is integrated.
    return EngineType::Fetch;
}
```

**实现状态**: ✅ 已实现
- [x] 实现了引擎路由器
- [x] 实现了基于请求特征的引擎选择
- [x] 支持引擎优先级排序

### 5.3 断路器保护 ✅

当某个引擎连续失败超过 5 次时：
1. 自动开启断路器（Circuit Breaker）
2. 后续请求直接跳过该引擎
3. 30 秒后进入半开状态（Half-Open）尝试恢复
4. 成功后关闭断路器，恢复正常

**实现状态**: ✅ 已实现
- [x] 实现了断路器机制
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

### 6.2 重试策略 ✅
- **算法**: 指数退避（Exponential Backoff）
- **间隔**: 10s → 1m → 5m → 30m → 1h
- **最大重试**: 5 次
- **死信处理**: 超过最大重试次数后标记为 `dead`，等待人工介入

**实现状态**: ✅ 已实现
- [x] 实现了指数退避重试算法
- [x] 实现了最大重试次数限制
- [x] 实现了死信处理机制

### 6.3 签名校验 ⚠️
使用 HMAC-SHA256 签名保证完整性：
```http
POST /webhook HTTP/1.1
Host: customer-server.com
X-crawlrs-Signature: sha256=abc123...
X-crawlrs-Event: crawl.completed
Content-Type: application/json

{"crawl_id": "...", "status": "completed"}
```

**实现状态**: ⚠️ 部分实现
- [x] 实现了HMAC-SHA256签名机制
- [x] 在请求头中添加了X-crawlrs-Signature
- [ ] 未添加X-crawlrs-Event头部---

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

**实现状态**: ⚠️ 部分实现
- [x] 实现了Robots.txt解析工具
- [x] 实现了Robots.txt缓存机制（1小时TTL）
- [x] 在请求前检查Robots.txt规则
- [ ] 未实现Crawl-delay指令遵守### 7.3 访问控制
- **地域限制**: 支持按国家/地区屏蔽（基于 GeoIP）
- **域名黑名单**: 内置高危域名列表（恶意软件、钓鱼站点）
- **团队白名单**: 企业版支持静态 IP 白名单

---

## 8. 性能指标（SLO） ⚠️

### 8.1 目标指标
| 指标 | 目标值 | 测量方式 |
|------|--------|----------|
| **API 吞吐量** | 10000 RPS | 压力测试 |
| **P50 延迟** | < 50ms | Prometheus |
| **P99 延迟** | < 200ms | Prometheus |
| **任务处理速度** | 1000 tasks/min | Worker 指标 |
| **成功率** | > 99.9% | 错误率统计 |
| **可用性** | 99.95% | Uptime 监控 |

### 8.2 降级策略
当系统负载超过 80% 时：
1. 自动增加队列积压超时时间
2. 暂停低优先级任务（如预爬取）
3. 限制新爬取任务的最大深度
4. 触发告警通知运维团队

**实现状态**: ⚠️ 部分实现
- [x] 实现了Prometheus指标采集
- [x] 实现了结构化日志记录
- [x] 实现了/metrics端点
- [ ] 未完全实现降级策略
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
| 告警级别 | 触发条件 | 响应时间 |
|---------|---------|---------|
| **P0-Critical** | 服务完全不可用 | 5 分钟 |
| **P1-High** | 成功率 < 95% | 15 分钟 |
| **P2-Medium** | 队列积压 > 10000 | 30 分钟 |
| **P3-Low** | 单个引擎故障 | 1 小时 |

---

## 11. 术语表

| 术语 | 定义 |
|------|------|
| **NuQ** | Node-Unique Queue，基于 Postgres 的自定义队列系统 |
| **Backlog** | 并发受限时的任务积压表 |
| **Semaphore** | 信号量，用于控制并发数量 |
| **Circuit Breaker** | 断路器，防止级联故障 |
| **Outbox Pattern** | 先持久化后投递的可靠消息模式 |
| **SSRF** | Server-Side Request Forgery，服务端请求伪造攻击 |
| **Credits** | 平台计费单位，1 Credit ≈ 1 次基础操作 |

---

## 12. 未来规划

### 12.1 Phase 2（Q2 2025）
- [ ] 支持更多 LLM 模型（Llama 3、Mistral）
- [ ] 实现分布式追踪（OpenTelemetry）
- [ ] 优化大规模爬取的内存占用
- [ ] 支持自定义 JavaScript 注入

### 12.2 Phase 3（Q3 2025）
- [ ] 提供官方 Python/Go/Java SDK
- [ ] 支持实时数据流（WebSocket Push）
- [ ] 引入机器学习反爬检测模型
- [ ] 支持多租户隔离

---

## 13. 变更记录

| 版本 | 日期 | 变更内容 | 作者 |
|------|------|---------|------|
| v2.0.0 | 2024-12-10 | Rust 重构初始版本 | 架构团队 |
| v1.5.0 | 2024-01-15 | Node.js 版本最终版 | 原开发团队 |

---

**批准人**: CTO  
**生效日期**: 2024-12-10