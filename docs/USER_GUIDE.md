

# 👤 Comprehensive User Documentation
<div align="center">

![Guide](https://img.shields.io/badge/type-user%20guide-blue)
![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

**Version:** 0.1.0 | **Last Updated:** 2025-01-15

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
- [Error Handling](#error-handling)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)
- [FAQ](#faq)

---

## Introduction

Welcome to crawlrs, the high-performance web scraping platform built with Rust. This guide will help you get started and make the most of our API.

### What You Can Do

- **Scrape** - Extract content from single web pages
- **Crawl** - Automatically discover and scrape multiple pages
- **Search** - Query Google, Bing, Baidu, and Sogou
- **Extract** - Parse and structure data from HTML

### Key Concepts

| Concept | Description |
|---------|-------------|
| **Task** | A unit of work (scrape, crawl, extract) |
| **API Key** | Your authentication credential with specific permissions |
| **Team** | A logical group of API keys with shared limits |
| **Credits** | Resource consumption unit for tracking usage |
| **Webhook** - HTTP callback for task completion notifications |

---

## Getting Started

### 1. Sign Up

1. Visit the crawlrs registration page
2. Create your account
3. Generate your first API key

### 2. Install a Client

Choose a client library for your preferred language:

**JavaScript/Node.js:**
```bash
npm install axios
```

**Python:**
```bash
pip install requests
```

**Go:**
```bash
go get github.com/your-org/crawlrs-go-sdk
```

**Rust:**
```bash
cargo add crawlrs-client
```

### 3. Your First Request

Here's how to scrape your first page:

```bash
curl -X POST https://api.crawlrs.com/v1/scrape \
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
  "url": "https://example.com",
  "credits_used": 10
}
```

---

## Authentication

### API Key Management

**Generate API Key:**
1. Log in to your dashboard
2. Navigate to "API Keys"
3. Click "Create New Key"
4. Choose scopes and team
5. Save the key (you won't see it again)

**API Key Format:**
```
crawlrs_sk_abc123def456...
```

**Security Best Practices:**
- Never commit API keys to version control
- Use environment variables for API keys
- Rotate keys regularly
- Revoke unused keys
- Limit scopes to minimum required

### Scopes

When creating an API key, choose which features it can access:

| Scope | Description | Example Use Case |
|--------|-------------|------------------|
| `scrape` | Single page scraping | E-commerce product pages |
| `crawl` | Multi-page crawling | Blog post discovery |
| `search` | Search engine queries | Market research |
| `extract` | Data extraction | Email parsing from HTML |
| `admin` | Full administrative access | Automated backups |

**Example:**
```bash
# Create key with only scrape scope
curl -X POST https://api.crawlrs.com/v1/keys \
  -H "Authorization: Bearer ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Production Scrape Key",
    "scopes": ["scrape"],
    "team_id": "your-team-id"
  }'
```

### Rate Limits

Your API key has rate limits based on your plan:

| Plan | Requests/Minute | Concurrent |
|-------|----------------|-------------|
| Free | 30 | 3 |
| Pro | 100 | 10 |
| Enterprise | Unlimited | 100 |

**Checking Your Limits:**
```bash
curl -X GET https://api.crawlrs.com/v1/keys/YOUR_KEY/limits \
  -H "Authorization: Bearer YOUR_KEY"
```

**Rate Limit Headers:**
Every API response includes your current usage:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1705315200
```

---

## Scraping

### Basic Scraping

Scrape a single page and get the HTML:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/article/123'
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "id": "550e8400-e29b-41d4-a716-446655440000",
//   "url": "https://example.com/article/123",
//   "credits_used": 10
// }
```

### Get Scrape Results

After creating a scrape task, retrieve the results:

```javascript
const response = await axios.get('https://api.crawlrs.com/v1/scrape/${taskId}', {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data.task.result);
// {
//   "html": "<html>...</html>",
//   "markdown": "# Article Title\n\nContent...",
//   "text": "Article Title\n\nContent..."
// }
```

### Output Formats

Request data in multiple formats:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/article',
    formats: ['html', 'markdown', 'text']
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

**Available Formats:**
- `html` - Raw HTML content
- `markdown` - Converted to Markdown
- `text` - Plain text without HTML tags

### Content Filtering

Include or exclude specific HTML tags:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/blog',
    include_tags: ['h1', 'h2', 'p', 'article'],
    exclude_tags: ['script', 'style', 'nav', 'footer']
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

### Extraction Rules

Extract specific data using CSS selectors:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/product',
    extraction_rules: {
      title: {
        selector: 'h1',
        attribute: 'text'
      },
      price: {
        selector: '.price',
        attribute: 'text'
      },
      description: {
        selector: '.description',
        attribute: 'text'
      },
      imageUrl: {
        selector: '.product-image img',
        attribute: 'src'
      },
      inStock: {
        selector: '#stock-status',
        attribute: 'data-stock'
      }
    }
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
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

Configure how the scraper should fetch the page:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/page',
    options: {
      headers: {
        'User-Agent': 'Mozilla/5.0 (compatible; MyBot/1.0)',
        'Accept-Language': 'en-US,en;q=0.9'
      },
      wait_for: 2000,           // Wait 2 seconds for page load
      timeout: 30,                // 30 second timeout
      js_rendering: false,       // Don't use JavaScript rendering
      screenshot: true,            // Take a screenshot
      screenshot_options: {
        full_page: true,
        quality: 90,
        format: 'png'
      },
      mobile: false,               // Don't simulate mobile
      proxy: 'http://proxy.example.com:8080',
      skip_tls_verification: false,
      needs_tls_fingerprint: false
    }
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

### Page Actions

Perform actions on the page before scraping:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/lazy-load-page',
    options: {
      js_rendering: true  // Required for actions
    },
    actions: [
      {
        type: 'wait',
        milliseconds: 1000
      },
      {
        type: 'scroll',
        direction: 'down'
      },
      {
        type: 'wait',
        milliseconds: 2000
      },
      {
        type: 'click',
        selector: '.load-more-button'
      },
      {
        type: 'wait',
        milliseconds: 3000
      },
      {
        type: 'scroll',
        direction: 'down'
      },
      {
        type: 'input',
        selector: '#search-input',
        text: 'search query'
      },
      {
        type: 'screenshot',
        full_page: true
      }
    ]
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
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

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: 'https://example.com/article',
    sync_wait_ms: 10000  // Wait up to 10 seconds
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data.task.result);
// Results available immediately
```

**Best Practices:**
- Use `sync_wait_ms` only when you need immediate results
- Recommended max: 5000ms for most cases
- Maximum: 30000ms
- For long-running tasks, use webhooks instead

---

## Crawling

### Basic Crawling

Crawl a website starting from a URL:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/crawl', {
    url: 'https://example.com/blog',
    max_depth: 2,
    max_pages: 100
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "id": "550e8400-e29b-41d4-a716-446655440000",
//   "url": "https://example.com/blog",
//   "credits_used": 50
// }
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
```javascript
const response = await axios.post('https://api.crawlrs.com/v1/crawl', {
    url: 'https://example.com',
    max_depth: 3,  // Crawl 3 levels deep
    max_pages: 500  // Stop after 500 pages
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

### URL Patterns

Control which URLs to crawl:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/crawl', {
    url: 'https://example.com',
    follow_links: true,
    include_patterns: ['/blog/.*', '/articles/.*'],
    exclude_patterns: ['/admin/.*', '/login/.*', '/api/.*']
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

**Pattern Examples:**
- `/blog/.*` - Include all blog posts
- `/category/[a-z]+` - Include category pages
- `\.pdf$` - Include PDF files
- `/admin/.*` - Exclude admin pages
- `\\?.*` - Exclude URLs with query strings

### Crawl Progress

Track crawl progress:

```javascript
let crawlId = '550e8400-e29b-41d4-a716-446655440000';

// Poll for status
setInterval(async () => {
  const response = await axios.get(`https://api.crawlrs.com/v1/crawl/${crawlId}`, {
      headers: {
        'Authorization': 'Bearer YOUR_API_KEY'
      }
    }
  );

  const status = response.data.task.status;
  const progress = response.data.task.progress;

  console.log(`Status: ${status}`);
  console.log(`Pages: ${progress.pages_processed}/${progress.total_pages}`);

  if (status === 'completed' || status === 'failed') {
    clearInterval(interval);
    // Get results
    await getResults(crawlId);
  }
}, 2000);  // Check every 2 seconds
```

### Get Crawl Results

Retrieve all pages crawled:

```javascript
const getResults = async (crawlId) => {
  const response = await axios.get(`https://api.crawlrs.com/v1/crawl/${crawlId}/results`, {
      headers: {
        'Authorization': 'Bearer YOUR_API_KEY'
      },
      params: {
        page: 1,
        limit: 20
      }
    }
  );

  response.data.results.forEach(result => {
    console.log(`URL: ${result.url}`);
    console.log(`Markdown: ${result.markdown.substring(0, 100)}...`);
  });

  return response.data.pagination;
};

// Pagination
const pagination = await getResults(crawlId);
console.log(`Total: ${pagination.total}`);
console.log(`Pages: ${Math.ceil(pagination.total / pagination.limit)}`);
```

### Cancel a Crawl

Stop a running crawl:

```javascript
const response = await axios.delete(`https://api.crawlrs.com/v1/crawl/${crawlId}`, {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "message": "Crawl task cancelled"
// }
```

---

## Searching

### Basic Search

Search using Google:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/search', {
    engine: 'google',
    query: 'Rust web scraping tutorial'
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "id": "550e8400-e29b-41d4-a716-446655440000",
//   "engine": "google",
//   "query": "Rust web scraping tutorial",
//   "credits_used": 5
// }
```

### Search Engines

Available search engines:

| Engine | Code | Best For |
|--------|-------|-----------|
| Google | `google` | General web search |
| Bing | `bing` | Microsoft ecosystem |
| Baidu | `baidu` | Chinese content |
| Sogou | `sogou` | Chinese content |

**Example:**
```javascript
const engines = ['google', 'bing', 'baidu', 'sogou'];

for (const engine of engines) {
  const response = await axios.post(
    'https://api.crawlrs.com/v1/search',
    {
      engine: engine,
      query: 'your search query'
    },
    {
      headers: {
        'Authorization': 'Bearer YOUR_API_KEY'
      }
    }
  );

  console.log(`${engine} results:`, response.data.results);
}
```

### Search Options

Customize your search:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/search', {
    engine: 'google',
    query: 'Rust programming',
    num_results: 20,        // Number of results (max 100)
    language: 'en',           // Language
    region: 'us',             // Region
    safe_search: false,        // Safe search
    sync_wait_ms: 5000        // Wait for completion
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
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

Retrieve search results:

```javascript
const searchId = '550e8400-e29b-41d4-a716-446655440000';

// Option 1: Synchronous (wait for completion)
const response = await axios.post('https://api.crawlrs.com/v1/search', {
    engine: 'google',
    query: 'Rust tutorial',
    sync_wait_ms: 5000
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data.results);
// [
//   {
//     "title": "Rust Programming Tutorial",
//     "url": "https://example.com/rust-tutorial",
//     "snippet": "Learn Rust programming...",
//     "engine": "google"
//   },
//   ...
// ]

// Option 2: Asynchronous (use webhook)
const response = await axios.post('https://api.crawlrs.com/v1/search', {
    engine: 'google',
    query: 'Rust tutorial',
    webhook: 'https://your-server.com/callback'
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data.id);
// Results will be sent to your webhook
```

---

## Data Extraction

### Extract from HTML

Parse structured data from HTML:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/extract', {
    html: '<html><body><h1>Title</h1><p>Content</p></body></html>',
    extraction_rules: {
      title: {
        selector: 'h1',
        attribute: 'text'
      },
      content: {
        selector: 'p',
        attribute: 'text'
      }
    }
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "data": {
//     "title": "Title",
//     "content": "Content"
//   }
// }
```

### Advanced Extraction

Extract multiple elements:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/extract', {
    html: htmlContent,
    extraction_rules: {
      headings: {
        selector: 'h1, h2, h3',
        attribute: 'text',
        multiple: true
      },
      links: {
        selector: 'a',
        attribute: 'href',
        multiple: true
      },
      images: {
        selector: 'img',
        attribute: 'src',
        multiple: true
      },
      meta_description: {
        selector: 'meta[name="description"]',
        attribute: 'content'
      },
      meta_keywords: {
        selector: 'meta[name="keywords"]',
        attribute: 'content'
      }
    }
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "data": {
//     "headings": ["Main Title", "Section 1", "Section 2"],
//     "links": ["/page1", "/page2", "/page3"],
//     "images": ["/img1.jpg", "/img2.jpg"],
//     "meta_description": "Page description...",
//     "meta_keywords": "keyword1, keyword2, keyword3"
//   }
// }
```

### Nested Extraction

Extract data from structured HTML:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/extract', {
    html: `
      <div class="product">
        <h1 class="name">Product Name</h1>
        <div class="price">$99.99</div>
        <ul class="features">
          <li>Feature 1</li>
          <li>Feature 2</li>
        </ul>
      </div>
    `,
    extraction_rules: {
      productName: {
        selector: '.name',
        attribute: 'text'
      },
      productPrice: {
        selector: '.price',
        attribute: 'text'
      },
      features: {
        selector: '.features li',
        attribute: 'text',
        multiple: true
      }
    }
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "data": {
//     "productName": "Product Name",
//     "productPrice": "$99.99",
//     "features": ["Feature 1", "Feature 2"]
//   }
// }
```

---

## Webhooks

### Setup Webhooks

Receive notifications when tasks complete:

```javascript
const response = await axios.post('https://api.crawlrs.com/v1/webhooks', {
    url: 'https://your-server.com/webhook',
    events: ['task.completed', 'task.failed'],
    secret: 'your-webhook-secret',
    active: true
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);

console.log(response.data);
// {
//   "success": true,
//   "webhook": {
//     "id": "550e8400-e29b-41d4-a716-446655440000",
//     "url": "https://your-server.com/webhook",
//     "events": ["task.completed", "task.failed"],
//     "active": true
//   }
// }
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

**Example:**
```javascript
const response = await axios.post('https://api.crawlrs.com/v1/webhooks', {
    url: 'https://your-server.com/webhook',
    events: ['task.created', 'task.started', 'task.completed', 'task.failed']
  },
  {
    headers: {
      'Authorization': 'Bearer YOUR_API_KEY'
    }
  }
);
```

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

  // Verify signature
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

  // Process task result
  if (webhook.event === 'task.completed') {
    const result = webhook.result;
    // Process result...
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

If your webhook endpoint returns an error:

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
    console.log(`Rate limited. Retry after ${retryAfter} seconds`);

    // Wait and retry
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

// Validate before making request
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
- Never request internal URLs
- Always validate URLs before sending
- Use the whitelist approach

**4. Insufficient Credits**

```json
{
  "success": false,
  "error": "Insufficient credits for this operation"
}
```

**Solution:**
```javascript
// Check credits before making request
const response = await axios.get('https://api.crawlrs.com/v1/credits', {
    headers: {
      'Authorization': `Bearer ${API_KEY}`
    }
  }
);
if (response.data.credits < requiredCredits) {

if (creditsResponse.data.credits < requiredCredits) {
  console.error('Insufficient credits');
  // Prompt user to upgrade or wait for renewal
  return;
}
```

### Error Handling Best Practices

1. **Always Check `success` Field**
   ```javascript
   const response = await makeRequest();
   if (!response.data.success) {
     console.error('API error:', response.data.error);
     // Handle error
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
         const delay = Math.pow(2, i) * 1000;  // 1s, 2s, 4s
         await new Promise(resolve => setTimeout(resolve, delay));
       }
     }
   }
   }
   ```

3. **Log Errors with Context**
   ```javascript
   try {
     const response = await axios.post(url, data);
   } catch (error) {
     console.error({
       timestamp: new Date().toISOString(),
       endpoint: url,
       error: error.message,
       status: error.response?.status,
       data: error.response?.data
     });
   }
   ```

4. **Implement Circuit Breaker**
   ```javascript
   class CircuitBreaker {
     constructor(threshold = 5) {
       this.failureCount = 0;
       this.threshold = threshold;
       this.state = 'closed';
       this.nextAttempt = Date.now();
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
         this.nextAttempt = Date.now() + 60000;  // Retry after 1 minute
       }
     }
   }
   ```

---

## Best Practices

### 1. Use Async Mode for Long-Running Tasks

**Don't:**
```javascript
// Bad: Long wait times
const response = await axios.post('https://api.crawlrs.com/v1/crawl', {
  url: 'https://example.com',
  sync_wait_ms: 30000  // Wait 30 seconds
});
```

**Do:**
```javascript
// Good: Use webhook for async processing
const response = await axios.post('https://api.crawlrs.com/v1/crawl', {
  url: 'https://example.com',
  webhook: 'https://your-server.com/callback'
});

// Process results when webhook is called
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
      // Don't retry on 4xx errors (except 429)
      if (error.response?.status >= 400 && error.response?.status < 500 && error.response?.status !== 429) {
        throw error;
      }

      // Backoff before retry
      const delay = Math.pow(2, i) * 1000;
      await new Promise(resolve => setTimeout(resolve, delay));

      console.log(`Retry ${i + 1}/${retries} after ${delay}ms`);
    }
  }
}
```

### 3. Cache Results When Appropriate

```javascript
const cache = new Map();

async function scrapeWithCache(url) {
  const cacheKey = `scrape:${url}`;

  // Check cache
  if (cache.has(cacheKey)) {
    console.log('Cache hit');
    return cache.get(cacheKey);
  }

  // Make request
  const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
    url: url,
    formats: ['markdown']
  }, {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });

  // Cache result
  cache.set(cacheKey, response.data);
  return response.data;
}
```

### 4. Use Specific Output Formats

**Don't:**
```javascript
// Bad: Request all formats when not needed
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
  url: 'https://example.com',
  formats: ['html', 'markdown', 'text', 'json']  // All formats
});
```

**Do:**
```javascript
// Good: Request only what you need
const response = await axios.post('https://api.crawlrs.com/v1/scrape', {
  url: 'https://example.com',
  formats: ['markdown']  // Only markdown
});
```

### 5. Monitor Your Usage

```javascript
// Check usage regularly
async function monitorUsage() {
  const response = await axios.get('https://api.crawlrs.com/v1/usage', {
    headers: { 'Authorization': `Bearer ${API_KEY}` }
  });

  const { credits_used, credits_remaining, requests_today, rate_limit_info } = response.data;

  console.log(`Credits used: ${credits_used}/${credits_used + credits_remaining}`);
  console.log(`Requests today: ${requests_today}`);
  console.log(`Rate limit: ${rate_limit_info.remaining}/${rate_limit_info.limit}`);

  // Alert if running low
  if (credits_remaining < 100) {
    console.warn('Low credits remaining!');
  }
}

// Run every hour
setInterval(monitorUsage, 3600000);
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

// Use before making request
try {
  validateScrapeRequest(scrapeRequest);
  await axios.post('/v1/scrape', scrapeRequest);
} catch (error) {
  console.error('Validation failed:', error.message);
}
```

### 7. Use Environment Variables for Secrets

**Never commit API keys:**

```bash
# .env file
CRAWLRS_API_KEY=crawlrs_sk_abc123def456...
CRAWLRS_WEBHOOK_SECRET=my-secret-key
```

```javascript
// Load environment variables
require('dotenv').config();

const API_KEY = process.env.CRAWLRS_API_KEY;
const WEBHOOK_SECRET = process.env.CRAWLRS_WEBHOOK_SECRET;

// Use in requests
const response = await axios.post('https://api.crawlrs.com/v1/scrape', data, {
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

**Choose the right engine for your needs:**

| Scenario | Recommended Engine | Reason |
|----------|-------------------|---------|
| Static HTML pages | Reqwest | Fastest, lowest cost |
| JavaScript-heavy SPAs | Playwright | Renders JS |
| Anti-bot protection | Playwright/Fire | Bypasses detection |
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
- Consider upgrading your plan for higher limits
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
- Try with a different engine (Playwright vs Reqwest)
- Check your webhook endpoint is responding
- Contact support if issue persists

**4. Webhook not being called**

**Problem:** Tasks complete but webhook not triggered

**Solutions:**
- Verify webhook URL is publicly accessible
- Check your server logs for incoming requests
- Verify webhook signature is being sent correctly
- Test webhook endpoint manually:
  ```bash
  curl -X POST https://your-server.com/webhook \
    -H 'Content-Type: application/json' \
    -d '{"event":"test"}'
  ```
- Check webhook status in dashboard

**5. Slow response times**

**Problem:** API requests take longer than expected

**Solutions:**
- Use synchronous mode only when necessary
- Reduce payload size (fewer formats, smaller sync_wait_ms)
- Use caching for repeated requests
- Check your network latency
- Consider geographic distribution

**6. "Insufficient credits"**

**Problem:** Cannot create new tasks

**Solutions:**
- Check your current credit usage in dashboard
- Upgrade your plan for more credits
- Wait for monthly credit renewal
- Optimize your requests to use fewer credits

---

## FAQ

### General Questions

**Q: What is the difference between scrape and crawl?**

A: **Scrape** extracts content from a single page. **Crawl** automatically discovers and scrapes multiple linked pages starting from a URL.

**Q: How much does it cost?**

A: Usage is measured in credits. Each operation consumes credits:
- Scrape: 5-20 credits (based on options)
- Crawl: 10-50 credits per page
- Search: 2-5 credits per query

**Q: Can I scrape any website?**

A: Most websites, but we respect robots.txt and may block sites that explicitly prohibit scraping. Always check a website's Terms of Service.

**Q: What happens if a task fails?**

A: Failed tasks are logged and you receive a webhook notification (if configured). You can retry the task with the same parameters.

### Technical Questions

**Q: How do I handle JavaScript-rendered content?**

A: Use Playwright engine by setting `js_rendering: true` in options:
```javascript
{
  options: {
    js_rendering: true
  }
}
```

**Q: Can I use a proxy?**

A: Yes, specify a proxy in options:
```javascript
{
  options: {
    proxy: 'http://user:pass@proxy.example.com:8080'
  }
}
```

**Q: How do I extract data from multiple elements?**

A: Use `multiple: true` in extraction rules:
```javascript
{
  extraction_rules: {
    links: {
      selector: 'a',
      attribute: 'href',
      multiple: true
    }
  }
}
```

**Q: What is the maximum sync_wait_ms?**

A: Maximum is 30,000 milliseconds (30 seconds). For longer tasks, use webhooks for async processing.

### Billing Questions

**Q: How are credits calculated?**

A: Credits are deducted when tasks complete. If a task fails, credits are refunded.

**Q: Do unused credits roll over?**

A: Yes, unused credits from your billing period roll over to the next period.

**Q: Can I set a credit usage limit?**

A: Yes, you can set a monthly cap in your dashboard to prevent overages.

---

## Support

### Getting Help

- 📖 [Documentation](/)
- 📚 [API Reference](docs/API_REFERENCE.md)
- 🏗️ [Architecture](docs/ARCHITECTURE.md)
- 🐛 [Issue Tracker](https://github.com/your-org/crawlrs/issues)
- 💬 [Discord Community](https://discord.gg/your-server)
- 📧 Email: Kirky-X@outlook.com

### Reporting Bugs

When reporting bugs, include:
1. API endpoint used
2. Request parameters (sanitized)
3. Expected vs actual behavior
4. Error messages
5. Reproduction steps

### Feature Requests

We love feature requests! Submit them via:
- GitHub Issues (with "enhancement" label)
- Discord Community channel
- Email to support

---

**Happy Scraping! 🚀**
