# 👤 Comprehensive User Documentation
<div align="center">

![Guide](https://img.shields.io/badge/type-user%20guide-blue)
![Version](https://img.shields.io/badge/version-1.0.0-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

**Version:** 1.0.0 | **Last Updated:** 2025-07-21

</div>

---

## 📖 Table of Contents

- [Introduction](#introduction)
- [Getting Started](#getting-started)
- [Authentication](#authentication)
- [Scraping](#scraping)
- [Crawling](#crawling)
- [Searching](#searching)
- [Data Extraction](#data-extraction)
- [Webhooks](#webhooks)
- [Tasks](#tasks)
- [Teams & Usage](#teams--usage)
- [Error Handling](#error-handling)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)
- [FAQ](#faq)

---

## Introduction

Welcome to **crawlrs**, a high-performance self-hosted web scraping platform built with Rust. This guide covers all available API endpoints and features.

### What You Can Do

- **Scrape** - Extract content from single web pages
- **Crawl** - Automatically discover and scrape multiple pages
- **Search** - Query Google, Bing, Baidu, and Sogou
- **Extract** - Parse and structure data from HTML

### Key Concepts

| Concept | Description |
|---------|-------------|
| **Task** | A unit of work (scrape, crawl, extract) |
| **API Key** | Your authentication credential scoped to a team |
| **Team** | A logical group of API keys with shared usage tracking |
| **Webhook** | HTTP callback for task completion notifications |

### Available Engines

| Engine | Description | Best For |
|--------|-------------|----------|
| **Reqwest** | Pure Rust HTTP client | Static HTML, fast scraping |
| **Playwright** | Full browser automation | JavaScript-heavy SPAs |
| **Playwright (Firefox)** | Firefox-based rendering | Anti-bot bypass |
| **FlareSolverr** | Cloudflare challenge solver | Sites behind Cloudflare |

**FlareSolverr modes:**

| Mode | Description |
|------|-------------|
| `Full` | Full browser automation with session reuse, slow but reliable |
| `Cdp` | Chrome DevTools Protocol mode, moderate performance |
| `Tls` | TLS fingerprint spoofing, fastest but limited bypass capability |

---

## Getting Started

### 1. Installation

crawlrs is a single Rust binary. Build and run:

```bash
git clone https://github.com/your-org/crawlrs.git
cd crawlrs
cargo build --release

# Start the API server (default)
./target/release/crawlrs

# Or run in worker mode (requires API running)
./target/release/crawlrs worker
```

The server starts on `http://localhost:8899` by default. Configure via `config/default.toml` or environment variables (prefix `CRAWLRS__`).

### 2. Configure Authentication

Set your API key in the server config. See [Authentication](#authentication) for details.

### 3. Your First Request

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com"
  }'
```

**Response:**
```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "url": "https://example.com"
}
```

---

## Authentication

### API Key Configuration

API keys are configured on the server side. There is no cloud dashboard — keys are set via environment variables, config files, or the database.

**Environment variable:**
```bash
export CRAWLRS__AUTH__KEYS="sk-your-key-here"
```

**Config file (`config/default.toml`):**
```toml
[auth]
keys = ["sk-your-key-here"]
```

Keys are passed as a Bearer token in the `Authorization` header:

```bash
curl -H "Authorization: Bearer sk-your-key-here" http://localhost:8899/v1/scrape
```

### Scopes

API keys are scoped to restrict access:

| Scope | Description |
|--------|-------------|
| `scrape` | Single page scraping |
| `crawl` | Multi-page crawling |
| `search` | Search engine queries |
| `extract` | Data extraction |
| `admin` | Full administrative access |

### Security Best Practices

- Never commit API keys to version control
- Use environment variables for API keys
- Rotate keys regularly
- Revoke unused keys
- Limit scopes to minimum required
- Restrict network access to the server port

### Rate Limiting

Rate limits are configured server-side. Default configuration:

```toml
[rate_limit]
requests_per_minute = 60
concurrent = 10
```

Every API response includes your current rate limit status:

```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1705315200
```

Check your current limits:

```bash
curl -X GET http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

---

## Scraping

### Basic Scraping

Scrape a single page:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article/123"
  }'
```

**Response:**
```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "url": "https://example.com/article/123"
}
```

### Get Scrape Results

```bash
curl http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**
```json
{
  "success": true,
  "task": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "completed",
    "result": {
      "html": "<html>...</html>",
      "markdown": "# Article Title\n\nContent...",
      "text": "Article Title\n\nContent..."
    }
  }
}
```

### Output Formats

Request data in multiple formats:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article",
    "formats": ["html", "markdown", "text"]
  }'
```

**Available Formats:**
- `html` - Raw HTML content
- `markdown` - Converted to Markdown
- `text` - Plain text without HTML tags

### Content Filtering

Include or exclude specific HTML tags:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/blog",
    "include_tags": ["h1", "h2", "p", "article"],
    "exclude_tags": ["script", "style", "nav", "footer"]
  }'
```

### Extraction Rules

Extract specific data using CSS selectors:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/product",
    "extraction_rules": {
      "title": {
        "selector": "h1",
        "attribute": "text"
      },
      "price": {
        "selector": ".price",
        "attribute": "text"
      },
      "description": {
        "selector": ".description",
        "attribute": "text"
      },
      "imageUrl": {
        "selector": ".product-image img",
        "attribute": "src"
      },
      "inStock": {
        "selector": "#stock-status",
        "attribute": "data-stock"
      }
    }
  }'
```

**Result:**
```json
{
  "success": true,
  "data": {
    "title": "Premium Widget",
    "price": "$99.99",
    "description": "A high-quality widget...",
    "imageUrl": "https://example.com/images/widget.jpg",
    "inStock": "yes"
  }
}
```

### Custom Options

Configure how the scraper fetches the page:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/page",
    "options": {
      "headers": {
        "User-Agent": "Mozilla/5.0 (compatible; MyBot/1.0)",
        "Accept-Language": "en-US,en;q=0.9"
      },
      "wait_for": 2000,
      "timeout": 30,
      "js_rendering": false,
      "screenshot": true,
      "screenshot_options": {
        "full_page": true,
        "quality": 90,
        "format": "png"
      },
      "mobile": false,
      "proxy": "http://proxy.example.com:8080",
      "skip_tls_verification": false
    }
  }'
```

### Page Actions

Perform actions on the page before scraping:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/lazy-load-page",
    "options": {
      "js_rendering": true
    },
    "actions": [
      {
        "type": "wait",
        "milliseconds": 1000
      },
      {
        "type": "scroll",
        "direction": "down"
      },
      {
        "type": "wait",
        "milliseconds": 2000
      },
      {
        "type": "click",
        "selector": ".load-more-button"
      },
      {
        "type": "wait",
        "milliseconds": 3000
      },
      {
        "type": "scroll",
        "direction": "down"
      },
      {
        "type": "input",
        "selector": "#search-input",
        "text": "search query"
      },
      {
        "type": "screenshot",
        "full_page": true
      }
    ]
  }'
```

**Action Types:**

| Action | Parameters | Description |
|--------|-----------|-------------|
| `wait` | `milliseconds` | Wait before next action |
| `scroll` | `direction` | Scroll page (up/down) |
| `click` | `selector` | Click element matching selector |
| `input` | `selector`, `text` | Type text into input field |
| `screenshot` | `full_page` | Take screenshot |

### Synchronous Scraping

Wait for the scrape to complete and get results immediately:

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article",
    "sync_wait_ms": 10000
  }'
```

**Best Practices:**
- Use `sync_wait_ms` only when you need immediate results
- Recommended max: 5000ms for most cases
- Maximum: 30000ms
- For long-running tasks, use webhooks instead

### Cancel a Scrape

Stop a running scrape task:

```bash
curl -X POST http://localhost:8899/v1/scrape/550e8400-e29b-41d4-a716-446655440000/_cancel \
  -H "Authorization: Bearer YOUR_API_KEY"
```

---

## Crawling

### Basic Crawling

Crawl a website starting from a URL:

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/blog",
    "max_depth": 2,
    "max_pages": 100
  }'
```

**Response:**
```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "url": "https://example.com/blog"
}
```

### Crawl Depth

Control how deep the crawler goes:

| Depth | Description | Example |
|-------|-------------|----------|
| 0 | Only the start page | Home page only |
| 1 | Start page + linked pages | Home + 1 link deep |
| 2 | 2 levels deep | Home → Category → Article |
| 3+ | Deep crawling | Full site discovery |

**Example:**

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "max_depth": 3,
    "max_pages": 500
  }'
```

### URL Patterns

Control which URLs to crawl:

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "follow_links": true,
    "include_patterns": ["/blog/.*", "/articles/.*"],
    "exclude_patterns": ["/admin/.*", "/login/.*", "/api/.*"]
  }'
```

**Pattern Examples:**
- `/blog/.*` - Include all blog posts
- `/category/[a-z]+` - Include category pages
- `\.pdf$` - Include PDF files
- `/admin/.*` - Exclude admin pages
- `\\?.*` - Exclude URLs with query strings

### Crawl Progress

Track crawl progress:

```bash
# Poll for status
curl http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "task": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "running",
    "progress": {
      "pages_processed": 45,
      "total_pages": 100
    }
  }
}
```

### Get Crawl Results

Retrieve all pages crawled:

```bash
curl "http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000/results?page=1&limit=20" \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "results": [
    {
      "url": "https://example.com/blog/post-1",
      "markdown": "# Post Title\n\nContent..."
    }
  ],
  "pagination": {
    "total": 150,
    "page": 1,
    "limit": 20
  }
}
```

### Cancel a Crawl

Stop a running crawl via cancel endpoint:

```bash
curl -X POST http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000/_cancel \
  -H "Authorization: Bearer YOUR_API_KEY"
```

Or delete the crawl task:

```bash
curl -X DELETE http://localhost:8899/v1/crawl/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer YOUR_API_KEY"
```

---

## Searching

### Basic Search

Search using Google:

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "google",
    "query": "Rust web scraping tutorial"
  }'
```

**Response:**

```json
{
  "success": true,
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "engine": "google",
  "query": "Rust web scraping tutorial"
}
```

### Search Engines

Available search engines:

| Engine | Code | Best For |
|--------|-------|-----------|
| Google | `google` | General web search |
| Bing | `bing` | Microsoft ecosystem |
| Baidu | `baidu` | Chinese content |
| Sogou | `sogou` | Chinese content |

### Search Options

Customize your search:

```bash
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "google",
    "query": "Rust programming",
    "num_results": 20,
    "language": "en",
    "region": "us",
    "safe_search": false,
    "sync_wait_ms": 5000
  }'
```

**Available Languages:**
- `en` - English
- `zh` - Chinese
- `es` - Spanish
- `fr` - French
- `de` - German
- `ja` - Japanese
- `ko` - Korean

**Available Regions:**
- `us` - United States
- `uk` - United Kingdom
- `ca` - Canada
- `au` - Australia
- `jp` - Japan
- `cn` - China

### Get Search Results

```bash
# Synchronous (wait for completion)
curl -X POST http://localhost:8899/v1/search \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "engine": "google",
    "query": "Rust tutorial",
    "sync_wait_ms": 5000
  }'
```

**Response:**

```json
{
  "success": true,
  "results": [
    {
      "title": "Rust Programming Tutorial",
      "url": "https://example.com/rust-tutorial",
      "snippet": "Learn Rust programming...",
      "engine": "google"
    }
  ]
}
```

---

## Data Extraction

### Extract from HTML

Parse structured data from HTML:

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "html": "<html><body><h1>Title</h1><p>Content</p></body></html>",
    "extraction_rules": {
      "title": {
        "selector": "h1",
        "attribute": "text"
      },
      "content": {
        "selector": "p",
        "attribute": "text"
      }
    }
  }'
```

**Response:**

```json
{
  "success": true,
  "data": {
    "title": "Title",
    "content": "Content"
  }
}
```

### Advanced Extraction

Extract multiple elements:

```bash
curl -X POST http://localhost:8899/v1/extract \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "html": "<html>...</html>",
    "extraction_rules": {
      "headings": {
        "selector": "h1, h2, h3",
        "attribute": "text",
        "multiple": true
      },
      "links": {
        "selector": "a",
        "attribute": "href",
        "multiple": true
      },
      "images": {
        "selector": "img",
        "attribute": "src",
        "multiple": true
      },
      "meta_description": {
        "selector": "meta[name=\"description\"]",
        "attribute": "content"
      }
    }
  }'
