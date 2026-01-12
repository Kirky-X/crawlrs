# crawlrs 全流程测试计划

## 文档信息

- **文档版本**: v1.0.0
- **测试周期**: Week 1-4
- **最近更新**: 2025-01-12
- **基于文档**: PRD v2.1.0, TEST v2.1.0, UAT v2.1.0

---

## 1. 测试概述

### 1.1 测试目标

本测试计划旨在对 crawlrs 系统进行全面、系统的测试验证，确保：

- ✅ 所有核心功能符合 PRD 规格要求
- ✅ 系统性能达到 SLO 指标（吞吐量 > 10000 RPS，P95 延迟 < 50ms）
- ✅ 系统在生产环境下稳定运行
- ✅ 部署流程可重复、可验证
- ✅ 测试覆盖率 ≥ 80%

### 1.2 测试范围

| 模块 | 测试类型 | 优先级 | 状态 |
|------|---------|--------|------|
| 搜索功能 (Search) | 单元/集成/E2E/UAT | P0 | 待执行 |
| 抓取功能 (Scrape) | 单元/集成/E2E/UAT | P0 | 待执行 |
| 爬取功能 (Crawl) | 单元/集成/E2E/UAT | P0 | 待执行 |
| 提取功能 (Extract) | 单元/集成/E2E/UAT | P1 | 待执行 |
| 统一任务管理 | 集成/E2E/UAT | P0 | 待执行 |
| 并发与限流 | 集成/压力测试 | P0 | 待执行 |
| 错误处理 | 单元/集成/UAT | P1 | 待执行 |
| Webhook | 集成/UAT | P1 | 待执行 |
| 性能测试 | 压力测试 | P0 | 待执行 |
| 部署测试 | E2E/UAT | P1 | 待执行 |
| 监控测试 | E2E/UAT | P2 | 待执行 |
| 安全测试 | UAT | P1 | 待执行 |
| 稳定性测试 | 长期运行 | P1 | 待执行 |

### 1.3 测试通过标准

| 指标 | 目标值 | 最低可接受值 |
|------|--------|-------------|
| 单元测试通过率 | 100% | 95% |
| 集成测试通过率 | 100% | 95% |
| E2E 测试通过率 | 100% | 90% |
| UAT 通过率 | ≥ 95% | 90% |
| 测试覆盖率 | ≥ 80% | 75% |
| API 吞吐量 | > 10000 RPS | > 5000 RPS |
| P95 延迟 | < 50ms | < 100ms |
| P99 延迟 | < 200ms | < 500ms |
| 错误率 | < 0.1% | < 1% |
| 同步返回成功率 | > 70% | > 50% |

---

## 2. 测试环境配置

### 2.1 Docker Compose 测试栈

创建 `docker-compose.test.yml` 文件：

```yaml
version: '3.8'

services:
  # PostgreSQL 数据库（测试数据）
  postgres_test:
    image: postgres:16-alpine
    container_name: crawlrs_postgres_test
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: crawlrs_test
    ports:
      - "5433:5432"
    volumes:
      - postgres_test_data:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 5s
      timeout: 5s
      retries: 5
    command: >
      postgres
      -c max_connections=200
      -c shared_buffers=256MB
      -c effective_cache_size=512MB
      -c work_mem=64MB
      -c maintenance_work_mem=256MB
      -c checkpoint_completion_target=0.9
      -c wal_buffers=64MB
      -c max_wal_size=2GB
      -c min_wal_size=512MB

  # Redis 缓存（测试）
  redis_test:
    image: redis:7-alpine
    container_name: crawlrs_redis_test
    ports:
      - "6381:6379"
    volumes:
      - redis_test_data:/data
    command: redis-server
      --maxmemory 512mb
      --maxmemory-policy allkeys-lru
      --appendonly yes
      --appendfsync everysec
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 5s
      retries: 5

  # Chrome 浏览器（Playwright 引擎）
  chromium:
    image: browserless/chrome:latest
    container_name: crawlrs_chromium
    ports:
      - "9222:3000"
    environment:
      - CONCURRENT_LIMIT=10
      - TIMEOUT=30000
      - MAX_QUEUE_LENGTH=50
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/status"]
      interval: 10s
      timeout: 10s
      retries: 3

  # FlareSolverr（Google 搜索代理）
  flaresolverr:
    image: flaresolverr/flaresolverr:latest
    container_name: crawlrs_flaresolverr
    ports:
      - "8191:8191"
    environment:
      - TZ=UTC
      - LOG_LEVEL=info
      - CAPTCHA_SOLVER=basic
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8191"]
      interval: 30s
      timeout: 10s
      retries: 3

  # API 服务器
  api_server:
    build:
      context: .
      dockerfile: Dockerfile
    container_name: crawlrs_api_test
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=info
      - DATABASE_URL=postgres://postgres:postgres@postgres_test:5432/crawlrs_test
      - REDIS_URL=redis://redis_test:6379
      - CHROMIUM_REMOTE_DEBUGGING_URL=http://chromium:3000
      - FLARESOLVERR_URL=http://flaresolverr:8191
      - API_HOST=0.0.0.0
      - API_PORT=8080
      - WORKER_COUNT=4
      - SYNC_WAIT_DEFAULT_MS=5000
      - SYNC_WAIT_MAX_MS=30000
      - RATE_LIMIT_RPM=1000
      - TEAM_CONCURRENT_LIMIT=20
    depends_on:
      postgres_test:
        condition: service_healthy
      redis_test:
        condition: service_healthy
      chromium:
        condition: service_healthy
      flaresolverr:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 10s
      timeout: 10s
      retries: 3

  # Prometheus 指标收集
  prometheus:
    image: prom/prometheus:latest
    container_name: crawlrs_prometheus_test
    ports:
      - "9090:9090"
    volumes:
      - ./test-config/prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.enable-lifecycle'

  # Grafana 监控面板
  grafana:
    image: grafana/grafana:latest
    container_name: crawlrs_grafana_test
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_USER=admin
      - GF_SECURITY_ADMIN_PASSWORD=admin
      - GF_USERS_ALLOW_SIGN_UP=false
    volumes:
      - ./test-config/grafana/dashboards:/etc/grafana/provisioning/dashboards
      - grafana_data:/var/lib/grafana
    depends_on:
      - prometheus

volumes:
  postgres_test_data:
  redis_test_data:
  prometheus_data:
  grafana_data:

networks:
  default:
    name: crawlrs_test_network
```

