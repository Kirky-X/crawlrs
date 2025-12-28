# crawlrs 使用手册

## 目录

- [快速开始](#快速开始)
- [核心功能](#核心功能)
    - [搜索 (Search)](#搜索-search)
    - [抓取 (Scrape)](#抓取-scrape)
    - [爬取 (Crawl)](#爬取-crawl)
    - [提取 (Extract)](#提取-extract)
- [高级特性](#高级特性)
- [最佳实践](#最佳实践)
- [故障排查](#故障排查)

---

## 快速开始

### 1. 配置 API Key

crawlrs 使用 Bearer Token 进行身份认证。编辑配置文件设置 API Key：

```toml
# config/default.toml

[server]
# 服务配置
host = "0.0.0.0"
port = 8899

# 认证配置
[auth]
# API Key 列表（支持多个 key）
api_keys = [
    "sk-your-api-key-1",
    "sk-your-api-key-2"
]

# 可选的 Token 白名单（留空则接受所有 api_keys）
# allowed_tokens = []
```

### 2. 启动服务

```bash
# 开发模式运行
cargo run

# 或使用配置文件启动
cargo run -- --config config/default.toml
```

### 3. 发起第一个请求

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"]
  }'
```

### 4. 查看响应

```json
{
  "success": true,
  "data": {
    "markdown": "# Example Domain\n\nThis domain is for use...",
    "metadata": {
      "title": "Example Domain",
      "status_code": 200,
      "response_time_ms": 234
    }
  }
}
```

---

## 核心功能

### 搜索 (Search)

crawlrs 支持多搜索引擎并发聚合搜索，包括 Google、Bing、Baidu 和 Sogou。

#### 基本搜索

搜索网页内容并返回结果：

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust web scraping",
    "limit": 10
  }'
```

**响应示例**:

```json
{
  "success": true,
  "data": {
    "web": [
      {
        "title": "Web Scraping with Rust",
        "url": "https://example.com/rust-scraping",
        "description": "Learn how to build web scrapers with Rust..."
      }
    ]
  }
}
```

#### 多引擎配置

指定搜索引擎（默认全部启用）：

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust programming",
    "engines": ["google", "bing"],
    "limit": 10
  }'
```

**支持的引擎**:
- `google` - Google 搜索（需要配置 Google API Key）
- `bing` - Bing 搜索
- `baidu` - Baidu 搜索
- `sogou` - Sogou 搜索

#### 搜索 + 异步抓取

搜索后自动抓取每个结果页面的完整内容：

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust programming",
    "limit": 5,
    "scrape": true,
    "scrape_options": {
      "formats": ["markdown"]
    }
  }'
```

**响应示例**:

```json
{
  "success": true,
  "data": {
    "web": [
      {
        "title": "Rust Programming Language",
        "url": "https://www.rust-lang.org"
      }
    ]
  },
  "scrape_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
  ]
}
```

查询抓取状态：

```bash
curl http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer sk-your-api-key-1"
```

---

### 抓取 (Scrape)

#### 基本抓取

抓取单个网页内容：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown", "html"]
  }'
```

**支持的格式**:

- `markdown` - Markdown 格式（推荐）
- `html` - 清理后的 HTML
- `rawHtml` - 原始 HTML
- `links` - 提取所有链接

#### 自定义选项

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"],
    "options": {
      "headers": {
        "User-Agent": "CustomBot/1.0"
      },
      "timeout": 30,
      "mobile": false
    }
  }'
```

**可用选项**:

| 选项        | 类型      | 默认值   | 说明         |
|-----------|---------|-------|------------|
| `headers` | Object  | {}    | 自定义 HTTP 头 |
| `timeout` | Number  | 30    | 超时时间（秒）    |
| `mobile`  | Boolean | false | 模拟移动端      |
| `engine`  | String  | auto  | 爬虫引擎（reqwest/playwright） |

#### 页面交互

对于动态加载的内容，可以执行页面操作：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"],
    "actions": [
      {
        "type": "wait",
        "milliseconds": 2000
      },
      {
        "type": "scroll",
        "direction": "down"
      }
    ]
  }'
```

**支持的操作**:

- `wait` - 等待指定时间
- `click` - 点击元素
- `scroll` - 滚动页面

---

### 爬取 (Crawl)

#### 全站爬取

递归爬取整个网站：

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "max_depth": 2,
      "limit": 100,
      "crawl_delay_ms": 500
    },
    "scrape_options": {
      "formats": ["markdown"]
    }
  }'
```

**响应**:

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "total": 0,
  "completed": 0
}
```

#### 路径过滤

只爬取特定路径：

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "include_paths": ["/blog/*", "/docs/*"],
      "exclude_paths": ["/admin/*", "/api/*"],
      "max_depth": 3
    }
  }'
```

**路径匹配规则**:

- 支持通配符 `*`
- `include_paths` 优先于 `exclude_paths`

#### 查询爬取状态

```bash
curl http://localhost:8899/v1/crawl/crawl-550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer sk-your-api-key-1"
```

**响应**:

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "total": 150,
  "completed": 75,
  "failed": 2
}
```

**状态说明**:

- `queued` - 排队中
- `processing` - 进行中
- `completed` - 已完成
- `failed` - 失败
- `cancelled` - 已取消

#### 获取爬取结果

分页获取已完成的页面：

```bash
curl "http://localhost:8899/v1/crawl/crawl-550e8400/results?page=1&limit=50" \
  -H "Authorization: Bearer sk-your-api-key-1"
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "url": "https://example.com/page1",
      "markdown": "# Page 1 Content...",
      "metadata": {
        "title": "Page 1",
        "status_code": 200
      }
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 50,
    "total": 150
  }
}
```

#### 取消爬取

```bash
curl -X DELETE http://localhost:8899/v1/crawl/crawl-550e8400 \
  -H "Authorization: Bearer sk-your-api-key-1"
```

#### 并发控制

```bash
# config/default.toml

[concurrency]
default_team_limit = 10
```

#### Robots.txt 遵守

默认遵守网站的 robots.txt 规则：

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "ignore_robots": true
    }
  }'
```

⚠️ **注意**: 忽略 robots.txt 可能违反网站服务条款，请谨慎使用。

---

### 提取 (Extract)

#### 基于 Prompt 提取

使用自然语言描述提取需求：

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": [
      "https://example.com/product1",
      "https://example.com/product2"
    ],
    "prompt": "Extract product name, price, and availability status"
  }'
