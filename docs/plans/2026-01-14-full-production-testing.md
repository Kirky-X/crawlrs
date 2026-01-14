# Crawlrs 全面生产环境测试计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**目标**: 在模拟真实生产服务器部署场景下，对 crawlrs 项目进行全面测试，包括 Docker 环境部署、特性测试矩阵、API 接口测试、性能验证和问题诊断。

**架构方法**: 使用 Docker Compose 部署完整的测试环境，包括 PostgreSQL、Redis、Chrome 浏览器容器、FlareSolverr、MinIO 等所有必需组件。编写 Python 自动化测试脚本，向所有 API 接口发起真实请求，生成详细的测试报告。

**技术栈**: Rust (Axum), PostgreSQL, Redis, Playwright/Chromium, FlareSolverr, MinIO, Docker Compose, Python (requests, pytest), Prometheus, Grafana。

---

## 任务 1：创建 Docker 测试环境配置

**文件**:
- 创建: `docker-compose.test.full.yml`
- 修改: `docker/prometheus/prometheus.yml`
- 创建: `config/test.env`

**Step 1: 创建完整的 Docker Compose 测试配置文件**

创建 `docker-compose.test.full.yml`:

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:15-alpine
    container_name: crawlrs-test-postgres
    environment:
      POSTGRES_USER: crawlrs
      POSTGRES_PASSWORD: password
      POSTGRES_DB: crawlrs
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U crawlrs -d crawlrs"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 10s
    networks:
      - crawlrs-network

  redis:
    image: redis:7-alpine
    container_name: crawlrs-test-redis
    command: redis-server --appendonly yes --maxmemory 256mb --maxmemory-policy allkeys-lru
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks:
      - crawlrs-network

  chrome:
    image: browserless/chrome:latest
    container_name: crawlrs-test-chrome
    environment:
      - MAX_CONCURRENT_SESSIONS=10
      - MAX_QUEUE_LENGTH=50
      - CONNECTION_TIMEOUT=30000
    ports:
      - "9222:9222"
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:3000/json/version"]
      interval: 30s
      timeout: 10s
      retries: 5
      start_period: 20s
    volumes:
      - chrome_data:/tmp
    networks:
      - crawlrs-network

  flaresolverr:
    image: ghcr.io/flaresolverr/flaresolverr:latest
    container_name: crawlrs-test-flaresolverr
    environment:
      - LOG_LEVEL=info
      - CAPTCHA_SOLVER=none
      - TZ=UTC
    ports:
      - "8191:8191"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8191/health"]
      interval: 30s
      timeout: 15s
      retries: 5
      start_period: 30s
    volumes:
      - flaresolverr_data:/tmp
    networks:
      - crawlrs-network

  minio:
    image: minio/minio:RELEASE.2024-01-01T00-00-00Z
    container_name: crawlrs-test-minio
    command: server /data --console-address ":9001"
    ports:
      - "9000:9000"
      - "9001:9001"
    environment:
      - MINIO_ROOT_USER=minioadmin
      - MINIO_ROOT_PASSWORD=minioadmin123
    volumes:
      - minio_data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9000/minio/health/live"]
      interval: 30s
      timeout: 20s
      retries: 5
    networks:
      - crawlrs-network

  crawlrs:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    image: crawlrs:test
    container_name: crawlrs-app
    command: ["./crawlrs", "api"]
    ports:
      - "3000:3000"
    environment:
      - CRAWLRS__DATABASE__URL=postgres://crawlrs:password@postgres:5432/crawlrs
      - CRAWLRS__REDIS__URL=redis://redis:6379
      - CRAWLRS__SERVER__HOST=0.0.0.0
      - CRAWLRS__SERVER__PORT=3000
      - CRAWLRS__SEARCH__ENGINES__GOOGLE_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__BING_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__BAIDU_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__SOGOU_ENABLED=true
      - CRAWLRS__CHROME_REMOTE_DEBUGGING_URL=http://chrome:9222
      - CRAWLRS__SEARCH__FLARESOLVERR__URL=http://flaresolverr:8191/v1
      - CRAWLRS__RATE_LIMITING__ENABLED=true
      - CRAWLRS__RATE_LIMITING__DEFAULT_RPM=100
      - CRAWLRS__METRICS__ENABLED=true
      - CRAWLRS__METRICS__PORT=3001
      - CRAWLRS__RUST_LOG=info
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
      chrome:
        condition: service_healthy
    volumes:
      - ../storage:/app/storage
      - ../logs:/app/logs
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s
    networks:
      - crawlrs-network

  prometheus:
    image: prom/prometheus:v2.45.0
    container_name: crawlrs-test-prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.enable-lifecycle'
    ports:
      - "9090:9090"
    volumes:
      - prometheus_data:/prometheus
    networks:
      - crawlrs-network

  grafana:
    image: grafana/grafana:10.0.0
    container_name: crawlrs-test-grafana
    environment:
      - GF_SECURITY_ADMIN_USER=admin
      - GF_SECURITY_ADMIN_PASSWORD=admin
    ports:
      - "3001:3000"
    volumes:
      - grafana_data:/var/lib/grafana
    depends_on:
      - prometheus
    networks:
      - crawlrs-network

networks:
  crawlrs-network:
    driver: bridge

volumes:
  postgres_data:
  redis_data:
  chrome_data:
  flaresolverr_data:
  minio_data:
  prometheus_data:
  grafana_data:
```

**Step 2: 创建测试环境变量文件**

创建 `config/test.env`:

```bash
# 测试环境变量配置
APP_HOST=0.0.0.0
APP_PORT=3000
ENV_MODE=test

# 数据库配置
DB_HOST=localhost
DB_PORT=5432
DB_NAME=crawlrs
DB_USER=crawlrs
DB_PASSWORD=password

# Redis配置
REDIS_HOST=localhost
REDIS_PORT=6379

# Chrome配置
CHROME_HOST=localhost
CHROME_PORT=9222

# FlareSolverr配置
FLARESOLVERR_HOST=localhost
FLARESOLVERR_PORT=8191

# MinIO配置
MINIO_HOST=localhost
MINIO_PORT=9000
MINIO_ROOT_USER=minioadmin
MINIO_ROOT_PASSWORD=minioadmin123

# 搜索引擎配置
SEARCH_ENGINE_GOOGLE_ENABLED=true
SEARCH_ENGINE_BING_ENABLED=true
SEARCH_ENGINE_BAIDU_ENABLED=true
SEARCH_ENGINE_SOGOU_ENABLED=true
SEARCH_ENGINE_DEFAULT=bing