```

---

## Webhooks

### Create Webhook

Receive notifications when tasks complete:

```bash
curl -X POST http://localhost:8899/v1/webhooks \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://your-server.com/webhook",
    "events": ["task.completed", "task.failed"],
    "secret": "your-webhook-secret",
    "active": true
  }'
```

**Response:**

```json
{
  "success": true,
  "webhook": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://your-server.com/webhook",
    "events": ["task.completed", "task.failed"],
    "active": true
  }
}
```

### List Webhooks

```bash
curl http://localhost:8899/v1/webhooks \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### Webhook Events

Subscribe to specific events:

| Event | Trigger | Use Case |
|--------|----------|-----------|
| `task.created` | Task created | Immediate tracking |
| `task.started` | Task started | Progress monitoring |
| `task.completed` | Task succeeded | Result processing |
| `task.failed` | Task failed | Error handling |
| `task.cancelled` | Task cancelled | Cleanup |

### Handle Webhook

Create an endpoint to receive webhooks:

```javascript
// Express.js example
const express = require('express');
const app = express();
const crypto = require('crypto');

app.post('/webhook', express.raw({type: 'application/json'}), (req, res) => {
  const signature = req.headers['x-webhook-signature'];
  const payload = req.body;

  const expectedSignature = crypto
    .createHmac('sha256', 'your-webhook-secret')
    .update(payload)
    .digest('hex');

  if (signature !== `sha256=${expectedSignature}`) {
    return res.status(401).json({ error: 'Invalid signature' });
  }

  const webhook = JSON.parse(payload);
  console.log('Event:', webhook.event);
  console.log('Task:', webhook.task);

  if (webhook.event === 'task.completed') {
    const result = webhook.result;
  }

  res.status(200).send('OK');
});

app.listen(3000, () => {
  console.log('Webhook server running on port 3000');
});
```

