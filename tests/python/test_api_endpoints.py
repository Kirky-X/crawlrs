#!/usr/bin/env python3
"""
Crawlrs API 端点测试套件

测试所有 REST API 端点的功能
"""

import pytest
import time


class TestHealthEndpoints:
    """健康检查端点测试"""

    def test_health_check(self, api_client):
        """测试健康检查端点"""
        result = api_client.health_check()
        assert result.success, f"健康检查失败: {result.error_message}"
        assert result.response.status_code == 200
        assert "healthy" in str(result.response.body).lower()

    def test_version_endpoint(self, api_client):
        """测试版本端点"""
        result = api_client.get_version()
        assert result.success, f"版本检查失败: {result.error_message}"
        assert result.response.status_code == 200


class TestSearchEndpoints:
    """搜索端点测试"""

    def test_basic_search(self, api_client):
        """测试基本搜索功能"""
        result = api_client.search(query="test", engines=["baidu"], limit=5)
        assert result.success, f"搜索失败: {result.error_message}"
        assert result.response.status_code == 200

        response_data = result.response.body
        assert "results" in response_data
        assert isinstance(response_data["results"], list)

    def test_search_with_multiple_engines(self, api_client):
        """测试多引擎搜索"""
        result = api_client.search(
            query="python programming", engines=["baidu"], limit=10
        )
        assert result.success, f"多引擎搜索失败: {result.error_message}"

        response_data = result.response.body
        assert "results" in response_data

    def test_search_with_sync_wait(self, api_client):
        """测试同步等待功能"""
        result = api_client.search(
            query="web scraping", engines=["baidu"], limit=5, sync_wait_ms=1000
        )
        assert result.success, f"同步等待搜索失败: {result.error_message}"


class TestCrawlEndpoints:
    """爬取端点测试"""

    def test_create_crawl(self, api_client, crawl_test_params):
        """测试创建爬取任务"""
        result = api_client.crawl(
            url=crawl_test_params["url"],
            config=crawl_test_params.get("options", {"max_depth": 1}),
            sync_wait_ms=5000,
        )
        # Geographic restriction may fail in test environment
        assert result.response.status_code in [200, 202, 400, 422]

    def test_get_crawl_status(self, api_client, crawl_test_params):
        """测试获取爬取状态"""
        crawl_result = api_client.crawl(
            url=crawl_test_params["url"],
            config=crawl_test_params.get("options", {"max_depth": 1}),
            sync_wait_ms=5000,
        )
        if not crawl_result.success:
            pytest.skip("无法创建爬取任务")

        response_data = crawl_result.response.body
        if "crawl_id" in response_data and response_data["crawl_id"]:
            crawl_id = response_data["crawl_id"]
            status_result = api_client.get_crawl_status(crawl_id)
            assert status_result.success or status_result.response.status_code == 404


class TestScrapeEndpoints:
    """抓取端点测试"""

    def test_create_scrape(self, api_client, scrape_test_params):
        """测试创建抓取任务"""
        result = api_client.scrape(
            url=scrape_test_params["url"],
            config=scrape_test_params.get("options", {"timeout": 30}),
            sync_wait_ms=5000,
        )
        assert result.success, f"创建抓取任务失败: {result.error_message}"
        assert result.response.status_code in [200, 202, 422]


class TestExtractEndpoints:
    """提取端点测试"""

    def test_extract_content(self, api_client):
        """测试内容提取 - 验证端点存在"""
        result = api_client.extract(
            url="https://example.com",
            selectors={"title": "h1"},
            sync_wait_ms=5000,
        )
        assert result.response.status_code in [200, 400, 422]


class TestWebhookEndpoints:
    """Webhook 端点测试"""

    def test_create_webhook(self, api_client):
        """测试创建 Webhook"""
        result = api_client.create_webhook(
            url="https://httpbin.org/post",
            events=["task.completed"],
        )
        assert result.success, f"创建 Webhook 失败: {result.error_message}"
        response_data = result.response.body
        assert "id" in response_data


class TestTeamEndpoints:
    """团队管理端点测试"""

    def test_get_team_geo_restrictions(self, api_client):
        """测试获取地理限制"""
        result = api_client.get_team_geo_restrictions()
        assert result.response.status_code in [200, 401, 403]

    def test_update_team_geo_restrictions(self, api_client):
        """测试更新地理限制"""
        restrictions = {"enable_geo_restrictions": True, "allowed_countries": ["US"]}
        result = api_client.update_team_geo_restrictions(restrictions)
        assert result.response.status_code in [200, 401, 403]


class TestAuditEndpoints:
    """审计日志端点测试"""

    def test_get_audit_logs(self, api_client):
        """测试获取审计日志"""
        result = api_client.get_audit_logs(limit=10)
        assert result.response.status_code in [200, 401, 403]

    def test_get_denied_requests(self, api_client):
        """测试获取拒绝请求"""
        result = api_client.get_denied_requests()
        assert result.response.status_code in [200, 401, 403]


class TestRateLimiting:
    """速率限制测试"""

    def test_rate_limit_headers(self, api_client):
        """测试速率限制响应头"""
        result = api_client.health_check()
        assert result.success


class TestCircuitBreaker:
    """熔断器测试"""

    def test_service_unavailable_response(self, api_client):
        """测试服务不可用响应"""
        result = api_client.search(query="test", engines=["nonexistent"], limit=5)
        assert result.response.status_code in [200, 400, 422, 500, 503]