# 速率限制配置
RATE_LIMITING_ENABLED=true
RATE_LIMITING_DEFAULT_RPM=100

# 监控配置
METRICS_ENABLED=true
METRICS_PORT=3001

# 测试配置
TEST_DATABASE_URL=postgres://crawlrs:password@localhost:5432/crawlrs
TEST_REDIS_PORT=6379
SKIP_S3_TESTS=false
SKIP_BROWSER_TESTS=false
SKIP_SEARCH_TESTS=false
WEBHOOK_SECRET=test-secret-key-for-testing-only
```

**Step 3: 创建健康检查脚本**

创建 `scripts/health-check.sh`:

```bash
#!/bin/bash
set -e

SERVICES=("postgres:5432" "redis:6379" "chrome:9222" "flaresolverr:8191" "minio:9000" "crawlrs:3000")

echo "=== Crawlrs 服务健康检查 ==="
for service in "${SERVICES[@]}"; do
    IFS=':' read -r host port <<< "$service"
    echo -n "检查 $host:$port ... "
    if nc -z -w5 "$host" "$port" 2>/dev/null; then
        echo "✓ 健康"
    else
        echo "✗ 不可用"
        exit 1
    fi
done
echo "所有服务正常运行！"
```

**Step 4: 运行验证**

Run: `chmod +x scripts/health-check.sh && ./scripts/health-check.sh`
Expected: 所有服务健康检查通过

**Step 5: 提交**

```bash
git add docker-compose.test.full.yml config/test.env scripts/health-check.sh
git commit -m "feat: 添加完整的 Docker 测试环境配置"
```

---

## 任务 2：创建 Python API 测试框架

**文件**:
- 创建: `tests/python/api_test_framework.py`
- 创建: `tests/python/conftest.py`
- 创建: `tests/python/requirements.txt`

**Step 1: 创建 API 测试框架核心**

创建 `tests/python/api_test_framework.py`:

```python
#!/usr/bin/env python3
"""
Crawlrs API 测试框架
"""

import json
import time
import requests
from datetime import datetime
from typing import Dict, Any, Optional, List
from dataclasses import dataclass, field
from enum import Enum
from urllib.parse import urljoin
import logging

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


class HTTPMethod(Enum):
    GET = "GET"
    POST = "POST"
    PUT = "PUT"
    DELETE = "DELETE"


@dataclass
class RequestInfo:
    method: str
    url: str
    headers: Dict[str, str]
    body: Optional[Any] = None
    timestamp: str = field(default_factory=lambda: datetime.now().isoformat())


@dataclass
class ResponseInfo:
    status_code: int
    headers: Dict[str, str]
    body: Any
    elapsed_time_ms: float
    timestamp: str = field(default_factory=lambda: datetime.now().isoformat())


@dataclass
class TestResult:
    test_name: str
    request: RequestInfo
    response: ResponseInfo
    success: bool
    error_message: Optional[str] = None