### 2.2 环境变量配置

创建 `.env.test` 文件：

```bash
# Database
DB_HOST=postgres_test
DB_PORT=5432
DB_USER=postgres
DB_PASSWORD=postgres
DB_NAME=crawlrs_test
DATABASE_URL=postgres://postgres:postgres@postgres_test:5432/crawlrs_test

# Redis
REDIS_HOST=redis_test
REDIS_PORT=6379
REDIS_URL=redis://redis_test:6379

# Browser
CHROMIUM_REMOTE_DEBUGGING_URL=http://chromium:3000

# Proxy
FLARESOLVERR_URL=http://flaresolverr:8191

# API Server
API_HOST=0.0.0.0
API_PORT=8080
RUST_LOG=info

# Worker
WORKER_COUNT=4

# Sync Wait
SYNC_WAIT_DEFAULT_MS=5000
SYNC_WAIT_MAX_MS=30000

# Rate Limiting
RATE_LIMIT_RPM=1000

# Concurrency
TEAM_CONCURRENT_LIMIT=20

# Metrics
METRICS_ENABLED=true
METRICS_PORT=9090
```

### 2.3 Prometheus 配置

创建 `test-config/prometheus.yml`：

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'crawlrs'
    static_configs:
      - targets: ['api_server:8080']
    metrics_path: /metrics
    scheme: http

  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'redis'
    static_configs:
      - targets: ['redis_test:6379']
```

### 2.4 Grafana 仪表盘

创建 `test-config/grafana/dashboards/crawlrs_overview.json`：

```json
{
  "dashboard": {
    "title": "crawlrs 测试监控仪表盘",
    "panels": [
      {
        "title": "API 请求率",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(http_requests_total[5m])",
            "legendFormat": "{{method}} {{endpoint}}"
          }
        ]
      },
      {
        "title": "响应延迟 P95/P99",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))",
            "legendFormat": "P95"
          },
          {
            "expr": "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))",
            "legendFormat": "P99"
          }
        ]
      },
      {
        "title": "错误率",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(http_requests_total{status=~'5..'}[5m]) / rate(http_requests_total[5m])",
            "legendFormat": "错误率"
          }
        ]
      },
      {
        "title": "任务处理速率",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(tasks_completed_total[1m])",
            "legendFormat": "完成速率"
          }
        ]
      },
      {
        "title": "队列积压",
        "type": "stat",
        "targets": [
          {
            "expr": "tasks_queue_depth",
            "legendFormat": "积压任务数"
          }
        ]
      }
    ]
  }
}
```

---

## 3. 测试数据准备

### 3.1 测试 Fixtures

创建 `tests/fixtures/mod.rs`：

```rust
use sea_orm::DatabaseConnection;
use uuid::Uuid;

pub struct TestFixtures {
    pub db: DatabaseConnection,
    pub team_id: Uuid,
    pub api_key_id: Uuid,
}

impl TestFixtures {
    pub async fn new() -> Self {
        let db = setup_test_db().await;
        let team_id = Self::create_team(&db).await;
        let api_key_id = Self::create_api_key(&db, team_id).await;
        
        Self {
            db,
            team_id,
            api_key_id,
        }
    }
    
