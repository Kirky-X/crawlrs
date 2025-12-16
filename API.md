# crawlrs API 文档

**Version**: v1  
**Base URL**: `https://api.crawlrs.com/v1`  
**Authentication**: Bearer Token

---

## 目录

- [认证](#认证)
- [通用规范](#通用规范)
- [搜索 API](#搜索-api)
- [抓取 API](#抓取-api)
- [爬取 API](#爬取-api)
- [提取 API](#提取-api)
- [错误码](#错误码)
- [限流与配额](#限流与配额)

---

## 认证

### API Key 认证

所有请求需在 HTTP 头中包含 API Key：

```http
Authorization: Bearer YOUR_API_KEY
```

**示例**:

```bash
curl https://api.crawlrs.com/v1/scrape \
  -H "Authorization: Bearer sk_live_abc123..."
```

### 获取 API Key

1. 登录控制台: https://console.crawlrs.com
2. 导航到 API Keys 页面
3. 点击 "Create New Key"
4. 复制并安全存储

⚠️ **注意**: 请勿在客户端代码中暴露 API Key

---

## 通用规范

### 请求格式

- **Content-Type**: `application/json`
- **字符编码**: UTF-8

### 响应格式

成功响应：

```json
{
  "success": true,
  "data": { ... },
  "credits_used": 1
}
```

错误响应：

```json
{
  "success": false,
  "error": "Error message",
  "error_code": "ERROR_CODE"
}
```

### 时间格式

所有时间使用 ISO 8601 格式（UTC）：

```
2024-12-10T10:30:45.123Z
```

### 分页

使用 `page` 和 `limit` 参数：

```
GET /v1/crawl/:id/results?page=1&limit=50
```

响应包含分页信息：

```json
{
  "data": [...],
  "pagination": {
    "page": 1,
    "limit": 50,
    "total": 150,
    "total_pages": 3
  }
}
```

---

## 搜索 API

### POST /v1/search

搜索网页内容并可选择性抓取结果。

#### 请求参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `query` | string | ✅ | - | 搜索关键词 |
| `sources` | array | ❌ | ["web"] | 搜索来源 |
| `limit` | integer | ❌ | 10 | 结果数量 (1-100) |
| `async_scraping` | boolean | ❌ | false | 是否异步抓取 |
| `scrape_options` | object | ❌ | {} | 抓取配置 |

**sources 可选值**:
- `web` - 网页搜索
- `news` - 新闻搜索
- `images` - 图片搜索

#### 请求示例

```bash
curl -X POST https://api.crawlrs.com/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust web scraping",
    "sources": ["web"],
    "limit": 10,
    "async_scraping": true,
    "scrape_options": {
      "formats": ["markdown"]
    }
  }'
```

#### 响应示例

**同步模式** (`async_scraping: false`):

```json
{
  "success": true,
  "data": {
    "web": [
      {
        "title": "Web Scraping with Rust",
        "url": "https://example.com/rust-scraping",
        "description": "Learn how to build web scrapers...",
        "content": "# Web Scraping with Rust\n\n..."
      }
    ]
  },
  "credits_used": 11
}
```

**异步模式** (`async_scraping: true`):

```json
{
  "success": true,
  "data": {
    "web": [
      {
        "title": "Web Scraping with Rust",
        "url": "https://example.com/rust-scraping",
        "description": "Learn how to build web scrapers..."
      }
    ]
  },
  "scrape_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
  ],
  "credits_used": 1
}
```

#### 计费规则

- 搜索请求: **1 Credit**
- 同步抓取: 每个结果 **1-5 Credits**
- 异步抓取: 创建时不计费，完成时结算

---

## 抓取 API

### POST /v1/scrape

抓取单个网页内容。

#### 请求参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `url` | string | ✅ | - | 目标 URL |
| `formats` | array | ❌ | ["markdown"] | 输出格式 |
| `options` | object | ❌ | {} | 抓取选项 |
| `actions` | array | ❌ | [] | 页面交互动作 |
| `webhook_url` | string | ❌ | - | Webhook 回调地址 |

**formats 可选值**:
- `markdown` - Markdown 格式
- `html` - 清理后的 HTML
- `rawHtml` - 原始 HTML
- `screenshot` - 页面截图（Base64）
- `links` - 提取所有链接

**options 参数**:

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `headers` | object | {} | 自定义 HTTP 头 |
| `timeout` | integer | 30 | 超时时间（秒） |
| `mobile` | boolean | false | 模拟移动端 |
| `screenshot` | object | - | 截图配置 |

**screenshot 配置**:

```json
{
  "fullPage": true,
  "type": "png",
  "quality": 80
}
```

**actions 动作类型**:

| 类型 | 参数 | 说明 |
|------|------|------|
| `wait` | `milliseconds` | 等待指定时间 |
| `click` | `selector` | 点击元素 |
| `scroll` | `direction` | 滚动页面 |
| `screenshot` | `fullPage` | 截图 |

#### 请求示例

**基本抓取**:

```bash
curl -X POST https://api.crawlrs.com/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown", "html"]
  }'
```

**自定义选项**:

```bash
curl -X POST https://api.crawlrs.com/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"],
    "options": {
      "headers": {
        "User-Agent": "CustomBot/1.0",
        "Accept-Language": "en-US"
      },
      "timeout": 60,
      "mobile": false
    }
  }'
```

**页面交互**:

```bash
curl -X POST https://api.crawlrs.com/v1/scrape \
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
        "selector": "#load-more"
      },
      {
        "type": "scroll",
        "direction": "down"
      }
    ]
  }'
```

**截图**:

```bash
curl -X POST https://api.crawlrs.com/v1/scrape \
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

#### 响应示例

**成功响应**:

```json
{
  "success": true,
  "data": {
    "markdown": "# Example Domain\n\nThis domain is...",
    "html": "<html><body>...",
    "metadata": {
      "title": "Example Domain",
      "description": "Example Domain description",
      "status_code": 200,
      "content_type": "text/html",
      "response_time_ms": 234,
      "final_url": "https://example.com"
    }
  },
  "credits_used": 1
}
```

**截图响应**:

```json
{
  "success": true,
  "data": {
    "screenshot": "iVBORw0KGgoAAAANSUhEUgAAA...",
    "metadata": {
      "width": 1920,
      "height": 1080,
      "format": "png",
      "size_bytes": 245678
    }
  },
  "credits_used": 3
}
```

#### 计费规则

- 基础抓取: **1 Credit**
- 截图/PDF: **+2 Credits**
- 使用代理: **+1 Credit**

---

### GET /v1/scrape/:id

查询抓取任务状态（异步模式）。

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 任务 ID |

#### 请求示例

```bash
curl https://api.crawlrs.com/v1/scrape/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 响应示例

**进行中**:

```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "created_at": "2024-12-10T10:00:00Z"
}
```

**已完成**:

```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "data": {
    "markdown": "# Content...",
    "metadata": { ... }
  },
  "completed_at": "2024-12-10T10:00:05Z",
  "credits_used": 1
}
```

**失败**:

```json
{
  "success": false,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "failed",
  "error": "Timeout after 30 seconds",
  "failed_at": "2024-12-10T10:00:35Z"
}
```

---

## 爬取 API

### POST /v1/crawl

递归爬取整个网站。

#### 请求参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `url` | string | ✅ | - | 起始 URL |
| `crawler_options` | object | ❌ | {} | 爬取配置 |
| `scrape_options` | object | ❌ | {} | 抓取配置 |
| `max_concurrency` | integer | ❌ | 5 | 最大并发数 |
| `webhook_url` | string | ❌ | - | Webhook 回调地址 |

**crawler_options 参数**:

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_depth` | integer | 2 | 最大爬取深度 (1-10) |
| `limit` | integer | 100 | 最大页面数 (1-10000) |
| `include_paths` | array | [] | 包含路径（正则） |
| `exclude_paths` | array | [] | 排除路径（正则） |
| `ignore_robots` | boolean | false | 忽略 robots.txt |
| `crawl_delay_ms` | integer | 500 | 请求间隔（毫秒） |

#### 请求示例

**基本爬取**:

```bash
curl -X POST https://api.crawlrs.com/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "max_depth": 2,
      "limit": 100
    }
  }'
```

**路径过滤**:

```bash
curl -X POST https://api.crawlrs.com/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "include_paths": ["/blog/*", "/docs/*"],
      "exclude_paths": ["/admin/*", "/api/*"],
      "max_depth": 3,
      "limit": 500
    },
    "scrape_options": {
      "formats": ["markdown"]
    }
  }'
```

**高级配置**:

```bash
curl -X POST https://api.crawlrs.com/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "crawler_options": {
      "max_depth": 5,
      "limit": 1000,
      "crawl_delay_ms": 1000,
      "ignore_robots": false
    },
    "max_concurrency": 10,
    "webhook_url": "https://your-server.com/webhook"
  }'
```

#### 响应示例

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "total": 0,
  "completed": 0,
  "created_at": "2024-12-10T10:00:00Z",
  "expires_at": "2024-12-11T10:00:00Z"
}
```

---

### GET /v1/crawl/:id

查询爬取任务状态。

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 爬取 ID |

#### 请求示例

```bash
curl https://api.crawlrs.com/v1/crawl/crawl-550e8400 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 响应示例

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "processing",
  "total": 150,
  "completed": 75,
  "failed": 2,
  "created_at": "2024-12-10T10:00:00Z",
  "updated_at": "2024-12-10T10:05:00Z"
}
```

**status 可能值**:
- `queued` - 排队中
- `processing` - 进行中
- `completed` - 已完成
- `failed` - 失败
- `cancelled` - 已取消

---

### GET /v1/crawl/:id/results

获取爬取结果（分页）。

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 爬取 ID |

#### 查询参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `page` | integer | 1 | 页码 |
| `limit` | integer | 50 | 每页数量 (1-100) |

#### 请求示例

```bash
curl "https://api.crawlrs.com/v1/crawl/crawl-550e8400/results?page=1&limit=50" \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 响应示例

```json
{
  "success": true,
  "data": [
    {
      "url": "https://example.com/page1",
      "markdown": "# Page 1\n\nContent...",
      "metadata": {
        "title": "Page 1",
        "status_code": 200,
        "scraped_at": "2024-12-10T10:01:00Z"
      }
    },
    {
      "url": "https://example.com/page2",
      "markdown": "# Page 2\n\nContent...",
      "metadata": {
        "title": "Page 2",
        "status_code": 200,
        "scraped_at": "2024-12-10T10:01:05Z"
      }
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 50,
    "total": 150,
    "total_pages": 3
  }
}
```

---

### DELETE /v1/crawl/:id

取消正在进行的爬取任务。

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | string | 爬取 ID |

#### 请求示例

```bash
curl -X DELETE https://api.crawlrs.com/v1/crawl/crawl-550e8400 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

#### 响应示例

```json
{
  "success": true,
  "id": "crawl-550e8400-e29b-41d4-a716-446655440000",
  "status": "cancelled",
  "message": "Crawl task cancelled successfully"
}
```

---

## 提取 API

### POST /v1/extract

使用 LLM 进行结构化数据提取。

#### 请求参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `urls` | array | ✅ | - | 目标 URL 数组 (最多 100) |
| `prompt` | string | * | - | 提取指令 |
| `schema` | object | * | - | JSON Schema 定义 |
| `options` | object | ❌ | {} | 提取选项 |

\* `prompt` 和 `schema` 二选一

**options 参数**:

| 选项 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `agent` | string | "gpt-4" | LLM 模型 |
| `enable_web_search` | boolean | false | 允许联网搜索 |
| `max_concurrency` | integer | 5 | 最大并发数 |

**agent 可选值**:
- `gpt-4` - OpenAI GPT-4
- `claude-3` - Anthropic Claude 3
- `gemini-pro` - Google Gemini Pro

#### 请求示例

**基于 Prompt**:

```bash
curl -X POST https://api.crawlrs.com/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "urls": [
      "https://example.com/product1",
      "https://example.com/product2"
    ],
    "prompt": "Extract product name, price, and availability"
  }'
```

**基于 Schema**:

```bash
curl -X POST https://api.crawlrs.com/v1/extract \
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
    },
    "options": {
      "agent": "gpt-4"
    }
  }'
```

#### 响应示例

```json
{
  "success": true,
  "id": "extract-550e8400-e29b-41d4-a716-446655440000",
  "data": [
    {
      "url": "https://example.com/product1",
      "extracted": {
        "name": "Wireless Mouse",
        "price": 29.99,
        "in_stock": true
      }
    },
    {
      "url": "https://example.com/product2",
      "extracted": {
        "name": "USB Keyboard",
        "price": 49.99,
        "in_stock": false
      }
    }
  ],
  "tokens_used": 1234,
  "credits_used": 12
}
```

#### 计费规则

- 每 1000 tokens: **10 Credits**
- 提取失败不扣费

---

## 错误码

### HTTP 状态码

| 状态码 | 说明 |
|--------|------|
| 200 | 成功 |
| 400 | 请求参数错误 |
| 401 | 未授权（API Key 无效） |
| 403 | 禁止访问 |
| 404 | 资源不存在 |
| 429 | 速率限制 |
| 500 | 服务器内部错误 |
| 503 | 服务不可用 |

### 错误码列表

| 错误码 | HTTP 状态 | 说明 |
|--------|----------|------|
| `INVALID_URL` | 400 | URL 格式错误 |
| `INVALID_PARAMETER` | 400 | 参数无效 |
| `SSRF_DETECTED` | 400 | 检测到 SSRF 攻击 |
| `UNAUTHORIZED` | 401 | API Key 无效或过期 |
| `FORBIDDEN` | 403 | 权限不足 |
| `NOT_FOUND` | 404 | 任务或资源不存在 |
| `RATE_LIMIT_EXCEEDED` | 429 | 超过速率限制 |
| `SEMAPHORE_EXHAUSTED` | 503 | 并发槽位用尽 |
| `TIMEOUT` | 500 | 请求超时 |
| `ENGINE_FAILED` | 500 | 所有引擎失败 |
| `INTERNAL_ERROR` | 500 | 服务器内部错误 |

### 错误响应示例

```json
{
  "success": false,
  "error": "Invalid URL format",
  "error_code": "INVALID_URL",
  "details": {
    "url": "not-a-valid-url"
  }
}
```

---

## 限流与配额

### 速率限制

| 套餐 | RPM | 并发任务 | 爬取深度 | 爬取页数 |
|------|-----|---------|---------|---------|
| **免费版** | 100 | 5 | 5 | 100 |
| **专业版** | 1,000 | 20 | 10 | 10,000 |
| **企业版** | 10,000 | 100 | 不限 | 不限 |

### 限流响应

当超过速率限制时：

```json
{
  "success": false,
  "error": "Rate limit exceeded",
  "error_code": "RATE_LIMIT_EXCEEDED",
  "retry_after": 60
}
```

**响应头**:

```http
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1702209600
Retry-After: 60
```

### 并发限制

当团队并发槽位用尽时：

```json
{
  "success": false,
  "error": "Too many concurrent tasks",
  "error_code": "SEMAPHORE_EXHAUSTED",
  "details": {
    "current": 5,
    "limit": 5
  }
}
```

---

## Webhook 事件

### 事件类型

| 事件 | 触发时机 |
|------|---------|
| `scrape.completed` | 抓取任务完成 |
| `scrape.failed` | 抓取任务失败 |
| `crawl.completed` | 爬取任务完成 |
| `crawl.failed` | 爬取任务失败 |
| `extract.completed` | 提取任务完成 |

### Webhook 请求格式

```http
POST /your-webhook-endpoint HTTP/1.1
Host: your-server.com
Content-Type: application/json
X-crawlrs-Signature: sha256=abc123...
X-crawlrs-Event: crawl.completed

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

### 签名验证

使用 HMAC-SHA256 验证签名：

**Python 示例**:

```python
import hmac
import hashlib

def verify_webhook(payload, signature, secret):
    expected = hmac.new(
        secret.encode(),
        payload.encode(),
        hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(
        f"sha256={expected}",
        signature
    )
```

**Node.js 示例**:

```javascript
const crypto = require('crypto');

function verifyWebhook(payload, signature, secret) {
  const expected = crypto
    .createHmac('sha256', secret)
    .update(payload)
    .digest('hex');
  
  return signature === `sha256=${expected}`;
}
```

---

## SDK 示例

### Python

```python
import requests

class CrawlrsClient:
    def __init__(self, api_key):
        self.api_key = api_key
        self.base_url = "https://api.crawlrs.com/v1"
    
    def scrape(self, url, formats=None):
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json"
        }
        
        data = {
            "url": url,
            "formats": formats or ["markdown"]
        }
        
        response = requests.post(
            f"{self.base_url}/scrape",
            headers=headers,
            json=data
        )
        
        return response.json()

# 使用
client = CrawlrsClient("YOUR_API_KEY")
result = client.scrape("https://example.com")
print(result["data"]["markdown"])
```

### JavaScript

```javascript
class CrawlrsClient {
  constructor(apiKey) {
    this.apiKey = apiKey;
    this.baseUrl = "https://api.crawlrs.com/v1";
  }
  
  async scrape(url, formats = ["markdown"]) {
    const response = await fetch(`${this.baseUrl}/scrape`, {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${this.apiKey}`,
        "Content-Type": "application/json"
      },
      body: JSON.stringify({ url, formats })
    });
    
    return await response.json();
  }
}

// 使用
const client = new CrawlrsClient("YOUR_API_KEY");
const result = await client.scrape("https://example.com");
console.log(result.data.markdown);
```

---

## 附录

### 支持的 User-Agent

系统会自动选择合适的 User-Agent，或使用自定义 UA：

```json
{
  "options": {
    "headers": {
      "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)..."
    }
  }
}
```

### 代理配置

企业版支持代理配置：

```json
{
  "options": {
    "proxy": {
      "server": "http://proxy.example.com:8080",