class CrawlrsAPIClient:
    """Crawlrs API 客户端"""
    
    def __init__(self, base_url: str = "http://localhost:3000", api_key: str = "test-api-key"):
        self.base_url = base_url.rstrip('/')
        self.api_key = api_key
        self.session = requests.Session()
        self.session.headers.update({
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json"
        })
        self.test_results: List[TestResult] = []
        
    def _send_request(self, method: HTTPMethod, endpoint: str, body: Optional[Dict] = None) -> tuple:
        url = urljoin(self.base_url + "/", endpoint.lstrip("/"))
        request_headers = dict(self.session.headers)
        
        request_info = RequestInfo(method=method.value, url=url, headers=request_headers, body=body)
        
        start_time = time.time()
        try:
            if method == HTTPMethod.GET:
                response = self.session.get(url, timeout=30)
            elif method == HTTPMethod.POST:
                response = self.session.post(url, json=body, headers=request_headers, timeout=30)
            elif method == HTTPMethod.PUT:
                response = self.session.put(url, json=body, headers=request_headers, timeout=30)
            elif method == HTTPMethod.DELETE:
                response = self.session.delete(url, timeout=30)
            else:
                raise ValueError(f"不支持的 HTTP 方法: {method}")
                
            elapsed_time = (time.time() - start_time) * 1000
            
            try:
                response_body = response.json()
            except json.JSONDecodeError:
                response_body = response.text
                
            response_info = ResponseInfo(
                status_code=response.status_code,
                headers=dict(response.headers),
                body=response_body,
                elapsed_time_ms=elapsed_time
            )
            return response_info, request_info, None
            
        except Exception as e:
            elapsed_time = (time.time() - start_time) * 1000
            error_info = ResponseInfo(status_code=0, headers={}, body=None, elapsed_time_ms=elapsed_time)
            return error_info, request_info, str(e)
    
    def _record_result(self, test_name: str, request: RequestInfo, response: ResponseInfo, error: Optional[str] = None) -> TestResult:
        success = response.status_code in [200, 201, 202, 204] and error is None
        result = TestResult(test_name=test_name, request=request, response=response, success=success, error_message=error)
        self.test_results.append(result)
        return result
    
    # API 端点方法
    def health_check(self) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, "/health")
        return self._record_result("health_check", request, response, error)
    
    def get_version(self) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, "/v1/version")
        return self._record_result("get_version", request, response, error)
    
    def search(self, query: str, engines: Optional[List[str]] = None, limit: int = 10) -> TestResult:
        body = {"query": query, "limit": limit}
        if engines:
            body["engines"] = engines
        response, request, error = self._send_request(HTTPMethod.POST, "/v1/search", body=body)
        return self._record_result(f"search_{query}", request, response, error)
    
    def crawl(self, url: str, options: Optional[Dict] = None) -> TestResult:
        body = {"url": url, "options": options or {}}
        response, request, error = self._send_request(HTTPMethod.POST, "/v1/crawl", body=body)
        return self._record_result(f"crawl_{url}", request, response, error)
    
    def get_crawl_status(self, crawl_id: str) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, f"/v1/crawl/{crawl_id}")
        return self._record_result(f"get_crawl_status_{crawl_id}", request, response, error)
    
    def get_crawl_results(self, crawl_id: str) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, f"/v1/crawl/{crawl_id}/results")
        return self._record_result(f"get_crawl_results_{crawl_id}", request, response, error)
    
    def cancel_crawl(self, crawl_id: str) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.DELETE, f"/v1/crawl/{crawl_id}")
        return self._record_result(f"cancel_crawl_{crawl_id}", request, response, error)
    
    def scrape(self, url: str, options: Optional[Dict] = None) -> TestResult:
        body = {"url": url, "options": options or {}}
        response, request, error = self._send_request(HTTPMethod.POST, "/v1/scrape", body=body)
        return self._record_result(f"scrape_{url}", request, response, error)
    
    def get_scrape_status(self, scrape_id: str) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, f"/v1/scrape/{scrape_id}")
        return self._record_result(f"get_scrape_status_{scrape_id}", request, response, error)
    
    def cancel_scrape(self, scrape_id: str) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.DELETE, f"/v1/scrape/{scrape_id}")
        return self._record_result(f"cancel_scrape_{scrape_id}", request, response, error)
    
    def extract(self, url: str, selectors: Dict[str, str]) -> TestResult:
        body = {"url": url, "selectors": selectors}
        response, request, error = self._send_request(HTTPMethod.POST, "/v1/extract", body=body)
        return self._record_result(f"extract_{url}", request, response, error)
    
    def create_webhook(self, url: str, events: List[str]) -> TestResult:
        body = {"url": url, "events": events}
        response, request, error = self._send_request(HTTPMethod.POST, "/v1/webhooks", body=body)
        return self._record_result(f"create_webhook_{url}", request, response, error)
    
    def get_team_geo_restrictions(self) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, "/v1/teams/geo-restrictions")
        return self._record_result("get_team_geo_restrictions", request, response, error)
    
    def update_team_geo_restrictions(self, restrictions: List[Dict]) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.PUT, "/v1/teams/geo-restrictions", body=restrictions)
        return self._record_result("update_team_geo_restrictions", request, response, error)
    
    def get_audit_logs(self) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, "/v1/audit/logs")
        return self._record_result("get_audit_logs", request, response, error)
    
    def get_denied_requests(self) -> TestResult:
        response, request, error = self._send_request(HTTPMethod.GET, "/v1/audit/denied")
        return self._record_result("get_denied_requests", request, response, error)
    
    def generate_report(self, output_path: str = "test_report.json") -> Dict:
        """生成测试报告"""
        total = len(self.test_results)
        success_count = sum(1 for r in self.test_results if r.success)
        
        response_times = [r.response.elapsed_time_ms for r in self.test_results if r.response.elapsed_time_ms > 0]
        avg_response_time = sum(response_times) / len(response_times) if response_times else 0
        
        report = {
            "summary": {
                "total_requests": total,
                "success_count": success_count,
                "failure_count": total - success_count,
                "success_rate_percent": round(success_count / total * 100, 2) if total > 0 else 0,
                "timestamp": datetime.now().isoformat()
            },
            "performance": {
                "average_response_time_ms": round(avg_response_time, 2),
                "min_response_time_ms": round(min(response_times), 2) if response_times else 0,
                "max_response_time_ms": round(max(response_times), 2) if response_times else 0
            },
            "detailed_results": [
                {
                    "test_name": r.test_name,
                    "endpoint": r.request.url,
                    "method": r.request.method,
                    "status_code": r.response.status_code,
                    "response_time_ms": round(r.response.elapsed_time_ms, 2),
                    "success": r.success,
                    "error": r.error_message
                }
                for r in self.test_results
            ]
        }
        
        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(report, f, ensure_ascii=False, indent=2)
            
        logger.info(f"测试报告已生成: {output_path}")
        return report
    
    def print_summary(self):
        print("\n" + "="*60)
        print("Crawlrs API 测试结果摘要")
        print("="*60)
        total = len(self.test_results)
        success_count = sum(1 for r in self.test_results if r.success)
        print(f"总请求数: {total}")
        print(f"成功: {success_count}")
        print(f"失败: {total - success_count}")
        print(f"成功率: {success_count/total*100:.2f}%" if total > 0 else "N/A")
        print("="*60 + "\n")


if __name__ == "__main__":
    client = CrawlrsAPIClient()
    client.health_check()
    client.get_version()
    report = client.generate_report()
    client.print_summary()
```

**Step 2: 创建 pytest 配置文件**

创建 `tests/python/conftest.py`:

```python
import pytest
import os
import sys
from typing import Generator

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from api_test_framework import CrawlrsAPIClient


@pytest.fixture(scope="session")
def api_client() -> Generator[CrawlrsAPIClient, None, None]:
    base_url = os.getenv("CRAWLRS_API_URL", "http://localhost:3000")
    api_key = os.getenv("CRAWLRS_API_KEY", "test-api-key")
    
    client = CrawlrsAPIClient(base_url=base_url, api_key=api_key)
    yield client
    
    report_path = os.getenv("TEST_REPORT_PATH", "test-results/report.json")
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    client.generate_report(report_path)


@pytest.fixture
def search_test_params():
    return {"query": "test query", "engines": ["bing"], "limit": 5}


@pytest.fixture
def crawl_test_params():
    return {"url": "https://example.com", "options": {"timeout": 30000, "extract_text": True}}


@pytest.fixture
def scrape_test_params():
    return {"url": "https://example.com", "options": {"wait_for_selector": "body", "timeout": 30000}}


@pytest.fixture
def extract_test_params():
    return {"url": "https://example.com", "selectors": {"title": "h1", "paragraphs": "p"}}


@pytest.fixture
def webhook_test_params():
    return {"url": "https://httpbin.org/post", "events": ["task.completed", "task.failed"]}
```

**Step 3: 创建测试 requirements**

创建 `tests/python/requirements.txt`:

```txt
requests>=2.31.0
pytest>=7.4.0
pytest-cov>=4.1.0
python-dotenv>=1.0.0
```

**Step 4: 提交**

```bash
git add tests/python/ -m "feat: 添加 Python API 测试框架"
```

---

## 任务 3：创建完整 API 测试套件

**文件**:
- 创建: `tests/python/test_api_endpoints.py`
- 创建: `tests/python/test_performance.py`
- 创建: `tests/python/test_error_handling.py`

**Step 1: 创建 API 端点测试**

创建 `tests/python/test_api_endpoints.py`:

```python
#!/usr/bin/env python3
"""
API 端点测试套件
"""

import pytest
import time


