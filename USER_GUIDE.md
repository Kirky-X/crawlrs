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

### 1. 获取 API Key

访问控制台创建 API Key：

```bash
https://console.crawlrs.com/api-keys
```

### 2. 发起第一个请求

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"]
  }'
```

### 3. 查看响应

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
  },
  "credits_used": 1
}
```

---

## 核心功能

### 搜索 (Search)

> **注意**: 搜索功能基于 Google Custom Search API，需要配置 Google API Key 和 Custom Search Engine ID。

#### 基本搜索

搜索网页内容并返回结果：

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust web scraping",
    "sources": ["web"],
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
  },
  "credits_used": 1
}
```

#### 搜索 + 异步抓取

搜索后自动抓取每个结果页面的完整内容：

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust programming",
    "limit": 5,
    "async_scraping": true,
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
  ],
  "credits_used": 6
}
```

查询抓取状态：

```bash
curl http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 计费说明

- 搜索请求: **1 Credit**
- 每个回填抓取: **1-5 Credits**（视内容复杂度）

---

### 抓取 (Scrape)

#### 基本抓取

抓取单个网页内容：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
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
- `screenshot` - 页面截图（Base64）
- `links` - 提取所有链接

#### 自定义选项

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
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

#### 页面交互

对于动态加载的内容，可以执行页面操作：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
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
        "type": "click",
        "selector": "#load-more-button"
      },
      {
        "type": "scroll",
        "direction": "down"
      },
      {
        "type": "screenshot",
        "fullPage": true
      }
    ]
  }'
```

**支持的操作**:

- `wait` - 等待指定时间
- `click` - 点击元素
- `scroll` - 滚动页面
- `screenshot` - 截图

#### 截图功能

生成页面截图：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["screenshot"],
    "options": {
      "screenshot": {
        "fullPage": true,
        "type": "png"
      }
    }
  }'
```

**响应**:

```json
{
  "success": true,
  "data": {
    "screenshot": "iVBORw0KGgoAAAANSUhEUgAA...",
    "metadata": {
      "width": 1920,
      "height": 1080,
      "format": "png"
    }
  },
  "credits_used": 3
}
```

#### 计费说明

- 基础抓取: **1 Credit**
- 截图/PDF: **+2 Credits**
- 使用代理: **+1 Credit**

---

### 爬取 (Crawl)

#### 全站爬取

递归爬取整个网站：

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
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
  "completed": 0,
  "expires_at": "2024-12-11T00:00:00Z"
}
```

#### 路径过滤

只爬取特定路径：

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
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
- 支持正则表达式
- `include_paths` 优先于 `exclude_paths`

#### 查询爬取状态

```bash
curl http://localhost:8899/v1/crawl/crawl-550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**响应**:

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "total": 150,
  "completed": 75,
  "failed": 2,
  "created_at": "2024-12-10T10:00:00Z"
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
  -H "Authorization: Bearer YOUR_API_KEY"
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
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 并发控制

爬取任务受团队套餐限制：

| 套餐  | 最大并发 | 最大深度 | 最大页面数  |
|-----|------|------|--------|
| 免费版 | 5    | 5    | 100    |
| 专业版 | 20   | 10   | 10,000 |
| 企业版 | 100  | 不限   | 不限     |

#### Robots.txt 遵守

默认遵守网站的 robots.txt 规则：

```bash
# 强制忽略 robots.txt
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
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
  -H "Authorization: Bearer YOUR_API_KEY" \
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
  "data": [
    {
      "url": "https://example.com/product1",
      "extracted": {
        "product_name": "Wireless Mouse",
        "price": 29.99,
        "availability": "in_stock"
      }
    }
  ],
  "tokens_used": 1234,
  "credits_used": 12
}
```

#### 基于 Schema 提取

使用 JSON Schema 定义结构：

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com/product"],
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
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": ["https://example.com"],
    "prompt": "Extract key information",
    "options": {
      "agent": "gpt-4",
      "max_concurrency": 5
    }
  }'
```

**支持的模型**:

- `gpt-4` - OpenAI GPT-4（最准确）
- `claude-3` - Anthropic Claude 3
- `gemini-pro` - Google Gemini Pro

#### 计费说明

- 每 1000 tokens: **10 Credits**
- 提取失败不扣费

---

## 高级特性

### 环境变量配置

#### FireEngine 配置

FireEngine 使用 Flaresolverr 后端来处理需要 JavaScript 渲染的页面。您可以通过环境变量配置 Flaresolverr 服务地址：