**Webhook Payload:**

```json
{
  "event": "task.completed",
  "timestamp": "2025-01-15T00:00:00Z",
  "task": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "type": "scrape",
    "status": "completed",
    "url": "https://example.com"
  },
  "result": {
    "html": "...",
    "markdown": "...",
    "text": "..."
  }
}
```

### Webhook Retry Policy

| Status Code | Retry Policy |
|-----------|--------------|
| 200-299 | Success, no retry |
| 400-499 | Retry with exponential backoff |
| 5xx | Retry with exponential backoff |
| Timeout | Retry with exponential backoff |

**Retry Schedule:**
- 1st retry: Immediately
- 2nd retry: 1 minute
- 3rd retry: 5 minutes
- 4th retry: 15 minutes
- 5th retry: 1 hour
- After 5 failed attempts: Mark webhook as failed

---

## Tasks

### Query Tasks

Search and filter tasks across all types:

```bash
curl -X POST http://localhost:8899/v1/tasks/_query \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "status": "running",
    "type": "scrape",
    "limit": 20,
    "offset": 0
  }'
```

### Cancel Task

Cancel any running task by its ID:

```bash
curl -X POST http://localhost:8899/v1/tasks/_cancel \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "550e8400-e29b-41d4-a716-446655440000"
  }'
```