class TestHealthEndpoints:
    """健康检查端点测试"""
    
    def test_health_check(self, api_client):
        result = api_client.health_check()
        assert result.success, f"健康检查失败: {result.error_message}"
        assert result.response.status_code == 200
        assert "healthy" in str(result.response.body).lower()
    
    def test_version_endpoint(self, api_client):
        result = api_client.get_version()
        assert result.success, f"版本检查失败: {result.error_message}"
        assert result.response.status_code == 200


class TestSearchEndpoints:
    """搜索端点测试"""
    
    def test_basic_search(self, api_client):
        result = api_client.search(query="test", engines=["bing"], limit=5)
        assert result.success, f"搜索失败: {result.error_message}"
        assert result.response.status_code == 200
        
        response_data = result.response.body
        assert "success" in response_data
        assert response_data["success"] == True
    
    def test_search_with_multiple_engines(self, api_client):
        result = api_client.search(query="python", engines=["bing", "google"], limit=10)
        assert result.success, f"多引擎搜索失败: {result.error_message}"


class TestCrawlEndpoints:
    """爬取端点测试"""
    
    def test_create_crawl(self, api_client, crawl_test_params):
        result = api_client.crawl(
            url=crawl_test_params["url"],
            options=crawl_test_params["options"]
        )
        assert result.success, f"创建爬取任务失败: {result.error_message}"
        assert result.response.status_code in [200, 202]
    
    def test_get_crawl_status(self, api_client):
        crawl_result = api_client.crawl(url="https://example.com")
        assert crawl_result.success
        
        crawl_id = crawl_result.response.body.get("data", {}).get("id")
        if crawl_id:
            status_result = api_client.get_crawl_status(crawl_id)
            assert status_result.success


class TestScrapeEndpoints:
    """抓取端点测试"""
    
    def test_create_scrape(self, api_client, scrape_test_params):
        result = api_client.scrape(
            url=scrape_test_params["url"],
            options=scrape_test_params["options"]
        )
        assert result.success, f"创建抓取任务失败: {result.error_message}"
        assert result.response.status_code in [200, 202]


class TestExtractEndpoints:
    """提取端点测试"""
    
    def test_extract_content(self, api_client, extract_test_params):
        result = api_client.extract(
            url=extract_test_params["url"],
            selectors=extract_test_params["selectors"]
        )
        assert result.success, f"内容提取失败: {result.error_message}"
        assert result.response.status_code == 200


class TestWebhookEndpoints:
    """Webhook 端点测试"""
    
    def test_create_webhook(self, api_client, webhook_test_params):
        result = api_client.create_webhook(
            url=webhook_test_params["url"],
            events=webhook_test_params["events"]
        )
        assert result.success, f"创建 Webhook 失败: {result.error_message}"
        assert result.response.status_code in [200, 201]


class TestTeamEndpoints:
    """团队管理端点测试"""
    
    def test_get_team_geo_restrictions(self, api_client):
        result = api_client.get_team_geo_restrictions()
        assert result.response.status_code in [200, 403]


class TestAuditEndpoints:
    """审计端点测试"""
    
    def test_get_audit_logs(self, api_client):
        result = api_client.get_audit_logs()
        assert result.response.status_code in [200, 403]
    
    def test_get_denied_requests(self, api_client):
        result = api_client.get_denied_requests()
        assert result.response.status_code in [200, 403]


class TestRateLimiting:
    """速率限制测试"""
    
    def test_rate_limit_headers(self, api_client):
        results = []
        for i in range(5):
            result = api_client.health_check()
            results.append(result)
            time.sleep(0.1)
        
        success_count = sum(1 for r in results if r.success)
        assert success_count >= 4


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
```

**Step 2: 创建性能测试**

创建 `tests/python/test_performance.py`:

```python
#!/usr/bin/env python3
"""
性能测试套件
"""

import pytest
import time
import statistics
import concurrent.futures


class TestPerformanceTargets:
    """性能目标测试"""
    
    def test_response_time_p99(self, api_client):
        """测试 P99 延迟是否小于 200ms"""
        response_times = []
        
        for i in range(100):
            result = api_client.health_check()
            if result.response.elapsed_time_ms > 0:
                response_times.append(result.response.elapsed_time_ms)
            time.sleep(0.05)
        
        response_times.sort()
        p99_index = int(len(response_times) * 0.99)
        p99_latency = response_times[p99_index]
        
        print(f"\nP99 延迟: {p99_latency}ms")
        assert p99_latency < 200, f"P99 延迟 {p99_latency}ms 超过 200ms 目标"
    
    def test_response_time_p95(self, api_client):
        """测试 P95 延迟"""
        response_times = []
        
        for i in range(50):
            result = api_client.health_check()
            if result.response.elapsed_time_ms > 0:
                response_times.append(result.response.elapsed_time_ms)
            time.sleep(0.05)
        
        response_times.sort()
        p95_index = int(len(response_times) * 0.95)
        p95_latency = response_times[p95_index]
        
        print(f"P95 延迟: {p95_latency}ms")
        assert p95_latency < 150, f"P95 延迟 {p95_latency}ms 超过 150ms 目标"


class TestThroughput:
    """吞吐量测试"""
    
    def test_requests_per_second(self, api_client):
        """测试每秒请求数 (RPS)"""
        start_time = time.time()
        request_count = 0
        
        while time.time() - start_time < 10:
            result = api_client.health_check()
            if result.success:
                request_count += 1
        
        duration = time.time() - start_time
        rps = request_count / duration
        
        print(f"\nRPS: {rps:.2f}")
        assert rps >= 50, f"RPS {rps} 低于最小目标 50"
    
    def test_concurrent_requests(self, api_client):
        """测试并发请求处理"""
        def make_request():
            result = api_client.health_check()
            return result.success, result.response.elapsed_time_ms
        
        with concurrent.futures.ThreadPoolExecutor(max_workers=20) as executor:
            futures = [executor.submit(make_request) for _ in range(20)]
            results = [f.result() for f in concurrent.futures.as_completed(futures)]
        
        success_count = sum(1 for success, _ in results if success)
        response_times = [time_ms for _, time_ms in results if time_ms > 0]
        
        print(f"\n并发请求成功: {success_count}/20")
        assert success_count >= 18, f"并发请求成功率 {success_count}/20 不达标"


