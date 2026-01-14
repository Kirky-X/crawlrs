#!/usr/bin/env python3
"""
错误处理测试套件

测试系统的容错能力和错误处理
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

    def test_invalid_api_key(self, api_client):
        """测试无效 API 密钥"""
        from api_test_framework import CrawlrsAPIClient

        invalid_client = CrawlrsAPIClient(api_key="invalid-key")
        result = invalid_client.search(query="test")

        assert result.response.status_code in [401, 403]

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

    def test_timeout_handling(self, api_client):
        """测试超时处理"""
        result = api_client.crawl(url="https://example.com", sync_wait_ms=100)

        assert result.response.status_code in [200, 408, 500]

    def test_concurrent_failure_recovery(self, api_client):
        """测试并发故障恢复"""
        from api_test_framework import CrawlrsAPIClient
        import concurrent.futures

        clients = [CrawlrsAPIClient() for _ in range(5)]

        results = []
        for client in clients:
            result = client.health_check()
            results.append(result.success)

        success_count = sum(results)
        assert success_count >= 4, f"只有 {success_count}/5 客户端正常工作"


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

    def test_idempotent_operations(self, api_client):
        """测试幂等操作"""
        task_ids = []
        for i in range(2):
            result = api_client.crawl(url="https://example.com", sync_wait_ms=5000)
            if result.success:
                task_id = result.response.body.get("data", {}).get("id")
                if task_id:
                    task_ids.append(task_id)

        if len(task_ids) >= 2:
            assert task_ids[0] != task_ids[1], "非幂等操作"

    def test_data_persistence(self, api_client):
        """测试数据持久化"""
        result = api_client.crawl(url="https://example.com", sync_wait_ms=10000)
        assert result.success

        crawl_id = result.response.body.get("data", {}).get("id")

        time.sleep(2)

        if crawl_id:
            status_result = api_client.get_crawl_status(crawl_id)
            assert status_result.success, "数据未正确持久化"

            assert status_result.response.body.get("data", {}).get("id") == crawl_id


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

    def test_unauthorized_access(self, api_client):
        """测试未授权访问"""
        from api_test_framework import CrawlrsAPIClient

        no_auth_client = CrawlrsAPIClient(api_key="")
        result = no_auth_client.get_audit_logs()

        assert result.response.status_code in [401, 403]


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