```

**响应**:

```json
{
  "success": true,
  "results": [
    {
      "url": "https://example.com/product1",
      "data": {
        "product_name": "Wireless Mouse",
        "price": 29.99,
        "availability": "in_stock"
      },
      "error": null
    }
  ]
}
```

#### 基于 Schema 提取

使用 JSON Schema 定义结构：

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com/product"],
    "prompt": "Extract product information",
    "schema": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Product name"
        },
        "price": {
          "type": "number",
          "description": "Price in USD"
        },
        "in_stock": {
          "type": "boolean"
        }
      },
      "required": ["name", "price"]
    }
  }'
```

#### 选择 LLM 模型

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com"],
    "prompt": "Extract key information",
    "model": "gpt-3.5-turbo"
  }'
```

**支持的模型**:

- `gpt-3.5-turbo` - OpenAI GPT-3.5
- `gpt-4` - OpenAI GPT-4（需要配置）
- 其他兼容 OpenAI API 的模型

---

## 高级特性

### Webhook 回调

#### 配置 Webhook

在配置文件中设置 Webhook：

```toml
# config/default.toml

[webhook]
timeout_seconds = 10
max_retries = 3
retry_interval_seconds = 60
user_agent = "Crawlrs-Webhook/1.0"
secret = "your-webhook-secret"
```

#### 创建 Webhook

```bash
curl -X POST http://localhost:8899/v1/webhooks \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-server.com/webhook",
    "events": ["scrape.completed", "crawl.completed"],
    "secret": "optional-signing-secret"
  }'
```

#### 接收事件

当任务完成时，系统会 POST 到您的 Webhook URL：

```http
POST /your-webhook-endpoint HTTP/1.1
Host: your-server.com
X-crawlrs-Signature: sha256=abc123...
X-crawlrs-Event: crawl.completed
Content-Type: application/json

{
  "event": "crawl.completed",
  "crawl_id": "crawl-550e8400",
  "status": "completed",
  "total": 150,
  "completed": 148,
  "failed": 2,
  "timestamp": "2024-12-10T12:00:00Z"
}
```

#### 验证签名

使用 HMAC-SHA256 验证签名：

```python
import hmac
import hashlib