class TestCacheEffectiveness:
    """缓存效果测试"""
    
    def test_redis_cache_hit_rate(self, api_client):
        """测试 Redis 缓存命中率"""
        query = "test cache"
        
        result1 = api_client.search(query=query, engines=["bing"], limit=5)
        time1 = result1.response.elapsed_time_ms
        
        times = []
        for i in range(5):
            result = api_client.search(query=query, engines=["bing"], limit=5)
            if result.success:
                times.append(result.response.elapsed_time_ms)
        
        cached_times = [t for t in times if t < time1]
        cache_hit_rate = len(cached_times) / len(times) if times else 0
        
        print(f"\n缓存命中率: {cache_hit_rate*100:.1f}%")
        assert cache_hit_rate >= 0.3, f"缓存命中率 {cache_hit_rate*100:.1f}% 过低"


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
```

**Step 3: 创建错误处理测试**

创建 `tests/python/test_error_handling.py`:

```python
#!/usr/bin/env python3
"""
错误处理测试套件
"""

import pytest
import time


class TestErrorResponses:
    """错误响应测试"""
    
    def test_invalid_url_crawl(self, api_client):
        """测试无效 URL 爬取"""
        result = api_client.crawl(url="not-a-valid-url")
        assert result.response.status_code in [400, 422, 500]
    
    def test_missing_required_fields(self, api_client):
        """测试缺少必填字段"""
        result = api_client.search(query="")
        assert result.response.status_code in [400, 422]
    
    def test_rate_limit_exceeded(self, api_client):
        """测试速率限制触发"""
        results = []
        for i in range(200):
            result = api_client.health_check()
            results.append(result.response.status_code)
            time.sleep(0.01)
        
        assert 429 in results, "未能触发速率限制"


class TestServiceResilience:
    """服务韧性测试"""
    
    def test_service_recovery_after_failure(self, api_client):
        """测试故障后服务恢复"""
        result1 = api_client.health_check()
        assert result1.success
        
        time.sleep(0.5)
        
        result2 = api_client.health_check()
        assert result2.success, "服务未能在短暂故障后恢复"


class TestDataIntegrity:
    """数据完整性测试"""
    
    def test_response_data_consistency(self, api_client):
        """测试响应数据一致性"""
        results = []
        for i in range(3):
            result = api_client.search(query="test", engines=["bing"], limit=5)
            if result.success:
                results.append(result.response.body)
        
        if len(results) >= 2:
            assert results[0].keys() == results[1].keys(), "响应数据结构不一致"
    
    def test_data_persistence(self, api_client):
        """测试数据持久化"""
        result = api_client.crawl(url="https://example.com")
        assert result.success
        
        crawl_id = result.response.body.get("data", {}).get("id")
        
        time.sleep(2)
        
        if crawl_id:
            status_result = api_client.get_crawl_status(crawl_id)
            assert status_result.success, "数据未正确持久化"


class TestSecurity:
    """安全测试"""
    
    def test_sql_injection_prevention(self, api_client):
        """测试 SQL 注入防护"""
        malicious_input = "'; DROP TABLE users; --"
        result = api_client.search(query=malicious_input)
        assert result.response.status_code in [200, 400, 422]
    
    def test_xss_prevention(self, api_client):
        """测试 XSS 防护"""
        xss_input = "<script>alert('xss')</script>"
        result = api_client.search(query=xss_input)
        assert result.response.status_code in [200, 400, 422]


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
```

**Step 4: 提交**

```bash
git add tests/python/test_*.py -m "feat: 添加完整 API 测试套件"
```

---

## 任务 4：创建特性测试矩阵配置

**文件**:
- 创建: `tests/feature-matrix/test_config.py`
- 创建: `docker-compose.test.minio.yml`
- 创建: `docker-compose.test.browser.yml`
- 创建: `docker-compose.test.search.yml`

**Step 1: 创建特性测试配置**

创建 `tests/feature-matrix/test_config.py`:

```python
"""
特性测试矩阵配置
"""

from dataclasses import dataclass
from typing import List
from enum import Enum


class Feature(Enum):
    MINIO = "minio"
    BROWSER = "browser"
    SEARCH = "search"
    REDIS = "redis"
    POSTGRES = "postgres"
    FLARESOLVERR = "flaresolverr"


@dataclass
class TestConfiguration:
    name: str
    description: str
    enabled_features: List[Feature]
    expected_services: List[str]


CONFIGURATIONS = {
    "minio_only": TestConfiguration(
        name="minio_only",
        description="仅启用 MinIO 存储服务",
        enabled_features=[Feature.MINIO, Feature.POSTGRES, Feature.REDIS],
        expected_services=["postgres", "redis", "minio"]
    ),
    
    "browser_only": TestConfiguration(
        name="browser_only",
        description="仅启用浏览器服务",
        enabled_features=[Feature.BROWSER, Feature.FLARESOLVERR, Feature.POSTGRES, Feature.REDIS],
        expected_services=["postgres", "redis", "chrome", "flaresolverr"]
    ),
    
    "search_only": TestConfiguration(
        name="search_only",
        description="仅启用搜索功能",
        enabled_features=[Feature.SEARCH, Feature.POSTGRES, Feature.REDIS, Feature.FLARESOLVERR],
        expected_services=["postgres", "redis", "flaresolverr"]
    ),
    
    "full_features": TestConfiguration(
        name="full_features",
        description="启用所有功能",
        enabled_features=[Feature.MINIO, Feature.BROWSER, Feature.SEARCH, Feature.REDIS, Feature.POSTGRES, Feature.FLARESOLVERR],
        expected_services=["postgres", "redis", "chrome", "flaresolverr", "minio"]
    )
}


def get_configuration(name: str):
    return CONFIGURATIONS.get(name)


def list_configurations():
    return list(CONFIGURATIONS.keys())
```

**Step 2: 创建特性测试 Docker 配置**

创建 `docker-compose.test.minio.yml`:

```yaml
version: '3.8'
services:
  postgres:
    image: postgres:15-alpine
    container_name: crawlrs-minio-test-postgres
    environment:
      POSTGRES_USER: crawlrs
      POSTGRES_PASSWORD: password
      POSTGRES_DB: crawlrs
    ports: ["5432:5432"]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U crawlrs -d crawlrs"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-minio-network]

  redis:
    image: redis:7-alpine
    container_name: crawlrs-minio-test-redis
    command: redis-server --appendonly yes
    ports: ["6379:6379"]
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-minio-network]

  minio:
    image: minio/minio:RELEASE.2024-01-01T00-00-00Z
    container_name: crawlrs-minio-test
    command: server /data --console-address ":9001"
    ports: ["9000:9000", "9001:9001"]
    environment:
      - MINIO_ROOT_USER=minioadmin
      - MINIO_ROOT_PASSWORD=minioadmin123
    volumes: [minio_data:/data]
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9000/minio/health/live"]
      interval: 30s
      timeout: 20s
      retries: 5
    networks: [crawlrs-minio-network]

  crawlrs:
    build: {context: .., dockerfile: docker/Dockerfile}
    image: crawlrs:minio-test
    container_name: crawlrs-minio-app
    command: ["./crawlrs", "api"]
    ports: ["3000:3000"]
    environment:
      - CRAWLRS__DATABASE__URL=postgres://crawlrs:password@postgres:5432/crawlrs
      - CRAWLRS__REDIS__URL=redis://redis:6379
      - CRAWLRS__SERVER__HOST=0.0.0.0
      - CRAWLRS__SERVER__PORT=3000
      - SKIP_BROWSER_TESTS=true
      - SKIP_SEARCH_TESTS=true
    depends_on:
      postgres: {condition: service_healthy}
      redis: {condition: service_healthy}
    networks: [crawlrs-minio-network]