```bash
# 设置 Flaresolverr 服务地址（默认: http://localhost:8191/v1）
export FIRE_ENGINE_URL=http://your-flaresolverr-server:8191/v1
```

**配置说明**:

- `FIRE_ENGINE_URL`: Flaresolverr 服务的完整 URL 地址
- 如果未设置，默认使用 `http://localhost:8191/v1`
- 确保 Flaresolverr 服务正在运行并可访问

**示例**:

```bash
# 本地开发环境
export FIRE_ENGINE_URL=http://localhost:8191/v1

# 生产环境
export FIRE_ENGINE_URL=http://flaresolverr.internal:8191/v1
```

### Webhook 回调

#### 配置 Webhook

在控制台配置 Webhook URL：

```
https://console.crawlrs.com/webhooks
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

#### 限制规则

| 套餐  | RPM (请求/分钟) | 并发任务 |
|-----|-------------|------|
| 免费版 | 100         | 5    |
| 专业版 | 1,000       | 20   |
| 企业版 | 10,000      | 100  |

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

### 错误处理

#### 错误响应格式

```json
{
  "success": false,
  "error": "Error message",
  "error_code": "SSRF_DETECTED",
  "details": {
    "url": "http://192.168.1.1"
  }
}
```

#### 常见错误码

| 错误码                   | HTTP 状态 | 说明          |
|-----------------------|---------|-------------|
| `INVALID_URL`         | 400     | URL 格式错误    |
| `SSRF_DETECTED`       | 400     | 检测到 SSRF 攻击 |
| `UNAUTHORIZED`        | 401     | API Key 无效  |
| `FORBIDDEN`           | 403     | 权限不足        |
| `NOT_FOUND`           | 404     | 资源不存在       |
| `RATE_LIMIT_EXCEEDED` | 429     | 速率限制        |
| `SEMAPHORE_EXHAUSTED` | 503     | 并发槽位用尽      |
| `INTERNAL_ERROR`      | 500     | 服务器内部错误     |

#### 重试策略

建议使用指数退避重试：

```python
import time
import requests

def retry_with_backoff(func, max_retries=3):
    for i in range(max_retries):
        try:
            return func()
        except requests.exceptions.RequestException as e:
            if i == max_retries - 1:
                raise
            wait_time = 2 ** i
            time.sleep(wait_time)
```

---

## 最佳实践

### 1. 优化抓取性能

**使用正确的引擎**:

- 静态页面 → 不指定（自动选择 ReqwestEngine）
- SPA 应用 → 需要 JS 渲染
- 需要截图 → 明确指定 `formats: ["screenshot"]`

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
# 只需要文本 → 使用 markdown（1 Credit）
"formats": ["markdown"]

# 需要截图 → 额外消耗 2 Credits
"formats": ["markdown", "screenshot"]
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

**原因**: 团队并发槽位已用尽

**解决方案**:

- 等待其他任务完成
- 升级套餐增加并发限制
- 取消不需要的任务

#### 2. 抓取返回空内容

**原因**: 页面需要 JavaScript 渲染

**解决方案**:

```bash
# 让系统自动选择 PlaywrightEngine 引擎
curl -X POST http://localhost:8899/v1/scrape \
  -d '{
    "url": "https://spa-app.com",
    "formats": ["markdown"]
  }'
```

#### 3. 爬取任务超时

**原因**: 站点规模过大或爬取过慢

**解决方案**:

- 减小 `max_depth` 和 `limit`
- 使用 `include_paths` 过滤
- 增加 `max_concurrency`（需要升级套餐）

#### 4. Webhook 未收到回调

**检查清单**:

- [ ] Webhook URL 可公网访问
- [ ] 服务器返回 2xx 状态码
- [ ] 检查 `webhook_events` 表状态
- [ ] 查看错误日志

### 调试技巧

#### 查看详细日志

在请求头中添加：

```
X-Debug: true
```

响应会包含详细的执行信息：

```json
{
  "success": true,
  "data": { ... },
  "debug": {
    "engine_used": "playwright",
    "execution_time_ms": 2345,
    "retry_count": 0
  }
}
```

#### 测试 Webhook

使用 webhook.site 测试：

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -d '{
    "url": "https://example.com",
    "webhook_url": "https://webhook.site/your-unique-id"
  }'
```

---

## 获取帮助

- **文档**: https://docs.crawlrs.com
- **API 参考**: https://docs.crawlrs.com/api
- **问题反馈**: https://github.com/your-org/crawlrs/issues
- **邮件支持**: support@crawlrs.com
- **社区论坛**: https://community.crawlrs.com

---

**最后更新**: 2024-12-10