    async fn create_team(db: &DatabaseConnection) -> Uuid {
        let team_id = Uuid::new_v4();
        team::ActiveModel {
            id: Set(team_id),
            name: Set(format!("Test Team {}", team_id.to_string()[..8])),
            plan: Set(Plan::Professional),
            concurrent_limit: Set(20),
            rate_limit_rpm: Set(1000),
            ..Default::default()
        }.insert(db).await.unwrap();
        team_id
    }
    
    async fn create_api_key(db: &DatabaseConnection, team_id: Uuid) -> Uuid {
        let api_key_id = Uuid::new_v4();
        let api_key = format!("crawlrs_test_{}", Uuid::new_v4().to_string());
        
        api_key::ActiveModel {
            id: Set(api_key_id),
            key: Set(api_key),
            team_id: Set(team_id),
            scopes: Set(ApiKeyScope::full_access()),
            is_active: Set(true),
            created_at: Set(chrono::Utc::now()),
            expires_at: Set(chrono::Utc::now() + chrono::Duration::days(30)),
            ..Default::default()
        }.insert(db).await.unwrap();
        
        api_key_id
    }
}

impl Drop for TestFixtures {
    fn drop(&mut self) {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async {
                cleanup_test_data(&self.db, &self.team_id).await;
            });
    }
}
```

### 3.2 测试数据生成脚本

创建 `scripts/generate_test_data.py`：

```python
#!/usr/bin/env python3
"""生成测试数据脚本"""

import asyncio
import aiohttp
import random
import string
from datetime import datetime, timedelta

# 测试 URL 列表
TEST_URLS = [
    "https://example.com",
    "https://httpbin.org/html",
    "https://httpbin.org/json",
    "https://httpbin.org/robots.txt",
    "https://www.wikipedia.org",
    "https://jsonplaceholder.typicode.com/posts/1",
]

# 测试查询词
TEST_QUERIES = [
    "rust programming language",
    "web scraping tutorial",
    "async await rust",
    "tokio runtime",
    "sea-orm database",
    "axum web framework",
]