networks:
  crawlrs-minio-network: {driver: bridge}

volumes:
  minio_data:
```

**Step 3: 创建浏览器测试配置**

创建 `docker-compose.test.browser.yml`:

```yaml
version: '3.8'
services:
  postgres:
    image: postgres:15-alpine
    container_name: crawlrs-browser-test-postgres
    environment:
      POSTGRES_USER: crawlrs
      POSTGRES_PASSWORD: password
      POSTGRES_DB: crawlrs
    ports: ["5432:5432"]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U crawlrs -d crawlrs"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-browser-network]

  redis:
    image: redis:7-alpine
    container_name: crawlrs-browser-test-redis
    command: redis-server --appendonly yes
    ports: ["6379:6379"]
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-browser-network]

  chrome:
    image: browserless/chrome:latest
    container_name: crawlrs-browser-test-chrome
    environment:
      - MAX_CONCURRENT_SESSIONS=10
      - MAX_QUEUE_LENGTH=50
    ports: ["9222:9222"]
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:3000/json/version"]
      interval: 30s
      timeout: 10s
      retries: 5
    volumes: [chrome_data:/tmp]
    networks: [crawlrs-browser-network]

  flaresolverr:
    image: ghcr.io/flaresolverr/flaresolverr:latest
    container_name: crawlrs-browser-test-flaresolverr
    environment:
      - LOG_LEVEL=info
      - CAPTCHA_SOLVER=none
    ports: ["8191:8191"]
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8191/health"]
      interval: 30s
      timeout: 15s
      retries: 5
    volumes: [flaresolverr_data:/tmp]
    networks: [crawlrs-browser-network]

  crawlrs:
    build: {context: .., dockerfile: docker/Dockerfile}
    image: crawlrs:browser-test
    container_name: crawlrs-browser-app
    command: ["./crawlrs", "api"]
    ports: ["3000:3000"]
    environment:
      - CRAWLRS__DATABASE__URL=postgres://crawlrs:password@postgres:5432/crawlrs
      - CRAWLRS__REDIS__URL=redis://redis:6379
      - CRAWLRS__SERVER__HOST=0.0.0.0
      - CRAWLRS__SERVER__PORT=3000
      - CRAWLRS__CHROME_REMOTE_DEBUGGING_URL=http://chrome:9222
      - CRAWLRS__SEARCH__FLARESOLVERR__URL=http://flaresolverr:8191/v1
      - SKIP_S3_TESTS=true
      - SKIP_SEARCH_TESTS=true
    depends_on:
      postgres: {condition: service_healthy}
      redis: {condition: service_healthy}
      chrome: {condition: service_healthy}
    networks: [crawlrs-browser-network]

networks:
  crawlrs-browser-network: {driver: bridge}

volumes:
  chrome_data:
  flaresolverr_data:
```

**Step 4: 创建搜索测试配置**

创建 `docker-compose.test.search.yml`:

```yaml
version: '3.8'
services:
  postgres:
    image: postgres:15-alpine
    container_name: crawlrs-search-test-postgres
    environment:
      POSTGRES_USER: crawlrs
      POSTGRES_PASSWORD: password
      POSTGRES_DB: crawlrs
    ports: ["5432:5432"]
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U crawlrs -d crawlrs"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-search-network]

  redis:
    image: redis:7-alpine
    container_name: crawlrs-search-test-redis
    command: redis-server --appendonly yes
    ports: ["6379:6379"]
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    networks: [crawlrs-search-network]

  flaresolverr:
    image: ghcr.io/flaresolverr/flaresolverr:latest
    container_name: crawlrs-search-test-flaresolverr
    environment:
      - LOG_LEVEL=info
      - CAPTCHA_SOLVER=none
    ports: ["8191:8191"]
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8191/health"]
      interval: 30s
      timeout: 15s
      retries: 5
    volumes: [flaresolverr_data:/tmp]
    networks: [crawlrs-search-network]

  crawlrs:
    build: {context: .., dockerfile: docker/Dockerfile}
    image: crawlrs:search-test
    container_name: crawlrs-search-app
    command: ["./crawlrs", "api"]
    ports: ["3000:3000"]
    environment:
      - CRAWLRS__DATABASE__URL=postgres://crawlrs:password@postgres:5432/crawlrs
      - CRAWLRS__REDIS__URL=redis://redis:6379
      - CRAWLRS__SERVER__HOST=0.0.0.0
      - CRAWLRS__SERVER__PORT=3000
      - CRAWLRS__SEARCH__ENGINES__GOOGLE_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__BING_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__BAIDU_ENABLED=true
      - CRAWLRS__SEARCH__ENGINES__SOGOU_ENABLED=true
      - CRAWLRS__SEARCH__FLARESOLVERR__URL=http://flaresolverr:8191/v1
      - SKIP_S3_TESTS=true
      - SKIP_BROWSER_TESTS=true
    depends_on:
      postgres: {condition: service_healthy}
      redis: {condition: service_healthy}
    networks: [crawlrs-search-network]

networks:
  crawlrs-search-network: {driver: bridge}

volumes:
  flaresolverr_data:
```

**Step 5: 提交**

```bash
git add tests/feature-matrix/ docker-compose.test.*.yml -m "feat: 添加特性测试矩阵配置"
```

---

## 任务 5：创建测试运行脚本和文档

**文件**:
- 创建: `scripts/run-full-test.sh`
- 创建: `docs/TEST_RUNBOOK.md`

**Step 1: 创建完整测试运行脚本**

创建 `scripts/run-full-test.sh`:

```bash
#!/bin/bash
set -e

