#!/usr/bin/env python3
"""
Crawlrs API 测试框架

提供完整的 API 测试能力，包括：
- HTTP 请求发送和响应记录
- 请求/响应详细信息日志
- 测试报告生成
- 性能指标统计
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

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)
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
    performance_metrics: Dict[str, Any] = field(default_factory=dict)


class CrawlrsAPIClient:
    """Crawlrs API 客户端"""

    def __init__(
        self, base_url: str = "http://localhost:3000", api_key: str = "test-api-key"
    ):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.session = requests.Session()
        self.session.headers.update(
            {
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
                "Accept": "application/json",
            }
        )
        self.test_results: List[TestResult] = []
        self.request_count = 0
        self.success_count = 0
        self.failure_count = 0

    def _send_request(
        self,
        method: HTTPMethod,
        endpoint: str,
        params: Optional[Dict] = None,
        body: Optional[Dict] = None,
        headers: Optional[Dict] = None,
    ) -> tuple:
        """发送 HTTP 请求并记录详细信息"""
        url = urljoin(self.base_url + "/", endpoint.lstrip("/"))

        request_headers = self.session.headers.copy()
        if headers:
            request_headers.update(headers)

        request_info = RequestInfo(
            method=method.value, url=url, headers=dict(request_headers), body=body
        )

        start_time = time.time()
        try:
            if method == HTTPMethod.GET:
                response = self.session.get(
                    url, params=params, headers=request_headers, timeout=30
                )
            elif method == HTTPMethod.POST:
                response = self.session.post(
                    url, params=params, json=body, headers=request_headers, timeout=30
                )
            elif method == HTTPMethod.PUT:
                response = self.session.put(
                    url, params=params, json=body, headers=request_headers, timeout=30
                )
            elif method == HTTPMethod.DELETE:
                response = self.session.delete(
                    url, params=params, headers=request_headers, timeout=30
                )
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
                elapsed_time_ms=elapsed_time,
            )

            self.request_count += 1

            return response_info, request_info, None

        except Exception as e:
            elapsed_time = (time.time() - start_time) * 1000
            error_info = ResponseInfo(
                status_code=0, headers={}, body=None, elapsed_time_ms=elapsed_time
            )
            return error_info, request_info, str(e)

    def _record_result(
        self,
        test_name: str,
        request: RequestInfo,
        response: ResponseInfo,
        error: Optional[str] = None,
    ) -> TestResult:
        """记录测试结果"""
        success = response.status_code in [200, 201, 202, 204] and error is None
        if success:
            self.success_count += 1
        else:
            self.failure_count += 1

        result = TestResult(
            test_name=test_name,
            request=request,
            response=response,
            success=success,
            error_message=error,
            performance_metrics={
                "response_time_ms": response.elapsed_time_ms,
                "status_code": response.status_code,
            },
        )
        self.test_results.append(result)
        return result

    def health_check(self) -> TestResult:
        """健康检查端点"""
        response, request, error = self._send_request(HTTPMethod.GET, "/health")
        return self._record_result("health_check", request, response, error)

    def get_version(self) -> TestResult:
        """获取版本信息"""
        response, request, error = self._send_request(HTTPMethod.GET, "/v1/version")
        return self._record_result("get_version", request, response, error)

    def search(
        self,
        query: str,
        engines: Optional[List[str]] = None,
        limit: int = 10,
        sync_wait_ms: Optional[int] = None,
    ) -> TestResult:
        """搜索接口"""
        body = {"query": query, "limit": limit}
        if engines:
            # API expects 'sources' or 'engine', not 'engines'
            if len(engines) == 1:
                body["engine"] = engines[0]
            else:
                body["sources"] = engines
        if sync_wait_ms:
            body["sync_wait_ms"] = sync_wait_ms

        response, request, error = self._send_request(
            HTTPMethod.POST, "/v1/search", body=body
        )
        return self._record_result(f"search_{query}", request, response, error)

    def crawl(
        self,
        url: str,
        config: Optional[Dict] = None,
        sync_wait_ms: Optional[int] = None,
    ) -> TestResult:
        """爬取接口"""
        body = {"url": url, "config": config or {"max_depth": 1}}
        if sync_wait_ms:
            body["sync_wait_ms"] = sync_wait_ms

        response, request, error = self._send_request(
            HTTPMethod.POST, "/v1/crawl", body=body
        )
        return self._record_result(f"crawl_{url}", request, response, error)

    def get_crawl_status(self, crawl_id: str) -> TestResult:
        """获取爬取状态"""
        response, request, error = self._send_request(
            HTTPMethod.GET, f"/v1/crawl/{crawl_id}"
        )
        return self._record_result(
            f"get_crawl_status_{crawl_id}", request, response, error
        )

    def get_crawl_results(self, crawl_id: str) -> TestResult:
        """获取爬取结果"""
        response, request, error = self._send_request(
            HTTPMethod.GET, f"/v1/crawl/{crawl_id}/results"
        )
        return self._record_result(
            f"get_crawl_results_{crawl_id}", request, response, error
        )

    def cancel_crawl(self, crawl_id: str) -> TestResult:
        """取消爬取任务"""
        response, request, error = self._send_request(
            HTTPMethod.DELETE, f"/v1/crawl/{crawl_id}"
        )
        return self._record_result(f"cancel_crawl_{crawl_id}", request, response, error)

    def scrape(
        self,
        url: str,
        config: Optional[Dict] = None,
        sync_wait_ms: Optional[int] = None,
    ) -> TestResult:
        """抓取接口"""
        # API expects options with: headers, wait_for, timeout, js_rendering, etc.
        body = {"url": url, "options": config or {"timeout": 30}}
        if sync_wait_ms:
            body["sync_wait_ms"] = sync_wait_ms

        response, request, error = self._send_request(
            HTTPMethod.POST, "/v1/scrape", body=body
        )
        return self._record_result(f"scrape_{url}", request, response, error)

    def get_scrape_status(self, scrape_id: str) -> TestResult:
        """获取抓取状态"""
        response, request, error = self._send_request(
            HTTPMethod.GET, f"/v1/scrape/{scrape_id}"
        )
        return self._record_result(
            f"get_scrape_status_{scrape_id}", request, response, error
        )

    def cancel_scrape(self, scrape_id: str) -> TestResult:
        """取消抓取任务"""
        response, request, error = self._send_request(
            HTTPMethod.DELETE, f"/v1/scrape/{scrape_id}"
        )
        return self._record_result(
            f"cancel_scrape_{scrape_id}", request, response, error
        )

    def extract(
        self, url: str, selectors: Dict[str, str], sync_wait_ms: Optional[int] = None
    ) -> TestResult:
        """提取接口"""
        body = {"url": url, "selectors": selectors}
        if sync_wait_ms:
            body["sync_wait_ms"] = sync_wait_ms

        response, request, error = self._send_request(
            HTTPMethod.POST, "/v1/extract", body=body
        )
        return self._record_result(f"extract_{url}", request, response, error)

    def create_webhook(
        self, url: str, events: List[str], secret: Optional[str] = None
    ) -> TestResult:
        """创建 Webhook"""
        body = {"url": url, "events": events}
        if secret:
            body["secret"] = secret

        response, request, error = self._send_request(
            HTTPMethod.POST, "/v1/webhooks", body=body
        )
        return self._record_result(f"create_webhook_{url}", request, response, error)

    def get_team_geo_restrictions(self) -> TestResult:
        """获取团队地理限制"""
        response, request, error = self._send_request(
            HTTPMethod.GET, "/v1/teams/geo-restrictions"
        )
        return self._record_result(
            "get_team_geo_restrictions", request, response, error
        )

    def update_team_geo_restrictions(self, restrictions: List[Dict]) -> TestResult:
        """更新团队地理限制"""
        response, request, error = self._send_request(
            HTTPMethod.PUT, "/v1/teams/geo-restrictions", body=restrictions
        )
        return self._record_result(
            "update_team_geo_restrictions", request, response, error
        )

    def get_audit_logs(self, limit: int = 100, offset: int = 0) -> TestResult:
        """获取审计日志"""
        params = {"limit": limit, "offset": offset}
        response, request, error = self._send_request(
            HTTPMethod.GET, "/v1/audit/logs", params=params
        )
        return self._record_result("get_audit_logs", request, response, error)

    def get_denied_requests(self) -> TestResult:
        """获取被拒绝的请求"""
        response, request, error = self._send_request(
            HTTPMethod.GET, "/v1/audit/denied"
        )
        return self._record_result("get_denied_requests", request, response, error)

    def generate_report(self, output_path: str = "test_report.json") -> Dict:
        """生成测试报告"""
        total = self.request_count
        success_rate = (self.success_count / total * 100) if total > 0 else 0

        response_times = [
            r.response.elapsed_time_ms
            for r in self.test_results
            if r.response.elapsed_time_ms > 0
        ]

        avg_response_time = (
            sum(response_times) / len(response_times) if response_times else 0
        )
        min_response_time = min(response_times) if response_times else 0
        max_response_time = max(response_times) if response_times else 0

        endpoint_stats = {}
        for result in self.test_results:
            endpoint = result.request.url.split("/")[-1] or "root"
            if endpoint not in endpoint_stats:
                endpoint_stats[endpoint] = {
                    "total": 0,
                    "success": 0,
                    "failure": 0,
                    "total_time_ms": 0,
                }
            endpoint_stats[endpoint]["total"] += 1
            if result.success:
                endpoint_stats[endpoint]["success"] += 1
            else:
                endpoint_stats[endpoint]["failure"] += 1
            endpoint_stats[endpoint]["total_time_ms"] += result.response.elapsed_time_ms

        report = {
            "summary": {
                "total_requests": total,
                "success_count": self.success_count,
                "failure_count": self.failure_count,
                "success_rate_percent": round(success_rate, 2),
                "timestamp": datetime.now().isoformat(),
            },
            "performance": {
                "average_response_time_ms": round(avg_response_time, 2),
                "min_response_time_ms": round(min_response_time, 2),
                "max_response_time_ms": round(max_response_time, 2),
                "total_test_duration_ms": sum(response_times),
            },
            "endpoint_statistics": {
                endpoint: {
                    "total": stats["total"],
                    "success": stats["success"],
                    "failure": stats["failure"],
                    "success_rate_percent": round(
                        stats["success"] / stats["total"] * 100, 2
                    )
                    if stats["total"] > 0
                    else 0,
                    "average_response_time_ms": round(
                        stats["total_time_ms"] / stats["total"], 2
                    ),
                }
                for endpoint, stats in endpoint_stats.items()
            },
            "detailed_results": [
                {
                    "test_name": r.test_name,
                    "endpoint": r.request.url,
                    "method": r.request.method,
                    "status_code": r.response.status_code,
                    "response_time_ms": round(r.response.elapsed_time_ms, 2),
                    "success": r.success,
                    "error": r.error_message,
                }
                for r in self.test_results
            ],
        }

        with open(output_path, "w", encoding="utf-8") as f:
            json.dump(report, f, ensure_ascii=False, indent=2)

        logger.info(f"测试报告已生成: {output_path}")
        return report

    def print_summary(self):
        """打印测试摘要"""
        print("\n" + "=" * 60)
        print("Crawlrs API 测试结果摘要")
        print("=" * 60)
        total = self.request_count
        success_count = self.success_count
        print(f"总请求数: {total}")
        print(f"成功: {success_count}")
        print(f"失败: {self.failure_count}")
        print(f"成功率: {success_count / total * 100:.2f}%" if total > 0 else "N/A")
        print("=" * 60 + "\n")


if __name__ == "__main__":
    client = CrawlrsAPIClient()
    client.health_check()
    client.get_version()
    report = client.generate_report()
    client.print_summary()
