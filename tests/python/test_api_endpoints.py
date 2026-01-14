#!/usr/bin/env python3
"""
API 端点测试套件

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
        result = api_client.search(query="test", engines=["bing"], limit=5)
        assert result.success, f"搜索失败: {result.error_message}"
        assert result.response.status_code == 200

        response_data = result.response.body
        assert "success" in response_data
        assert response_data["success"] == True
        assert "data" in response_data
        assert "results" in response_data["data"]

    def test_search_with_multiple_engines(self, api_client):
        """测试多引擎搜索"""
        result = api_client.search(
            query="python programming", engines=["bing", "google"], limit=10
        )
        assert result.success, f"多引擎搜索失败: {result.error_message}"

        response_data = result.response.body
        assert "data" in response_data
        assert "results" in response_data["data"]

    def test_search_with_sync_wait(self, api_client):
        """测试同步等待功能"""
        result = api_client.search(
            query="web scraping", engines=["bing"], limit=5, sync_wait_ms=3000
        )
        assert result.success, f"同步等待搜索失败: {result.error_message}"

        assert result.response.elapsed_time_ms >= 2500


class TestCrawlEndpoints:
    """爬取端点测试"""

    def test_create_crawl(self, api_client, crawl_test_params):
        """测试创建爬取任务"""
        result = api_client.crawl(
            url=crawl_test_params["url"],
            options=crawl_test_params["options"],
            sync_wait_ms=5000,
        )
        assert result.success, f"创建爬取任务失败: {result.error_message}"
        assert result.response.status_code in [200, 202]

        response_data = result.response.body
        assert "success" in response_data
        assert "data" in response_data
        assert "id" in response_data["data"]

    def test_get_crawl_status(self, api_client):
        """测试获取爬取状态"""
        crawl_result = api_client.crawl(url="https://example.com", sync_wait_ms=5000)
        assert crawl_result.success

        crawl_id = crawl_result.response.body.get("data", {}).get("id")

        if crawl_id:
            status_result = api_client.get_crawl_status(crawl_id)
            assert status_result.success, (
                f"获取爬取状态失败: {status_result.error_message}"
            )

            response_data = status_result.response.body
            assert "data" in response_data
            assert "status" in response_data["data"]

    def test_get_crawl_results(self, api_client):
        """测试获取爬取结果"""
        crawl_result = api_client.crawl(url="https://example.com", sync_wait_ms=10000)
        assert crawl_result.success

        crawl_id = crawl_result.response.body.get("data", {}).get("id")

        if crawl_id:
            results_result = api_client.get_crawl_results(crawl_id)
            assert results_result.success, (
                f"获取爬取结果失败: {results_result.error_message}"
            )

            response_data = results_result.response.body
            assert "data" in response_data
            assert "results" in response_data["data"]

    def test_cancel_crawl(self, api_client):
        """测试取消爬取任务"""
        crawl_result = api_client.crawl(url="https://example.com", sync_wait_ms=60000)
        assert crawl_result.success

        crawl_id = crawl_result.response.body.get("data", {}).get("id")

        if crawl_id:
            cancel_result = api_client.cancel_crawl(crawl_id)
            assert cancel_result.response.status_code in [200, 404, 409]


class TestScrapeEndpoints:
    """抓取端点测试"""

    def test_create_scrape(self, api_client, scrape_test_params):
        """测试创建抓取任务"""
        result = api_client.scrape(
            url=scrape_test_params["url"],
            options=scrape_test_params["options"],
            sync_wait_ms=5000,
        )
        assert result.success, f"创建抓取任务失败: {result.error_message}"
        assert result.response.status_code in [200, 202]

        response_data = result.response.body
        assert "success" in response_data
        assert "data" in response_data
        assert "id" in response_data["data"]

    def test_get_scrape_status(self, api_client):
        """测试获取抓取状态"""
        scrape_result = api_client.scrape(url="https://example.com", sync_wait_ms=5000)
        assert scrape_result.success

        scrape_id = scrape_result.response.body.get("data", {}).get("id")

        if scrape_id:
            status_result = api_client.get_scrape_status(scrape_id)
            assert status_result.success, (
                f"获取抓取状态失败: {status_result.error_message}"
            )

            response_data = status_result.response.body
            assert "data" in response_data
            assert "status" in response_data["data"]

    def test_cancel_scrape(self, api_client):
        """测试取消抓取任务"""
        scrape_result = api_client.scrape(url="https://example.com", sync_wait_ms=60000)
        assert scrape_result.success

        scrape_id = scrape_result.response.body.get("data", {}).get("id")

        if scrape_id:
            cancel_result = api_client.cancel_scrape(scrape_id)
            assert cancel_result.response.status_code in [200, 404, 409]


class TestExtractEndpoints:
    """提取端点测试"""

    def test_extract_content(self, api_client, extract_test_params):
        """测试内容提取功能"""
        result = api_client.extract(
            url=extract_test_params["url"],
            selectors=extract_test_params["selectors"],
            sync_wait_ms=5000,
        )
        assert result.success, f"内容提取失败: {result.error_message}"
        assert result.response.status_code == 200

        response_data = result.response.body
        assert "success" in response_data
        assert "data" in response_data


class TestWebhookEndpoints:
    """Webhook 端点测试"""

    def test_create_webhook(self, api_client, webhook_test_params):
        """测试创建 Webhook"""
        result = api_client.create_webhook(
            url=webhook_test_params["url"],
            events=webhook_test_params["events"],
            secret=webhook_test_params.get("secret"),
        )
        assert result.success, f"创建 Webhook 失败: {result.error_message}"
        assert result.response.status_code in [200, 201]

        response_data = result.response.body
        assert "success" in response_data
        assert "data" in response_data
        assert "id" in response_data["data"]


class TestTeamEndpoints:
    """团队管理端点测试"""

    def test_get_team_geo_restrictions(self, api_client):
        """测试获取团队地理限制"""
        result = api_client.get_team_geo_restrictions()
        assert result.response.status_code in [200, 403]

    def test_update_team_geo_restrictions(self, api_client):
        """测试更新团队地理限制"""
        restrictions = [
            {"country": "US", "allowed": True},
            {"country": "CN", "allowed": False},
        ]
        result = api_client.update_team_geo_restrictions(restrictions)
        assert result.response.status_code in [200, 403]


class TestAuditEndpoints:
    """审计端点测试"""

    def test_get_audit_logs(self, api_client):
        """测试获取审计日志"""
        result = api_client.get_audit_logs(limit=10)
        assert result.response.status_code in [200, 403]

    def test_get_denied_requests(self, api_client):
        """测试获取被拒绝的请求"""
        result = api_client.get_denied_requests()
        assert result.response.status_code in [200, 403]


class TestRateLimiting:
    """速率限制测试"""

    def test_rate_limit_headers(self, api_client):
        """测试速率限制响应头"""
        results = []
        for i in range(5):
            result = api_client.health_check()
            results.append(result)
            time.sleep(0.1)

        success_count = sum(1 for r in results if r.success)
        assert success_count >= 4


class TestCircuitBreaker:
    """断路器测试"""

    def test_service_unavailable_response(self, api_client):
        """测试服务不可用时的响应"""
        result = api_client.search(query="test", engines=["nonexistent"])
        assert result.response.status_code in [400, 404, 500, 503]


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