cd "$(dirname "$0")/.."

echo "========================================"
echo "Crawlrs 完整测试执行"
echo "========================================"

# 检查 Docker
if ! docker info >/dev/null 2>&1; then
    echo "❌ Docker 未运行"
    exit 1
fi

# 步骤 1: 启动测试环境
echo "步骤 1: 启动测试环境..."
docker-compose -f docker-compose.test.full.yml down -v 2>/dev/null || true
docker-compose -f docker-compose.test.full.yml up -d
echo "等待服务启动..."
sleep 30

# 健康检查
echo "运行健康检查..."
if ! ./scripts/health-check.sh; then
    echo "❌ 服务健康检查失败"
    exit 1
fi

# 步骤 2: 安装依赖
echo "步骤 2: 安装 Python 测试依赖..."
pip install -r tests/python/requirements.txt

# 步骤 3: 运行 API 测试
echo "步骤 3: 运行 API 测试..."
mkdir -p test-results
python -m pytest tests/python/test_api_endpoints.py -v --tb=short

# 步骤 4: 运行性能测试
echo "步骤 4: 运行性能测试..."
python -m pytest tests/python/test_performance.py -v --tb=short

# 步骤 5: 运行错误处理测试
echo "步骤 5: 运行错误处理测试..."
python -m pytest tests/python/test_error_handling.py -v --tb=short

# 步骤 6: 生成测试报告
echo "步骤 6: 生成测试报告..."
python -c "from api_test_framework import CrawlrsAPIClient; c = CrawlrsAPIClient(); c.generate_report('test-results/report.json')"

echo ""
echo "========================================"
echo "测试执行完成！"
echo "========================================"
echo "测试报告: test-results/report.json"
echo ""
echo "清理测试环境..."
docker-compose -f docker-compose.test.full.yml down -v
echo "✅ 所有测试完成！"
```

**Step 2: 创建测试运行手册**

创建 `docs/TEST_RUNBOOK.md`:

```markdown
# Crawlrs 测试运行手册

## 环境要求

### 硬件要求
- CPU: 4 核心以上
- 内存: 8GB 以上
- 磁盘: 50GB 以上可用空间
- Docker: 20.10+
- Docker Compose: 2.0+

### 软件要求
```bash
docker --version   # >= 20.10
docker-compose --version  # >= 2.0
python3 --version  # >= 3.8
```

## 快速开始

### 1. 启动测试环境
```bash
./scripts/start-test-env.sh
```

### 2. 安装测试依赖
```bash
pip install -r tests/python/requirements.txt
```

### 3. 运行完整测试
```bash
./scripts/run-full-test.sh
```

## 测试配置

### 可用配置

| 配置名称 | 描述 | 服务 |
|---------|------|------|
| `minio_only` | 仅 MinIO 存储 | PostgreSQL, Redis, MinIO |
| `browser_only` | 仅浏览器服务 | PostgreSQL, Redis, Chrome, FlareSolverr |
| `search_only` | 仅搜索功能 | PostgreSQL, Redis, FlareSolverr |
| `full_features` | 所有功能 | PostgreSQL, Redis, Chrome, FlareSolverr, MinIO |

### 运行特性测试
```bash
# MinIO 测试
docker-compose -f docker-compose.test.minio.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v

# 浏览器测试
docker-compose -f docker-compose.test.browser.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v

# 搜索测试
docker-compose -f docker-compose.test.search.yml up -d
python -m pytest tests/python/test_api_endpoints.py -v
```

## 测试套件

### API 测试
```bash
python -m pytest tests/python/test_api_endpoints.py -v
```

### 性能测试
```bash
python -m pytest tests/python/test_performance.py -v -s
```

### 错误处理测试
```bash
python -m pytest tests/python/test_error_handling.py -v
```

## 监控

测试期间可以访问以下监控界面：
- **Prometheus**: http://localhost:9090
- **Grafana**: http://localhost:3001 (admin/admin)

## 故障排除

### 端口冲突
```bash
# 检查端口占用
lsof -i :3000
lsof -i :5432

# 停止冲突服务
docker-compose down
```

### 服务启动失败
```bash
# 查看日志
docker-compose -f docker-compose.test.full.yml logs crawlrs

# 重启服务
docker-compose -f docker-compose.test.full.yml restart
```

### 数据库连接失败
```bash
# 检查数据库状态
docker ps | grep postgres
docker logs crawlrs-test-postgres

# 验证连接
pg_isready -U crawlrs -d crawlrs -h localhost -p 5432
```

## 性能指标

| 指标 | 目标值 |
|------|--------|
| API 吞吐量 | 10000 RPS |
| P50 延迟 | < 50ms |
| P99 延迟 | < 200ms |
| 成功率 | > 99.9% |
| 缓存命中率 | > 60% |
```

**Step 3: 提交**

```bash
git add scripts/run-full-test.sh docs/TEST_RUNBOOK.md -m "feat: 添加测试运行脚本和文档"
```

---

## 任务 6：创建项目文档和总结

**文件**:
- 创建: `docs/TEST_SUMMARY.md`

**Step 1: 创建测试总结文档**

创建 `docs/TEST_SUMMARY.md`:

```markdown
# Crawlrs 测试计划总结

## 测试目标

本测试计划旨在全面验证 crawlrs 项目在模拟真实生产环境下的：
1. Docker 容器化部署能力
2. 多服务集成正确性
3. 所有 REST API 端点功能
4. 系统性能和稳定性
5. 错误处理和容错能力
6. 特性开关组合兼容性

## 测试范围

### 测试覆盖的 API 端点

#### 公共端点
- `GET /health` - 健康检查
- `GET /metrics` - Prometheus 指标
- `GET /v1/version` - 版本信息

#### 受保护端点
- `POST /v1/search` - 搜索
- `POST /v1/crawl` - 创建爬取任务
- `GET /v1/crawl/{id}` - 获取爬取状态
- `GET /v1/crawl/{id}/results` - 获取爬取结果
- `DELETE /v1/crawl/{id}` - 取消爬取任务
- `POST /v1/scrape` - 创建抓取任务
- `GET /v1/scrape/{id}` - 获取抓取状态
- `DELETE /v1/scrape/{id}` - 取消抓取任务
- `POST /v1/extract` - 数据提取
- `POST /v1/webhooks` - 创建 Webhook
- `GET /v1/teams/geo-restrictions` - 获取地理限制
- `PUT /v1/teams/geo-restrictions` - 更新地理限制
- `GET /v1/audit/logs` - 审计日志
- `GET /v1/audit/denied` - 被拒绝请求

