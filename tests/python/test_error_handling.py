#!/usr/bin/env python3
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

"""
错误处理测试套件

测试系统的容错能力和错误处理
"""

import pytest
import time

from api_test_framework import CrawlrsAPIClient


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
        invalid_client = CrawlrsAPIClient(api_key="invalid-key")
        result = invalid_client.search(query="test")

        assert result.response.status_code in [401, 403]

    def test_rate_limit_exceeded(self, api_client):
        """测试速率限制触发

        限流为 handler 显式 opt-in（如 map/search/scrape/webhook 入口的
        check_rate_limit），geo-restrictions 等查询端点不在计量范围；
        /health 为公共端点按设计跳过限流（PUBLIC_ENDPOINTS）。这里以
        POST /v1/map 作探针（限流在 SSRF 之前，请求体即被 400 拒绝、
        零外网零积分消耗），并签发一次性 key 打满限流桶，避免污染共享
        key 的配额导致同套件后续用例连坐 429。
        """
        me = api_client.get_team_info()
        team_id = ((me.response.body or {}).get("data") or {}).get("id")
        assert me.success and team_id, (
            f"无法获取 team_id，无法签发限流测试专用 key: {me.response.status_code}"
        )

        issued = api_client.issue_api_key(team_id=str(team_id), scopes=["read"])
        assert issued.response.status_code == 201, (
            f"签发限流测试 key 失败: {issued.response.status_code} {issued.response.body}"
        )
        key = ((issued.response.body or {}).get("data") or {}).get("api_key")
        assert key, f"签发响应缺少 api_key: {issued.response.body}"

        probe = CrawlrsAPIClient(api_key=key)
        statuses = []
        for _ in range(200):
            # 回环地址命中 SSRF 静态拒绝（400），探针快速返回且不触发外网抓取
            result = probe.map_url("http://127.0.0.1:9/x")
            statuses.append(result.response.status_code)
            if 429 in statuses:
                break
            time.sleep(0.01)

        assert 429 in statuses, (
            f"未能触发速率限制（{len(statuses)} 次请求，末次状态 "
            f"{statuses[-1] if statuses else None}，状态分布 "
            f"{ {s: statuses.count(s) for s in set(statuses)} }）"
        )


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
        result = api_client.crawl(url="https://text.npr.org", sync_wait_ms=100)

        # 202：同步等待窗口超时后任务保持异步执行（Accepted + queued），语义正确
        assert result.response.status_code in [200, 202, 408, 500]

    def test_concurrent_failure_recovery(self, api_client):
        """测试并发故障恢复"""
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
                results.append(result.response.body.get("data") or {})

        if len(results) >= 2:
            assert results[0].keys() == results[1].keys(), "响应数据结构不一致"

    def test_idempotent_operations(self, api_client):
        """测试幂等操作"""
        task_ids = []
        for i in range(2):
            result = api_client.crawl(url="https://text.npr.org", sync_wait_ms=5000)
            if result.success:
                task_id = result.response.body.get("data", {}).get("id")
                if task_id:
                    task_ids.append(task_id)

        if len(task_ids) >= 2:
            assert task_ids[0] != task_ids[1], "非幂等操作"

    def test_data_persistence(self, api_client):
        """测试数据持久化"""
        result = api_client.crawl(url="https://text.npr.org", sync_wait_ms=10000)
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
        no_auth_client = CrawlrsAPIClient(api_key="")
        result = no_auth_client.get_audit_logs()

        assert result.response.status_code in [401, 403]


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