def verify_signature(payload, signature, secret):
    expected = hmac.new(
        secret.encode(),
        payload.encode(),
        hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(f"sha256={expected}", signature)
```

#### 事件类型

- `scrape.completed` - 抓取完成
- `scrape.failed` - 抓取失败
- `crawl.completed` - 爬取完成
- `crawl.failed` - 爬取失败
- `extract.completed` - 提取完成

### 速率限制

#### 限制配置

```toml
# config/default.toml

[rate_limiting]
enabled = true
default_rpm = 60
```

#### 限流响应

当超过限制时，返回 429 状态码：

```json
{
  "success": false,
  "error": "Rate limit exceeded",
  "retry_after": 60
}
```

**响应头**:

```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1702209600
Retry-After: 60
```

### 搜索引擎配置

#### Google 搜索配置

```toml
# config/default.toml

[google_search]
api_key = "your-google-api-key"
cx = "your-custom-search-engine-id"
```

#### LLM 配置

```toml
# config/default.toml

[llm]
api_key = "your-openai-api-key"
model = "gpt-3.5-turbo"
api_base_url = "https://api.openai.com/v1"
```

### 存储配置

```toml
# config/default.toml

[storage]
storage_type = "local"
local_path = "storage"
# s3_region = "us-east-1"
# s3_bucket = "my-bucket"
# s3_access_key = "access-key"
# s3_secret_key = "secret-key"
```

---

## 最佳实践

### 1. 优化抓取性能

**使用正确的引擎**:

- 静态页面 → 使用默认 ReqwestEngine
- SPA 应用 → 指定 PlaywrightEngine
- 需要截图 → 明确指定格式

**批量处理**:

```bash
# 不推荐：逐个抓取
for url in urls:
    scrape(url)

# 推荐：使用爬取功能
crawl(root_url, limit=100)
```

### 2. 控制成本

**使用缓存**:

- 避免重复抓取相同 URL
- 使用 CDN 缓存静态资源

**选择合适的格式**:

```bash
# 只需要文本 → 使用 markdown
"formats": ["markdown"]

# 不需要截图
"formats": ["markdown", "html"]
```

### 3. 提高成功率

**设置合理的超时**:

```bash
# 快速站点
"options": { "timeout": 10 }

# 慢速站点
"options": { "timeout": 60 }
```

**处理失败重试**:

- 使用 Webhook 接收失败通知
- 实现自动重试逻辑
- 记录失败原因

### 4. 遵守规则

**尊重 robots.txt**:

```bash
# 默认遵守
"crawler_options": {
  "ignore_robots": false
}
```

**设置爬取延迟**:

```bash
"crawler_options": {
  "crawl_delay_ms": 1000  # 每个请求间隔 1 秒
}
```

---

## 故障排查

### 常见问题

#### 1. 任务一直处于 `queued` 状态

**原因**: 并发槽位已用尽

**解决方案**:

- 等待其他任务完成
- 调整 `concurrency.default_team_limit` 配置
- 取消不需要的任务

#### 2. 抓取返回空内容

**原因**: 页面需要 JavaScript 渲染

**解决方案**:

```bash
# 使用 Playwright 引擎
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -d '{
    "url": "https://spa-app.com",
    "formats": ["markdown"],
    "options": {
      "engine": "playwright"
    }
  }'
```

#### 3. 爬取任务超时

**原因**: 站点规模过大或爬取过慢

**解决方案**:

- 减小 `max_depth` 和 `limit`
- 使用 `include_paths` 过滤
- 增加 `crawl_delay_ms` 间隔

#### 4. Webhook 未收到回调

**检查清单**:

- [ ] Webhook URL 可公网访问
- [ ] 服务器返回 2xx 状态码
- [ ] 查看服务端日志
- [ ] 验证 secret 配置

### 调试技巧

#### 查看详细日志

启动服务时设置日志级别：

```bash
RUST_LOG=debug cargo run
```

#### 测试 Webhook

使用 webhook.site 测试：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer sk-your-api-key-1" \
  -d '{
    "url": "https://example.com",
    "webhook_url": "https://webhook.site/your-unique-id"
  }'
```

---

## 获取帮助

- **GitHub Issues**: https://github.com/your-org/crawlrs/issues
- **文档**: 查看项目 README 和源码注释

---

**最后更新**: 2025-01