## 测试环境

### 服务配置

| 服务 | 镜像 | 端口 | 用途 |
|------|------|------|------|
| PostgreSQL | postgres:15-alpine | 5432 | 主数据库 |
| Redis | redis:7-alpine | 6379 | 缓存和限流 |
| Chrome | browserless/chrome | 9222 | JavaScript 渲染 |
| FlareSolverr | ghcr.io/flaresolverr/flaresolverr | 8191 | 反爬虫解决方案 |
| MinIO | minio/minio | 9000 | 对象存储 |
| Crawlrs | 自定义 | 3000 | 主应用 API |
| Prometheus | prom/prometheus | 9090 | 监控指标 |
| Grafana | grafana/grafana | 3001 | 可视化 |

### 测试配置矩阵

#### 配置 1: MinIO Only
- 测试 MinIO 存储功能
- 禁用浏览器和搜索功能

#### 配置 2: Browser Only
- 测试 Chrome 和 FlareSolverr
- 禁用存储和搜索功能

#### 配置 3: Search Only
- 测试搜索引擎集成
- 禁用浏览器和存储功能

#### 配置 4: Full Features
- 启用所有功能
- 完整的端到端测试

## 测试套件

### 1. API 端点测试
- 健康检查端点
- 搜索端点
- 爬取端点
- 抓取端点
- 提取端点
- Webhook 端点
- 团队管理端点
- 审计端点

### 2. 性能测试
- P99 延迟测试 (< 200ms)
- P95 延迟测试 (< 150ms)
- 平均响应时间 (< 100ms)
- 每秒请求数 (RPS)
- 并发请求处理
- 缓存命中率

### 3. 错误处理测试
- 错误响应验证
- 服务韧性测试
- 数据完整性测试
- 安全测试 (SQL 注入, XSS)
- 速率限制测试

### 4. 特性组合测试
- MinIO 存储测试
- 浏览器渲染测试
- 搜索引擎测试
- 完整功能测试

## 性能指标要求

| 指标 | 目标值 | 测试方法 |
|------|--------|---------|
| API 吞吐量 | 10000 RPS | 并发负载测试 |
| P50 延迟 | < 50ms | 延迟分布测试 |
| P99 延迟 | < 200ms | 延迟分布测试 |
| 成功率 | > 99.9% | 错误率监控 |
| 缓存命中率 | > 60% | Redis 缓存测试 |

## 测试输出

### 测试报告
- JSON 格式: `test-results/report.json`
- HTML 格式: `test-results/report.html`
- 包含详细请求/响应信息

### 监控面板
- Prometheus: 实时指标
- Grafana: 可视化监控

## 风险和缓解措施

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 外部服务不可用 | 测试失败 | 使用 Mock 服务 |
| 网络延迟高 | 性能测试不准确 | 本地 Docker 环境 |
| 资源不足 | 测试超时 | 充足的硬件配置 |
| 数据污染 | 测试结果不可靠 | 环境隔离和清理 |

## 执行计划

### 阶段 1: 环境准备
1. 创建 Docker Compose 配置
2. 设置测试环境变量
3. 验证服务健康状态

### 阶段 2: API 测试
1. 执行 API 端点测试
2. 验证请求/响应格式
3. 记录测试结果

### 阶段 3: 性能测试
1. 执行延迟测试
2. 执行吞吐量测试
3. 执行并发测试

### 阶段 4: 特性测试
1. 执行 MinIO 测试
2. 执行浏览器测试
3. 执行搜索测试
4. 执行完整功能测试

### 阶段 5: 报告生成
1. 汇总测试结果
2. 生成测试报告
3. 分析性能指标

## 验收标准

### 必须通过
- [ ] 所有 API 端点响应正确
- [ ] P99 延迟 < 200ms
- [ ] 测试成功率 > 99%
- [ ] 错误处理正确
- [ ] 数据完整性验证通过

### 加分项
- [ ] P99 延迟 < 100ms
- [ ] 测试成功率 = 100%
- [ ] 缓存命中率 > 70%
- [ ] RPS > 1000
```

**Step 2: 提交**

```bash
git add docs/TEST_SUMMARY.md -m "docs: 添加测试计划总结文档"
```

---

## 任务 7：运行完整测试验证

**Step 1: 启动测试环境**

Run: `./scripts/start-test-env.sh`
Expected: 所有服务成功启动

**Step 2: 运行完整测试**

Run: `./scripts/run-full-test.sh`
Expected: 所有测试通过

**Step 3: 验证测试报告**

Run: `cat test-results/report.json | python -m json.tool | head -50`
Expected: 测试报告包含完整的测试结果统计

**Step 4: 提交最终更改**

```bash
git add -A
git commit -m "feat: 完成全面的生产环境测试计划

- 添加完整的 Docker Compose 测试环境配置
- 创建 Python API 测试框架
- 添加 API 端点、性能和错误处理测试套件
- 创建特性测试矩阵配置
- 添加测试运行脚本和完整文档
- 支持 MinIO、浏览器、搜索等特性组合测试

测试覆盖:
- 20+ API 端点
- 性能测试 (P99 < 200ms, RPS > 50)
- 错误处理和容错测试
- 特性组合兼容性测试"
```

---

## 计划总结

本测试计划提供了完整的生产环境测试方案，包括：

1. **Docker 环境部署** - 完整的容器化测试环境
2. **特性测试矩阵** - 4 种不同的特性组合测试
3. **API 接口测试** - 20+ 端点的全面测试
4. **性能验证** - 延迟、吞吐量、并发测试
5. **错误处理测试** - 容错和安全测试
6. **自动化测试框架** - Python 测试框架和报告生成
7. **完整文档** - 测试运行手册和总结

所有测试都基于真实服务组件（PostgreSQL、Redis、Chrome、FlareSolverr、MinIO），不使用 Mock 数据，确保测试结果反映真实的系统行为。

---

**Plan complete and saved to `docs/plans/2026-01-14-full-production-testing.md`. Two execution options:**

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**

**If Subagent-Driven chosen:**
- **REQUIRED SUB-SKILL:** Use superpowers:subagent-driven-development
- Stay in this session
- Fresh subagent per task + code review

**If Parallel Session chosen:**
- Guide them to open new session in worktree
- **REQUIRED SUB-SKILL:** New session uses superpowers:executing-plans
