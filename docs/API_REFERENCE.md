
# 📡 Complete REST API Documentation

<div align="center">

![API Version](https://img.shields.io/badge/api-0.2.0-blue)
![Base URL](https://img.shields.io/badge/base%20URL-http://localhost:8899-green)
![License](https://img.shields.io/badge/license-Apache%202.0-orange)

**Version:** 0.2.0 | **Base URL:** `http://localhost:8899` | **Updated:** 2025-07-21

</div>

---

## 📖 Table of Contents

- [Authentication](#authentication)
- [Common Response Format](#common-response-format)
- [Errors](#errors)
- [Public Endpoints](#public-endpoints)
  - [Health Check](#health-check)
  - [Get Version](#get-version)
  - [Get Metrics](#get-metrics)
- [Protected Endpoints](#protected-endpoints)
  - [Scrape API](#scrape-api)
  - [Crawl API](#crawl-api)
  - [Search API](#search-api)
  - [Extract API](#extract-api)
  - [Task API](#task-api)
  - [Team API](#team-api)
  - [Webhook API](#webhook-api)
  - [Audit API](#audit-api)
- [Rate Limiting](#rate-limiting)
- [Webhooks](#webhooks)
- [SDK API](#sdk-api)
- [SDK Examples](#sdk-examples)
- [Best Practices](#best-practices)
- [Changelog](#changelog)
- [Support](#support)

---

## Authentication

All protected endpoints require authentication using an API key in the `Authorization` header:

```http
Authorization: Bearer YOUR_API_KEY
```

### API Key Scopes

API keys can have different scopes that control access to specific features:

| Scope | Description |
|--------|-------------|
| `scrape` | Access to scrape endpoints |
| `crawl` | Access to crawl endpoints |
| `search` | Access to search endpoints |
| `extract` | Access to extract endpoints |
| `admin` | Full administrative access |

---

## Common Response Format

All API responses follow this unified structure:

### Success Response

```json
{
  "success": true,
  "data": {
    // Response data here
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Success Response with Pagination

```json
{
  "success": true,
  "data": {
    // Response data here
  },
  "meta": {
    "page": 1,
    "per_page": 20,
    "total_items": 100,
    "total_pages": 5,
    "has_next": true,
    "has_previous": false
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Error Response

```json
{
  "success": false,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Detailed error message"
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Rate Limit Error Response

```json
{
  "success": false,
  "error": {
    "code": "RATE_LIMITED",
    "message": "Rate limit exceeded"
  },
  "retry_after_seconds": 60,
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `success` | boolean | Whether the request was successful |
| `data` | object | Response data (only present on success) |
| `error` | object | Error details (only present on failure) |
| `error.code` | string | Error code for programmatic handling |
| `error.message` | string | Human-readable error message |
| `meta` | object | Pagination metadata (only for list responses) |
| `timestamp` | string | Response timestamp in RFC3339 format |

---

## Errors

### HTTP Status Codes

| Code | Description |
|------|-------------|
| 200 | Success |
| 201 | Created |
| 400 | Bad Request - Invalid parameters |
| 401 | Unauthorized - Missing or invalid API key |
| 403 | Forbidden - Insufficient permissions |
| 429 | Too Many Requests - Rate limit exceeded |
| 422 | Unprocessable Entity - Validation error |
| 500 | Internal Server Error |

### Error Codes

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `VALIDATION_ERROR` | 400 | Invalid request parameters |
| `NOT_FOUND` | 404 | Resource not found |
| `UNAUTHORIZED` | 401 | Missing or invalid API key |
| `FORBIDDEN` | 403 | Insufficient permissions |
| `RATE_LIMITED` | 429 | Rate limit exceeded |
| `CONFLICT` | 409 | Resource conflict |
| `PRECONDITION_FAILED` | 412 | Precondition failed |
| `UNPROCESSABLE_ENTITY` | 422 | Validation error |
| `INTERNAL_ERROR` | 500 | Internal server error |
| `SERVICE_UNAVAILABLE` | 503 | Service unavailable |
| `DATABASE_ERROR` | 500 | Database error |
| `CACHE_ERROR` | 500 | Cache error |
| `EXTERNAL_SERVICE_ERROR` | 502 | External service error |
| `TIMEOUT` | 504 | Request timeout |
| `QUOTA_EXCEEDED` | 402 | Quota exceeded |
| `FEATURE_DISABLED` | 403 | Feature not enabled |

---

## Public Endpoints

### Health Check

Check if the API is running.

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "healthy"
}
```

### Get Version

Get the current API version.

**Endpoint:** `GET /v1/version`

**Response:**
```text
0.2.0
```

### Get Metrics

Get system performance metrics (requires `metrics` feature).

**Endpoint:** `GET /metrics`

**Response:**
```text
# Prometheus metrics format
api_requests_total{method="POST",endpoint="/v1/scrape"} 1234
api_request_duration_seconds{method="POST",endpoint="/v1/scrape",quantile="0.5"} 0.045
```

---

## Protected Endpoints

### Scrape API

Scrape a single web page.

#### Create Scrape Task

**Endpoint:** `POST /v1/scrape`

**Request Body:**
```json
{
  "url": "https://example.com",
  "formats": ["markdown", "html"],
  "include_tags": ["h1", "h2", "p"],
  "exclude_tags": ["script", "style"],
  "webhook": "https://your-webhook.com/callback",
  "extraction_rules": {
    "title": {
      "selector": "h1",
      "attribute": "text"
    }
  },
  "actions": [
    {
      "type": "wait",
      "milliseconds": 1000
    },
    {
      "type": "click",
      "selector": ".load-more"
    }
  ],
  "options": {
    "headers": {
      "User-Agent": "Mozilla/5.0..."
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
    "skip_tls_verification": false,
    "needs_tls_fingerprint": false,
    "use_fire_engine": false
  },
  "metadata": {
    "custom_key": "custom_value"
  },
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Target URL (http/https only) |
| `formats` | array | No | Output formats: `markdown`, `html`, `text` |
| `include_tags` | array | No | HTML tags to include in output |
| `exclude_tags` | array | No | HTML tags to exclude from output |
| `webhook` | string | No | Webhook URL for completion notification |
| `extraction_rules` | object | No | CSS selector extraction rules |
| `actions` | array | No | Page interaction actions |
| `options` | object | No | Scraping options |
| `metadata` | object | No | Custom metadata for the task |
| `sync_wait_ms` | integer | No | Wait time for synchronous response (max 30000) |

**Action Types:**

| Type | Parameters | Description |
|-------|-----------|-------------|
| `wait` | `milliseconds` | Wait for specified time |
| `click` | `selector` | Click element matching selector |
| `scroll` | `direction` | Scroll page (up/down) |
| `screenshot` | `full_page` | Take screenshot |
| `input` | `selector`, `text` | Input text into element |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com",
    "credits_used": 10
  },
  "timestamp": "2025-01-15T12:00:00+00:00"
}
```

#### Get Scrape Status

**Endpoint:** `GET /v1/scrape/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "task": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "status": "completed",
      "url": "https://example.com",
      "result": {
        "html": "...",
        "markdown": "...",
        "text": "..."
      }
    }
  }
}
```

#### Cancel Scrape

**Endpoint:** `POST /v1/scrape/{id}/_cancel`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Scrape task cancelled"
  }
}
```

---

### Crawl API

Crawl multiple pages from a starting URL.

#### Create Crawl Task

**Endpoint:** `POST /v1/crawl`

**Request Body:**
```json
{
  "url": "https://example.com",
  "max_depth": 2,
  "max_pages": 100,
  "follow_links": true,
  "include_patterns": ["/blog/.*"],
  "exclude_patterns": ["/admin/.*"],
  "formats": ["markdown"],
  "webhook": "https://your-webhook.com/callback",
  "options": {
    "timeout": 30,
    "js_rendering": false,
    "proxy": "http://proxy.example.com:8080"
  },
  "sync_wait_ms": 10000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Starting URL |
| `max_depth` | integer | No | Maximum crawl depth (default: 1) |
| `max_pages` | integer | No | Maximum pages to crawl |
| `follow_links` | boolean | No | Follow links on pages (default: true) |
| `include_patterns` | array | No | Regex patterns for URLs to include |
| `exclude_patterns` | array | No | Regex patterns for URLs to exclude |
| `formats` | array | No | Output formats |
| `webhook` | string | No | Webhook URL for notifications |
| `options` | object | No | Scraping options |
| `sync_wait_ms` | integer | No | Wait time for synchronous response |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "url": "https://example.com",
    "credits_used": 50
  }
}
```

#### Get Crawl Status

**Endpoint:** `GET /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "task": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "status": "running",
      "url": "https://example.com",
      "progress": {
        "pages_processed": 45,
        "total_pages": 100
      }
    }
  }
}
```

#### Get Crawl Results

**Endpoint:** `GET /v1/crawl/{id}/results`

**Parameters:**
- `id` (path) - Task UUID

**Query Parameters:**
- `page` - Page number (default: 1)
- `limit` - Results per page (default: 20, max: 100)

**Response:**
```json
{
  "success": true,
  "data": {
    "results": [
      {
        "url": "https://example.com/page1",
        "html": "...",
        "markdown": "..."
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 100
    }
  }
}
```

#### Cancel Crawl

Cancel a crawl task. Supports both POST and DELETE methods.

**Endpoint:** `POST /v1/crawl/{id}/_cancel`

**Endpoint:** `DELETE /v1/crawl/{id}`

**Parameters:**
- `id` (path) - Task UUID

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Crawl task cancelled"
  }
}
```

---

### Search API

Search using various search engines.

#### Search

**Endpoint:** `POST /v1/search`

**Request Body:**
```json
{
  "engine": "google",
  "query": "Rust web scraping",
  "num_results": 10,
  "language": "en",
  "region": "us",
  "safe_search": false,
  "webhook": "https://your-webhook.com/callback",
  "sync_wait_ms": 5000
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `engine` | string | Yes | Search engine: `google`, `bing`, `baidu`, `sogou` |
| `query` | string | Yes | Search query |
| `limit` | integer | No | Number of results (default: 10, max: 100) |
| `lang` | string | No | Search language (default: `en`) |
| `country` | string | No | Search region (default: `us`) |
| `safe_search` | boolean | No | Enable safe search (default: false) |
| `webhook` | string | No | Webhook URL for notifications |
| `sync_wait_ms` | integer | No | Wait time for synchronous response |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "query": "Rust web scraping",
    "results": [
      {
        "title": "Web Scraping with Rust",
        "url": "https://example.com/rust-scraping",
        "description": "Learn how to scrape websites using Rust",
        "engine": "google"
      }
    ],
    "crawl_id": "550e8400-e29b-41d4-a716-446655440000",
    "credits_used": 5
  }
}
```

---

### Extract API

Extract structured data from HTML.

#### Extract Data

**Endpoint:** `POST /v1/extract`

**Request Body:**
```json
{
  "html": "<html>...</html>",
  "extraction_rules": {
    "title": {
      "selector": "h1",
      "attribute": "text"
    },
    "links": {
      "selector": "a",
      "attribute": "href",
      "multiple": true
    }
  },
  "options": {
    "return_html": true
  }
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `html` | string | Yes | HTML content to extract from |
| `extraction_rules` | object | Yes | CSS selector extraction rules |
| `options` | object | No | Extraction options |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "title": "Example Page",
    "links": [
      "https://example.com/page1",
      "https://example.com/page2"
    ]
  }
}
```

---

### Task API

Query and manage tasks. Task API follows RESTful conventions with action suffixes (`_query` for queries, `_cancel` for cancellations).

#### Query Tasks

**Endpoint:** `POST /v1/tasks/_query`

**Request Body:**
```json
{
  "filters": {
    "status": ["completed", "running"],
    "type": ["scrape", "crawl", "extract"],
    "created_after": "2025-01-01T00:00:00Z",
    "created_before": "2025-01-15T00:00:00Z"
  },
  "pagination": {
    "page": 1,
    "limit": 20
  },
  "sort": {
    "field": "created_at",
    "order": "desc"
  }
}
```

**Filter Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `status` | array | Filter by status: `pending`, `running`, `completed`, `failed`, `cancelled` |
| `type` | array | Filter by type: `scrape`, `crawl`, `extract` |
| `created_after` | string | Filter by creation date (RFC3339) |
| `created_before` | string | Filter by creation date (RFC3339) |

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "tasks": [...],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 150
    }
  }
}
```

#### Cancel Tasks

**Endpoint:** `POST /v1/tasks/_cancel`

**Request Body:**
```json
{
  "task_ids": [
    "550e8400-e29b-41d4-a716-446655440000",
    "660e8400-e29b-41d4-a716-446655440001"
  ]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "cancelled_count": 2
  }
}
```

---

### Team API

#### Get Current Team

**Endpoint:** `GET /v1/teams/me`

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "770e8400-e29b-41d4-a716-446655440000",
    "name": "My Team",
    "created_at": "2025-01-01T00:00:00Z"
  }
}
```

#### Get Team Usage

**Endpoint:** `GET /v1/teams/me/usage`

**Response:**
```json
{
  "success": true,
  "data": {
    "credits_used": 1234,
    "credits_limit": 10000,
    "requests_today": 42,
    "requests_limit": 1000
  }
}
```

#### Get Team Geo Restrictions

**Endpoint:** `GET /v1/teams/geo-restrictions`

**Response:**
```json
{
  "success": true,
  "data": {
    "restrictions": {
      "allowed_countries": ["US", "UK", "CA"],
      "blocked_countries": ["CN", "RU"],
      "enabled": true
    }
  }
}
```

#### Update Team Geo Restrictions

**Endpoint:** `PUT /v1/teams/geo-restrictions`

**Request Body:**
```json
{
  "allowed_countries": ["US", "UK", "CA"],
  "blocked_countries": ["CN", "RU"],
  "enabled": true
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "message": "Geo restrictions updated"
  }
}
```

---

### Webhook API

#### List Webhooks

**Endpoint:** `GET /v1/webhooks`

**Query Parameters:**
- `page` - Page number (default: 1)
- `limit` - Results per page (default: 20, max: 100)

**Response:**
```json
{
  "success": true,
  "data": {
    "webhooks": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "url": "https://your-webhook.com/callback",
        "events": ["task.completed", "task.failed"],
        "active": true,
        "created_at": "2025-01-15T00:00:00Z"
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 5
    }
  }
}
```

#### Create Webhook

**Endpoint:** `POST /v1/webhooks`

**Request Body:**
```json
{
  "url": "https://your-webhook.com/callback",
  "events": ["task.completed", "task.failed"],
  "secret": "your-webhook-secret",
  "active": true
}
```

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|-------|----------|-------------|
| `url` | string | Yes | Webhook URL |
| `events` | array | Yes | Events to subscribe to |
| `secret` | string | No | Secret for webhook signature |
| `active` | boolean | No | Enable/disable webhook |

**Events:**
- `task.created` - Task created
- `task.started` - Task started
- `task.completed` - Task completed
- `task.failed` - Task failed
- `task.cancelled` - Task cancelled

**Response (Success):**
```json
{
  "success": true,
  "data": {
    "webhook": {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "url": "https://your-webhook.com/callback",
      "events": ["task.completed", "task.failed"],
      "active": true
    }
  }
}
```

---

### Audit API

#### Get Audit Logs

**Endpoint:** `GET /v1/audit/logs`

**Query Parameters:**
- `event_type` - Filter by event type
- `start_time` - Start timestamp
- `end_time` - End timestamp
- `page` - Page number
- `limit` - Results per page

**Response:**
```json
{
  "success": true,
  "data": {
    "logs": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "event_type": "api_request",
        "timestamp": "2025-01-15T00:00:00Z",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "endpoint": "/v1/scrape",
        "ip_address": "192.168.1.1",
        "user_agent": "Mozilla/5.0..."
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 500
    }
  }
}
```

#### Get Denied Requests

**Endpoint:** `GET /v1/audit/denied`

**Query Parameters:**
- `reason` - Filter by denial reason
- `start_time` - Start timestamp
- `end_time` - End timestamp
- `page` - Page number
- `limit` - Results per page

**Response:**
```json
{
  "success": true,
  "data": {
    "denied_requests": [
      {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "timestamp": "2025-01-15T00:00:00Z",
        "reason": "rate_limit_exceeded",
        "api_key_id": "770e8400-e29b-41d4-a716-446655440000",
        "endpoint": "/v1/scrape",
        "ip_address": "192.168.1.1"
      }
    ],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 100
    }
  }
}
```

---

## Rate Limiting

The API implements rate limiting at multiple levels:

1. **Per-API Key Rate Limit** - Limits requests per API key
2. **Per-Team Concurrency Limit** - Limits concurrent requests per team
3. **Global Rate Limit** - System-wide protection

### Rate Limit Headers

Rate limit information is included in response headers:

```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1705315200
```

---

## Webhooks

Webhooks allow you to receive notifications about task events.

### Webhook Payload Format

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
    "markdown": "..."
  }
}
```

### Webhook Signature

If a secret is provided, the webhook includes an `X-Webhook-Signature` header:

```
X-Webhook-Signature: sha256=hexdigest
```

Verify the signature by computing HMAC SHA256 of the payload using your secret.

---

## SDK API

SDK endpoints provide simplified interfaces for common operations, wrapping the underlying REST API.

### SDK Search

**Endpoint:** `POST /api/v1/sdk/search`

**Request Body:**
```json
{
  "query": "Rust web scraping",
  "engine": "google",
  "num_results": 10
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "results": [...],
    "credits_used": 5
  }
}
```

### SDK Tasks

**Endpoint:** `POST /api/v1/sdk/tasks`

**Request Body:**
```json
{
  "filters": {
    "status": ["completed"],
    "type": ["scrape", "crawl"]
  },
  "pagination": {
    "page": 1,
    "limit": 20
  }
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "tasks": [...],
    "pagination": {
      "page": 1,
      "limit": 20,
      "total": 150
    }
  }
}
```

### SDK Scrape

**Endpoint:** `POST /api/v1/sdk/scrape`

**Request Body:**
```json
{
  "url": "https://example.com",
  "formats": ["markdown"],
  "sync_wait_ms": 5000
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "completed",
    "result": {
      "markdown": "..."
    }
  }
}
```

### SDK Crawl

**Endpoint:** `POST /api/v1/sdk/crawl`

**Request Body:**
```json
{
  "url": "https://example.com",
  "max_pages": 50,
  "max_depth": 2,
  "formats": ["markdown"]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "running"
  }
}
```

---

## SDK Examples

### JavaScript/Node.js

```javascript
const axios = require('axios');

const client = axios.create({
  baseURL: 'http://localhost:8899',
  headers: {
    'Authorization': 'Bearer YOUR_API_KEY'
  }
});

const scrape = async (url) => {
  const response = await client.post('/v1/scrape', {
    url: url,
    formats: ['markdown'],
    sync_wait_ms: 5000
  });
  return response.data;
};
```

### Python

```python
import requests

client = requests.Session()
client.headers.update({
  'Authorization': 'Bearer YOUR_API_KEY'
})

def scrape(url):
  response = client.post('http://localhost:8899/v1/scrape', json={
    'url': url,
    'formats': ['markdown'],
    'sync_wait_ms': 5000
  })
  return response.json()
```

### Go

```go
package main

import (
  "bytes"
  "encoding/json"
  "net/http"
)

func Scrape(url string) error {
  client := &http.Client{}
  body := map[string]interface{}{
    "url": url,
    "formats": []string{"markdown"},
    "sync_wait_ms": 5000,
  }

  jsonData, _ := json.Marshal(body)
  req, _ := http.NewRequest("POST", "http://localhost:8899/v1/scrape", bytes.NewBuffer(jsonData))
  req.Header.Set("Authorization", "Bearer YOUR_API_KEY")
  req.Header.Set("Content-Type", "application/json")

  resp, err := client.Do(req)
  return err
}
```

---

## Best Practices

1. **Use Sync Mode Sparingly** - Only use `sync_wait_ms` when you need immediate results
2. **Implement Retry Logic** - Handle rate limits with exponential backoff
3. **Use Webhooks** - Prefer webhooks over polling for task status
4. **Set Timeouts** - Always configure appropriate timeouts
5. **Monitor Credits** - Track credit usage to avoid service interruption
6. **Handle Errors Gracefully** - Check both HTTP status and response `success` field
7. **Validate Inputs** - Validate URLs and parameters before sending requests
8. **Use Caching** - Enable oxcache for frequently accessed content
9. **Set Proper Rates** - Configure rate limits appropriate for your capacity
10. **Secure Webhooks** - Always verify webhook signatures

---

## Changelog

### v0.2.0 (2025-07-21)
- Added `GET /v1/webhooks` to list webhooks
- Added `GET /v1/teams/me` and `GET /v1/teams/me/usage` team endpoints
- Added SDK API section (`/api/v1/sdk/*` endpoints)
- Added `POST /v1/crawl/{id}/_cancel` cancel endpoint (alongside existing `DELETE /v1/crawl/{id}`)
- Merged Task API sections into single unified section
- Removed duplicate table of contents
- Updated base URL to localhost:8899

### v0.1.0 (2025-01-15)
- Initial release
- Scrape, Crawl, Search, Extract APIs
- Rate limiting and concurrency control
- Webhook support
- Audit logging
- Metrics export

---

## Support

For questions or issues:
- 📖 [Documentation](/)
- 🐛 [Issue Tracker](https://github.com/your-org/crawlrs/issues)
- 📧 Email: Kirky-X@outlook.com