---

## Teams & Usage

### Get Team Info

```bash
curl http://localhost:8899/v1/teams/me \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "team": {
    "id": "team-id-123",
    "name": "My Team"
  }
}
```

### Get Usage Stats

```bash
curl http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

**Response:**

```json
{
  "success": true,
  "usage": {
    "requests_today": 150,
    "rate_limit_info": {
      "limit": 60,
      "remaining": 45,
      "reset": 1705315200
    }
  }
}
```

---

## Error Handling

### Common Errors

**1. Rate Limit Exceeded**

```json
{
  "success": false,
  "error": "Rate limit exceeded: 60 requests/minute"
}
```

**Solution:**
```javascript
try {
  const response = await axios.post(url, data, {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });
} catch (error) {
  if (error.response?.status === 429) {
    const retryAfter = error.response.headers['retry-after'];
    await new Promise(resolve => setTimeout(resolve, retryAfter * 1000));
    return makeRequest(url, data);
  }
  throw error;
}
```

**2. Invalid URL**

```json
{
  "success": false,
  "error": "Invalid URL: must be http:// or https://"
}
```

**Solution:**
```javascript
function isValidUrl(url) {
  try {
    new URL(url);
    return url.startsWith('http://') || url.startsWith('https://');
  } catch {
    return false;
  }
}