async def generate_test_tasks(base_url: str, api_key: str, count: int = 100):
    """生成测试任务数据"""
    
    async with aiohttp.ClientSession() as session:
        headers = {
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        }
        
        tasks = []
        
        # 生成 Scrape 任务
        for i in range(count // 3):
            url = random.choice(TEST_URLS)
            response = await session.post(
                f"{base_url}/v1/scrape",
                json={
                    "url": url,
                    "formats": ["markdown"],
                },
                headers=headers,
            )
            if response.status == 200:
                task_id = (await response.json())["id"]
                tasks.append(("scrape", task_id))
        
        # 生成 Search 任务
        for i in range(count // 3):
            query = random.choice(TEST_QUERIES)
            response = await session.post(
                f"{base_url}/v1/search",
                json={
                    "query": query,
                    "limit": 10,
                },
                headers=headers,
            )
            if response.status == 200:
                task_id = (await response.json())["id"]
                tasks.append(("search", task_id))
        
        # 生成 Crawl 任务
        for i in range(count // 3):
            url = random.choice(TEST_URLS)
            response = await session.post(
                f"{base_url}/v1/crawl",
                json={
                    "url": url,
                    "crawler_options": {
                        "max_depth": 2,
                        "limit": 5,
                    },
                },
                headers=headers,
            )
            if response.status == 200:
                task_id = (await response.json())["id"]
                tasks.append(("crawl", task_id))
        
        return tasks

if __name__ == "__main__":
    base_url = "http://localhost:8080"
    api_key = "your_test_api_key"
    
    tasks = asyncio.run(generate_test_tasks(base_url, api_key, 100))
    print(f"生成了 {len(tasks)} 个测试任务")
```

---

## 4. 测试用例设计

### 4.1 搜索功能测试

#### 4.1.1 单元测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| UT-SRCH-001 | 单引擎搜索 | 验证 Google 引擎返回结果 | 结果包含 title/url/content/source_engine | P0 |
| UT-SRCH-002 | 多引擎聚合 | 验证 Bing/百度/Sogou 并发查询 | 响应时间 < 10s，至少 2 个引擎成功 | P0 |
| UT-SRCH-003 | 结果去重 | 验证 URL 和标题去重 | 无重复 URL，相似标题合并 | P0 |
| UT-SRCH-004 | ARC ID 生成 | 验证 Google ARC_ID 缓存机制 | 1 小时内相同 ARC_ID | P0 |
| UT-SRCH-005 | Bing Cookie 构造 | 验证 Cookie 和 FORM 参数 | 参数格式正确 | P0 |
| UT-SRCH-006 | 缓存键生成 | 验证搜索缓存键逻辑 | 相同查询生成相同键 | P0 |
| UT-SRCH-007 | 断路器触发 | 验证引擎失败时的降级 | 连续 5 次失败后跳过引擎 | P0 |

#### 4.1.2 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-SRCH-001 | 搜索 API 端到端 | POST /v1/search 完整流程 | 状态码 200，响应结构正确 | P0 |
| IT-SRCH-002 | 同步等待返回 | sync_wait_ms 参数 | 任务完成时同步返回 | P0 |
| IT-SRCH-003 | 缓存命中 | 相同查询二次请求 | 响应时间 < 100ms，cache_hit=true | P0 |
| IT-SRCH-004 | 引擎降级 | 模拟某引擎网络不可达 | 自动降级到其他引擎 | P0 |
| IT-SRCH-005 | 搜索 + 抓取回填 | crawl_results 参数 | 自动创建爬取任务 | P1 |

### 4.2 抓取功能测试

#### 4.2.1 单元测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| UT-SCRP-001 | Fetch 引擎路由 | 静态页面选择 ReqwestEngine | 引擎选择正确 | P0 |
| UT-SCRP-002 | Playwright 路由 | JS 渲染页面选择 Playwright | 引擎选择正确 | P0 |
| UT-SCRP-003 | 截图生成 | 验证 screenshot 格式 | Base64 PNG 数据 | P0 |
| UT-SCRP-004 | SSRF 防护 | 验证私有 IP 拒绝 | 返回错误码 400 | P0 |
| UT-SCRP-005 | 超时处理 | 验证 timeout 参数 | 超时后任务失败 | P0 |

#### 4.2.2 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-SCRP-001 | 单页面抓取 | POST /v1/scrape 静态页面 | Markdown/HTML 内容正确 | P0 |
| IT-SCRP-002 | JS 渲染页面 | 抓取 SPA 应用 | 动态内容存在于响应中 | P0 |
| IT-SCRP-003 | 多格式输出 | formats: [markdown, html, screenshot] | 所有格式正确返回 | P0 |
| IT-SCRP-004 | 同步等待完成 | sync_wait_ms=5000，快速任务 | < 5s 返回结果 | P0 |
| IT-SCRP-005 | 同步等待超时 | sync_wait_ms=3000，慢任务 | 返回任务 ID | P0 |
| IT-SCRP-006 | 代理配置 | 使用代理抓取 | 代理生效 | P1 |
| IT-SCRP-007 | 跳过 TLS | skip_tls_verification | 自签名证书可访问 | P1 |

### 4.3 爬取功能测试

#### 4.3.1 单元测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| UT-CRL-001 | 深度控制 | max_depth=2 | 爬取深度 ≤ 2 | P0 |
| UT-CRL-002 | 路径包含过滤 | include_paths=["/blog/*"] | 只爬取匹配路径 | P0 |
| UT-CRL-003 | 路径排除过滤 | exclude_paths=["/admin/*"] | 跳过匹配路径 | P0 |
| UT-CRL-004 | Robots.txt 解析 | 验证 robots.txt 规则 | 正确遵守 Disallow | P0 |
| UT-CRL-005 | 任务去重 | 相同 URL 多次访问 | 只处理一次 | P0 |
| UT-CRL-006 | 并发限制 | max_concurrency=5 | 同时处理 ≤ 5 个任务 | P0 |

#### 4.3.2 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-CRL-001 | 全站爬取 | POST /v1/crawl 完整流程 | 状态最终变为 completed | P0 |
| IT-CRL-002 | 路径过滤 | include/exclude 组合 | 过滤规则生效 | P0 |
| IT-CRL-003 | Robots.txt 遵守 | 验证 Disallow 路径 | 不爬取禁止路径 | P0 |
| IT-CRL-004 | 并发任务处理 | 同时创建多个爬取任务 | 无资源竞争，数据一致 | P0 |
| IT-CRL-005 | 任务取消 | POST /v1/crawl/:id/cancel | 任务状态变为 cancelled | P0 |
| IT-CRL-006 | 结果分页 | GET /v1/crawl/:id/results | 分页正确返回 | P0 |

### 4.4 提取功能测试

#### 4.4.1 单元测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| UT-EXT-001 | CSS 选择器提取 | 验证提取规则 | 正确提取页面元素 | P0 |
| UT-EXT-002 | LLM Prompt 提取 | 验证 LLM 集成 | 按 Prompt 返回结构化数据 | P0 |
| UT-EXT-003 | Token 计数 | 验证 TokenUsage | 正确追踪 Token 使用 | P0 |
| UT-EXT-004 | 错误恢复 | 验证重试机制 | 失败后自动重试 | P0 |

#### 4.4.2 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-EXT-001 | CSS 选择器提取 | POST /v1/extract with schema | 正确提取数据 | P0 |
| IT-EXT-002 | LLM 提取 | POST /v1/extract with prompt | LLM 返回结构化结果 | P0 |
| IT-EXT-003 | 错误恢复 | 模拟网络错误 | 自动重试，成功完成 | P0 |
| IT-EXT-004 | 超时处理 | 长时间 LLM 请求 | 超时后任务失败 | P0 |
| IT-EXT-005 | 资源耗尽 | 高并发 LLM 请求 | 优雅处理，不崩溃 | P0 |

### 4.5 统一任务管理测试

#### 4.5.1 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-TASK-001 | 批量任务查询 | POST /v2/tasks/query | 返回所有任务 | P0 |
| IT-TASK-002 | 状态过滤 | filters.status=["completed"] | 只返回匹配状态 | P0 |
| IT-TASK-003 | 任务类型过滤 | filters.task_type=["scrape"] | 只返回匹配类型 | P0 |
| IT-TASK-004 | 排除结果 | include_results=false | 响应不包含 result | P0 |
| IT-TASK-005 | 批量取消 | POST /v2/tasks/cancel | 所有任务 cancelled=true | P0 |
| IT-TASK-006 | 取消已完成任务 | 取消 completed 任务 | cancelled=false, reason=已结束 | P0 |
| IT-TASK-007 | 强制取消 | force=true | 任何状态都可取消 | P0 |
| IT-TASK-008 | Crawl 级联取消 | 取消父任务 | 所有子任务同时取消 | P0 |

### 4.6 并发与限流测试

#### 4.6.1 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-LIMIT-001 | 速率限制 | RPM 超过限制 | 返回 429 | P0 |
| IT-LIMIT-002 | 团队并发限制 | 并发槽位用尽 | 任务进入积压队列 | P0 |
| IT-LIMIT-003 | 积压调度 | BacklogWorker 调度 | 槽位释放后执行 | P0 |
| IT-LIMIT-004 | 不同套餐限制 | 免费/专业/企业 | 限制生效 | P0 |

### 4.7 错误处理测试

#### 4.7.1 单元测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| UT-ERR-001 | 断路器开启 | 连续 5 次失败 | 断路器状态为 Open | P0 |
| UT-ERR-002 | 断路器半开 | 30 秒后尝试恢复 | 状态变为 Half-Open | P0 |
| UT-ERR-003 | 断路器关闭 | 恢复成功后 | 状态变为 Closed | P0 |
| UT-ERR-004 | 引擎降级 | 主引擎断路 | 自动降级到备引擎 | P0 |

#### 4.7.2 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-ERR-001 | 超时处理 | timeout=10 | 10s 后返回超时错误 | P0 |
| IT-ERR-002 | SSRF 防护 | 访问 127.0.0.1 | 返回 SSRF 错误 | P0 |
| IT-ERR-003 | 引擎降级 | 主引擎不可用 | 自动使用备引擎 | P0 |
| IT-ERR-004 | 断路器触发 | 连续失败 5 次 | 跳过故障引擎 | P0 |

### 4.8 Webhook 测试

#### 4.8.1 集成测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| IT-WH-001 | 任务完成回调 | 配置 webhook_url | 收到 POST 请求 | P0 |
| IT-WH-002 | 签名验证 | 验证 X-crawlrs-Signature | 签名正确 | P0 |
| IT-WH-003 | 失败重试 | Webhook 返回 500 | 重试次数递增 | P0 |
| IT-WH-004 | 指数退避 | 重试间隔 | 2s → 4s → 8s → 16s → 32s | P0 |
| IT-WH-005 | 死信处理 | 超过 5 次重试 | 状态变为 dead | P0 |

### 4.9 安全测试

| 用例 ID | 用例名称 | 测试内容 | 预期结果 | 优先级 |
|---------|---------|---------|---------|--------|
| SEC-001 | 无效 API Key | 使用错误凭证 | 返回 401 | P0 |
| SEC-002 | 团队数据隔离 | Team B 查询 Team A 任务 | 返回 403 | P0 |
| SEC-003 | SSRF 防护 | 访问内网 IP | 拒绝访问 | P0 |
| SEC-004 | SQL 注入 | 特殊字符输入 | 正确转义 | P0 |
| SEC-005 | 参数越权 | 访问不存在的任务 | 正确返回 404 | P0 |

---

## 5. 压力测试场景

### 5.1 高并发抓取测试

**测试目标**: 验证系统在高并发下的稳定性

**测试配置** (K6):

```javascript
// tests/load/scrape_high_concurrency.js
import http from 'k6/http';
import { check, sleep } from 'k6';

export const options = {
  stages: [
    { duration: '2m', target: 100 },   // 预热到 100 VU
    { duration: '5m', target: 500 },   // 快速增长到 500 VU
    { duration: '10m', target: 1000 }, // 峰值 1000 VU
    { duration: '5m', target: 0 },     // 降级
  ],
  thresholds: {
    http_req_duration: ['p(95)<50', 'p(99)<200'],
    http_req_failed: ['rate<0.001'],
    http_reqs: ['rate>10000'],
  },
};

export default function () {
  const url = 'http://localhost:8080/v1/scrape';
  const payload = JSON.stringify({
    url: 'https://example.com',
    formats: ['markdown'],
  });
  
  const params = {
    headers: {
      'Content-Type': 'application/json',
      'Authorization': 'Bearer test-api-key',
    },
  };
  
  const res = http.post(url, payload, params);
  
  check(res, {
    'status is 200': (r) => r.status === 200,
    'response has task_id': (r) => r.json('id') !== undefined,
    'response time < 200ms': (r) => r.timings.duration < 200,
  });
  
  sleep(1);
}
```

**预期结果**:
- ✅ P95 延迟 < 50ms
- ✅ P99 延迟 < 200ms
- ✅ 错误率 < 0.1%
- ✅ 吞吐量 > 10000 RPS

### 5.2 持续负载测试

**测试目标**: 验证系统长时间运行的稳定性

**测试配置**:

```javascript
export const options = {
  vus: 200,                    // 固定 200 个虚拟用户
  duration: '1h',              // 持续 1 小时
  thresholds: {
    http_req_duration: ['p(99)<1000'],
    http_req_failed: ['rate<0.05'],
    vmem: ['<512MB'],          // 内存增长 < 512MB
  },
};
```

**监控指标**:
- CPU 使用率（应稳定 < 70%）
- 内存使用率（检查泄漏，应 < 10% 增长）
- 数据库连接数（应稳定 < 100）
- Redis 内存占用（应稳定 < 256MB）

### 5.3 搜索并发聚合压力测试

**测试目标**: 验证多引擎并发查询的稳定性

**测试配置**:

```javascript
export const options = {
  stages: [
    { duration: '1m', target: 50 },
    { duration: '5m', target: 200 },
    { duration: '2m', target: 0 },
  ],
  thresholds: {
    'http_req_duration{endpoint:search}': ['p(95)<10000'],
    'search_cache_hit_rate': ['rate>0.6'],
  },
};
```

**预期结果**:
- ✅ P95 延迟 < 10 秒
- ✅ 缓存命中率 > 60%
- ✅ 至少 2 个引擎成功
- ✅ 无内存泄漏

### 5.4 同步等待压力测试

**测试目标**: 验证同步等待在高并发下不会耗尽连接池

**测试配置**:

```javascript
export const options = {
  vus: 100,
  duration: '5m',
  thresholds: {
    'sync_return_rate': ['rate>0.7'],
    'http_req_duration': ['p(99)<6000'],
  },
};

export default function () {
  const payload = JSON.stringify({
    url: 'https://httpbin.org/delay/3',
    formats: ['markdown'],
    sync_wait_ms: 5000,
  });
  
  const res = http.post('http://localhost:8080/v1/scrape', payload, params);
  const body = JSON.parse(res.body);
  
  check(res, {
    'sync returned': (r) => body.status === 'completed',
  });
}
```

**预期结果**:
- ✅ 同步返回率 > 70%
- ✅ P99 延迟 < 6 秒
- ✅ 无连接池耗尽
- ✅ 无死锁

### 5.5 数据库压力测试

**测试目标**: 验证数据库在高并发下的性能

**测试配置**:
- 100 个并发 Worker
- 每秒执行 1000 次 `acquire_next()`
- 监控查询时间

**预期结果**:
- ✅ P95 查询时间 < 10ms
- ✅ 无慢查询（> 50ms）
- ✅ 连接池无耗尽

---

## 6. 测试执行流程

### 6.1 测试执行顺序

```
Phase 1: 单元测试
    │
    ├── 1.1 领域模型测试
    ├── 1.2 引擎选择测试
    ├── 1.3 并发控制测试
    ├── 1.4 搜索引擎测试
    ├── 1.5 搜索缓存测试
    ├── 1.6 同步等待机制测试
    └── 1.7 错误处理测试
            │
            ▼
Phase 2: 集成测试
    │
    ├── 2.1 API 端到端测试
    ├── 2.2 Worker 测试
    ├── 2.3 数据库交互测试
    ├── 2.4 统一任务管理测试
    ├── 2.5 Webhook 测试
    └── 2.6 限流测试
            │
            ▼
Phase 3: E2E 测试
    │
    ├── 3.1 完整搜索流程
    ├── 3.2 完整抓取流程
    ├── 3.3 完整爬取流程
    └── 3.4 完整提取流程
            │
            ▼
Phase 4: 压力测试
    │
    ├── 4.1 高并发抓取 (K6)
    ├── 4.2 持续负载测试
    ├── 4.3 搜索并发聚合测试
    ├── 4.4 同步等待性能测试
    └── 4.5 数据库性能测试
            │
            ▼
Phase 5: UAT 测试
    │
    ├── 5.1 功能验收测试
    ├── 5.2 性能验收测试
    ├── 5.3 部署验收测试
    ├── 5.4 监控验收测试
    └── 5.5 安全验收测试
```

### 6.2 测试执行命令

```bash
#!/bin/bash
# test_runner.sh - 全流程测试执行脚本

set -e

echo "=========================================="
echo "crawlrs 全流程测试执行"
echo "开始时间: $(date)"
echo "=========================================="

# Phase 1: 单元测试
echo ""
echo ">>> Phase 1: 单元测试"
cargo test --lib -- --test-threads=4

# Phase 2: 集成测试
echo ""
echo ">>> Phase 2: 集成测试"
docker-compose -f docker-compose.test.yml up -d
sleep 10  # 等待服务启动
cargo test --test integration -- --test-threads=2
docker-compose -f docker-compose.test.yml down

# Phase 3: E2E 测试
echo ""
echo ">>> Phase 3: E2E 测试"
docker-compose -f docker-compose.test.yml up -d
sleep 10
cargo test --test e2e -- --test-threads=1
docker-compose -f docker-compose.test.yml down

# Phase 4: 压力测试 (K6)
echo ""
echo ">>> Phase 4: 压力测试"
docker-compose -f docker-compose.test.yml up -d
sleep 10
# 运行 K6 压力测试
k6 run tests/load/scrape_high_concurrency.js
k6 run tests/load/search_concurrent.js
k6 run tests/load/sync_wait_stress.js
docker-compose -f docker-compose.test.yml down

# Phase 5: 覆盖率报告
echo ""
echo ">>> 生成覆盖率报告"
cargo tarpaulin --out Html --output-dir coverage/

echo ""
echo "=========================================="
echo "测试执行完成"
echo "结束时间: $(date)"
echo "=========================================="
```

### 6.3 测试结果记录模板

创建 `test-results/TEST_RESULTS.md`：

```markdown
# 测试执行报告

## 执行信息

- **执行时间**: 2025-01-12 10:00:00
- **执行环境**: Docker Compose 测试栈
- **Git Commit**: abc1234
- **测试人员**: QA 团队

## 测试统计

### Phase 1: 单元测试

| 测试套件 | 测试数 | 通过 | 失败 | 通过率 | 执行时间 |
|---------|-------|------|------|--------|---------|
| 领域模型 | 15 | 15 | 0 | 100% | 2m 30s |
| 引擎选择 | 10 | 10 | 0 | 100% | 1m 45s |
| 并发控制 | 8 | 8 | 0 | 100% | 3m 20s |
| 搜索引擎 | 20 | 20 | 0 | 100% | 5m 10s |
| 搜索缓存 | 5 | 5 | 0 | 100% | 1m 15s |
| 同步等待 | 10 | 10 | 0 | 100% | 2m 00s |
| 错误处理 | 12 | 12 | 0 | 100% | 2m 30s |
| **合计** | **80** | **80** | **0** | **100%** | **18m 30s** |

### Phase 2: 集成测试

| 测试套件 | 测试数 | 通过 | 失败 | 通过率 | 执行时间 |
|---------|-------|------|------|--------|---------|
| API 端到端 | 25 | 25 | 0 | 100% | 15m 00s |
| Worker 测试 | 10 | 10 | 0 | 100% | 8m 00s |
| 数据库交互 | 15 | 15 | 0 | 100% | 5m 00s |
| 任务管理 | 20 | 20 | 0 | 100% | 10m 00s |
| Webhook | 8 | 8 | 0 | 100% | 6m 00s |
| 限流测试 | 5 | 5 | 0 | 100% | 3m 00s |
| **合计** | **83** | **83** | **0** | **100%** | **47m 00s** |

### Phase 3: E2E 测试

| 测试套件 | 测试数 | 通过 | 失败 | 通过率 | 执行时间 |
|---------|-------|------|------|--------|---------|
| 完整搜索流程 | 5 | 5 | 0 | 100% | 20m 00s |
| 完整抓取流程 | 5 | 5 | 0 | 100% | 15m 00s |
| 完整爬取流程 | 3 | 3 | 0 | 100% | 30m 00s |
| 完整提取流程 | 2 | 2 | 0 | 100% | 10m 00s |
| **合计** | **15** | **15** | **0** | **100%** | **75m 00s** |

### Phase 4: 压力测试

| 测试场景 | 目标值 | 实际值 | 结果 |
|---------|-------|-------|------|
| API 吞吐量 | > 10000 RPS | 12500 RPS | ✅ 通过 |
| P95 延迟 | < 50ms | 42ms | ✅ 通过 |
| P99 延迟 | < 200ms | 156ms | ✅ 通过 |
| 错误率 | < 0.1% | 0.05% | ✅ 通过 |
| 同步返回率 | > 70% | 78% | ✅ 通过 |
| 搜索缓存命中率 | > 60% | 65% | ✅ 通过 |
| 数据库 P95 查询 | < 10ms | 6ms | ✅ 通过 |

### Phase 5: UAT 测试

| 类别 | 总数 | 通过 | 失败 | 通过率 |
|-----|------|------|------|-------|
| 搜索功能 | 5 | 5 | 0 | 100% |
| 抓取功能 | 7 | 7 | 0 | 100% |
| 爬取功能 | 4 | 4 | 0 | 100% |
| 提取功能 | 3 | 3 | 0 | 100% |
| 任务管理 | 5 | 5 | 0 | 100% |
| 并发测试 | 2 | 2 | 0 | 100% |
| 错误处理 | 3 | 3 | 0 | 100% |
| Webhook | 2 | 2 | 0 | 100% |
| 性能测试 | 4 | 4 | 0 | 100% |
| 部署测试 | 3 | 3 | 0 | 100% |
| 监控测试 | 3 | 3 | 0 | 100% |
| 安全测试 | 2 | 2 | 0 | 100% |
| **合计** | **43** | **43** | **0** | **100%** |

## 测试覆盖率

| 模块 | 行覆盖率 | 分支覆盖率 | 函数覆盖率 |
|-----|---------|-----------|-----------|
| src/domain | 92% | 85% | 95% |
| src/infrastructure | 88% | 80% | 90% |
| src/presentation | 85% | 78% | 88% |
| src/engines | 90% | 82% | 92% |
| **总体** | **89%** | **81%** | **91%** |

## 性能基准对比

| 指标 | PRD 目标 | 本次测试 | 变化 |
|-----|---------|---------|------|
| API 吞吐量 | > 10000 RPS | 12500 RPS | +25% |
| P50 延迟 | < 50ms | 25ms | -50% |
| P99 延迟 | < 200ms | 156ms | -22% |
| 任务处理速度 | > 1000/min | 1200/min | +20% |
| 成功率 | > 99.9% | 99.95% | +0.05% |

## 问题统计

### 高风险问题（阻塞发布）

- 无

### 中风险问题（需要修复）

- 无

### 低风险问题（可延后）

- 无

## 验收结论

**结论**: ✅ 通过验收

**签字**:
- 产品经理: _______________  日期: _______________
- 技术负责人: _______________  日期: _______________
- QA 负责人: _______________  日期: _______________
```

---

## 7. 风险与缓解措施

### 7.1 测试风险

| 风险 | 可能性 | 影响 | 缓解措施 |
|-----|-------|------|---------|
| 测试环境不稳定 | 中 | 高 | 使用 Docker Compose 容器化部署，配置健康检查 |
| 外部依赖不可用 | 中 | 高 | 使用本地模拟服务（FlareSolverr 容器） |
| 测试数据不足 | 低 | 中 | 使用 Fixtures 自动生成测试数据 |
| 并发测试资源不足 | 中 | 中 | 使用云服务器进行压力测试 |
| 测试覆盖不全 | 低 | 高 | 基于 PRD 和 UAT 文档编写完整测试用例 |

### 7.2 环境准备清单

- [ ] Docker 和 Docker Compose 安装
- [ ] 4GB+ 可用内存
- [ ] 20GB+ 可用磁盘空间
- [ ] 网络连通性配置
- [ ] K6 压力测试工具安装
- [ ] cargo-tarpaulin 覆盖率工具安装

---

## 8. 附录

### 8.1 相关文档链接

- [PRD 文档](./docs/prd.md)
- [测试文档](./docs/test.md)
- [UAT 文档](./docs/uat.md)
- [API 文档](./docs/api.md)
- [部署指南](./docs/deployment.md)

### 8.2 工具版本要求

| 工具 | 最低版本 | 推荐版本 |
|-----|---------|---------|
| Docker | 20.10 | 24.0 |
| Docker Compose | 2.0 | 2.20 |
| Rust | 1.75 | 1.76 |
| K6 | 0.45 | 0.47 |
| cargo-tarpaulin | 0.27 | 0.30 |

### 8.3 联系人

- **测试负责人**: QA 团队
- **技术支持**: 研发团队
- **环境问题**: 运维团队

---

**文档版本**: v1.0.0  
**最后更新**: 2025-01-12  
**下次审查**: 2025-01-19