if (!isValidUrl(userUrl)) {
  throw new Error('Invalid URL format');
}
```

**3. SSRF Blocked**

```json
{
  "success": false,
  "error": "SSRF protection: Internal URLs are not allowed"
}
```

**Solution:**
- Never request internal URLs (localhost, 127.0.0.1, private IPs)
- Always validate URLs before sending

**4. Authentication Failed**

```json
{
  "success": false,
  "error": "Invalid or missing API key"
}
```

**Solution:**
- Verify the `Authorization` header is set correctly
- Check the API key is configured on the server

### Error Handling Best Practices

1. **Always Check `success` Field**

```javascript
const response = await makeRequest();
if (!response.data.success) {
  console.error('API error:', response.data.error);
}
```

2. **Use Exponential Backoff for Retries**

```javascript
async function retryWithBackoff(fn, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      return await fn();
    } catch (error) {
      if (i === maxRetries - 1) throw error;
      const delay = Math.pow(2, i) * 1000;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}
```

3. **Implement Circuit Breaker**

```javascript
class CircuitBreaker {
  constructor(threshold = 5, resetTimeout = 60000) {
    this.failureCount = 0;
    this.threshold = threshold;
    this.state = 'closed';
    this.nextAttempt = Date.now();
    this.resetTimeout = resetTimeout;
  }

  async execute(fn) {
    if (this.state === 'open' && Date.now() < this.nextAttempt) {
      throw new Error('Circuit breaker is open');
    }

    try {
      const result = await fn();
      this.onSuccess();
      return result;
    } catch (error) {
      this.onFailure();
      throw error;
    }
  }

  onSuccess() {
    this.failureCount = 0;
    this.state = 'closed';
  }

  onFailure() {
    this.failureCount++;
    if (this.failureCount >= this.threshold) {
      this.state = 'open';
      this.nextAttempt = Date.now() + this.resetTimeout;
    }
  }
}
```

---

## Best Practices

### 1. Use Async Mode for Long-Running Tasks

**Don't:**

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "sync_wait_ms": 30000}'
```

**Do:**

```bash
curl -X POST http://localhost:8899/v1/crawl \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "webhook": "https://your-server.com/callback"}'
```

### 2. Implement Proper Retry Logic

```javascript
async function makeRequestWithRetry(url, data, retries = 3) {
  for (let i = 0; i < retries; i++) {
    try {
      const response = await axios.post(url, data, {
        headers: { 'Authorization': `Bearer ${API_KEY}` },
        timeout: 30000
      });
      return response.data;
    } catch (error) {
      if (error.response?.status >= 400 && error.response?.status < 500 && error.response?.status !== 429) {
        throw error;
      }

      const delay = Math.pow(2, i) * 1000;
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
}
```

### 3. Cache Results When Appropriate

```javascript
const cache = new Map();

async function scrapeWithCache(url) {
  const cacheKey = `scrape:${url}`;

  if (cache.has(cacheKey)) {
    return cache.get(cacheKey);
  }

  const response = await axios.post('http://localhost:8899/v1/scrape', {
    url: url,
    formats: ['markdown']
  }, {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });

  cache.set(cacheKey, response.data);
  return response.data;
}
```

### 4. Use Specific Output Formats

**Don't:**

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "formats": ["html", "markdown", "text"]}'
```

**Do:**

```bash
curl -X POST http://localhost:8899/v1/scrape \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "formats": ["markdown"]}'
```

### 5. Monitor Server Usage

```bash
# Check usage regularly
curl http://localhost:8899/v1/teams/me/usage \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### 6. Validate Inputs Before Sending

```javascript
function validateScrapeRequest(request) {
  const errors = [];

  if (!request.url) {
    errors.push('URL is required');
  }

  if (!isValidUrl(request.url)) {
    errors.push('Invalid URL format');
  }

  if (request.sync_wait_ms && request.sync_wait_ms > 30000) {
    errors.push('sync_wait_ms must be <= 30000');
  }

  if (errors.length > 0) {
    throw new Error(`Validation errors: ${errors.join(', ')}`);
  }

  return true;
}
```

### 7. Use Environment Variables for Secrets

```bash
# .env file
CRAWLRS_API_KEY=sk-your-key-here
CRAWLRS_WEBHOOK_SECRET=my-secret-key
CRAWLRS__SERVER__PORT=8899
CRAWLRS__DATABASE__URL=postgres://user:pass@localhost/crawlrs
```

```javascript
require('dotenv').config();

const API_KEY = process.env.CRAWLRS_API_KEY;
const WEBHOOK_SECRET = process.env.CRAWLRS_WEBHOOK_SECRET;

const response = await axios.post('http://localhost:8899/v1/scrape', data, {
  headers: { 'Authorization': `Bearer ${API_KEY}` }
});
```

**.gitignore:**

```
.env
.env.local
.env.production
```

### 8. Use Appropriate Engines

| Scenario | Recommended Engine | Reason |
|----------|-------------------|---------|
| Static HTML pages | Reqwest | Fastest, lowest overhead |
| JavaScript-heavy SPAs | Playwright | Renders JS |
| Anti-bot protection | Playwright (Firefox) | Bypasses detection |
| Cloudflare-protected sites | FlareSolverr | Solves challenge |
| Simple data extraction | Reqwest | Fast, efficient |
| Screenshots needed | Playwright | Full page rendering |
| High-volume scraping | Reqwest | Best performance |

---

## Troubleshooting

### Issues & Solutions

**1. "Rate limit exceeded" frequently**

**Problem:** Getting 429 errors even with low usage

**Solutions:**
- Check if multiple clients are using the same API key
- Implement proper rate limiting in your application
- Increase `requests_per_minute` in your server config
- Use async mode (webhooks) instead of sync waiting

**2. "SSRF protection: Internal URLs are not allowed"**

**Problem:** Request to internal URL is blocked

**Solutions:**
- Ensure URL is a public internet URL
- Never use localhost, 127.0.0.1, or internal IPs
- Validate URLs before sending to API

**3. Task stuck in "running" status**

**Problem:** Task never completes

**Solutions:**
- Check if the target URL is accessible
- Verify the URL isn't blocking bots
- Try with a different engine (Playwright vs Reqwest vs FlareSolverr)
- Check your webhook endpoint is responding
- Check worker logs if running in worker mode

**4. Webhook not being called**

**Problem:** Tasks complete but webhook not triggered

**Solutions:**
- Verify webhook URL is accessible from the crawlrs server
- Check your server logs for incoming requests
- Verify webhook signature is being sent correctly
- Test webhook endpoint manually:

  ```bash
  curl -X POST https://your-server.com/webhook \
    -H 'Content-Type: application/json' \
    -d '{"event":"test"}'
  ```

**5. Slow response times**

**Problem:** API requests take longer than expected

**Solutions:**
- Use synchronous mode only when necessary
- Reduce payload size (fewer formats, smaller sync_wait_ms)
- Use caching for repeated requests
- Check your network latency to the crawlrs server
- Ensure the server has sufficient resources (CPU, memory)

**6. Engine not available**

**Problem:** Requested engine returns an error

**Solutions:**
- Verify FlareSolverr is running and accessible if using that engine
- Check Playwright browsers are installed (`npx playwright install`)
- Ensure Reqwest engine is compiled in (default)
- Check server logs for engine-specific errors

---

## FAQ

### General Questions

**Q: What is the difference between scrape and crawl?**

A: **Scrape** extracts content from a single page. **Crawl** automatically discovers and scrapes multiple linked pages starting from a URL.

**Q: How do I install and run crawlrs?**

A: crawlrs is a single Rust binary. Clone the repo, run `cargo build --release`, then execute `./target/release/crawlrs`. See the [Getting Started](#getting-started) section for details. The server starts on `http://localhost:8899` by default.

**Q: Can I scrape any website?**

A: Most websites, but the server respects `robots.txt` and may block sites that explicitly prohibit scraping. Always check a website's Terms of Service.

**Q: What happens if a task fails?**

A: Failed tasks are logged and you receive a webhook notification (if configured). You can retry the task with the same parameters.

### Technical Questions

**Q: How do I handle JavaScript-rendered content?**

A: Use Playwright engine by setting `js_rendering: true` in options:

```json
{
  "options": {
    "js_rendering": true
  }
}
```

**Q: How do I bypass Cloudflare protection?**

A: Use the FlareSolverr engine. Choose the appropriate mode (`Full`, `Cdp`, or `Tls`) based on your needs:

```json
{
  "options": {
    "engine": "flaresolverr",
    "flaresolverr_mode": "Full"
  }
}
```

**Q: Can I use a proxy?**

A: Yes, specify a proxy in options:

```json
{
  "options": {
    "proxy": "http://user:pass@proxy.example.com:8080"
  }
}
```

**Q: How do I extract data from multiple elements?**

A: Use `multiple: true` in extraction rules:

```json
{
  "extraction_rules": {
    "links": {
      "selector": "a",
      "attribute": "href",
      "multiple": true
    }
  }
}
```

**Q: What is the maximum sync_wait_ms?**

A: Maximum is 30,000 milliseconds (30 seconds). For longer tasks, use webhooks for async processing.

**Q: How do I configure rate limits?**

A: Rate limits are set in the server config file (`config/default.toml`):

```toml
[rate_limit]
requests_per_minute = 60
concurrent = 10
```

---

## Support

### Getting Help

- 📖 [Documentation](/)
- 📚 [API Reference](API_REFERENCE.md)
- 🏗️ [Architecture](ARCHITECTURE.md)
- 🐛 [Issue Tracker](https://github.com/your-org/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com

### Reporting Bugs

When reporting bugs, include:
1. API endpoint used
2. Request parameters (sanitized)
3. Expected vs actual behavior
4. Error messages
5. Server logs if available
6. Reproduction steps

### Feature Requests

Submit feature requests via:
- GitHub Issues (with "enhancement" label)
- Email to maintainer

---

**Happy Scraping! 🚀**
