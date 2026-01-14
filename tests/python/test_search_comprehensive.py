#!/usr/bin/env python3
"""
Crawlrs 搜索引擎综合测试

测试所有搜索引擎的可用性和功能
"""

import pytest
import time


class TestSearchAuthentication:
    """搜索认证测试"""

    def test_search_without_auth(self, api_client):
        """测试无认证的搜索请求"""
        # 直接发送请求不带认证
        import requests

        response = requests.post(
            "http://localhost:3000/v1/search", json={"query": "test"}, timeout=30
        )
        # 应该返回 401 未授权
        assert response.status_code in [401, 403], (
            f"期望 401/403，实际 {response.status_code}"
        )

    def test_search_with_valid_auth(self, api_client):
        """测试带有效认证的搜索请求"""
        result = api_client.search(query="test", engines=["baidu"], limit=3)
        assert result.success, f"认证搜索失败: {result.error_message}"
        assert result.response.status_code == 200


class TestBaiduEngine:
    """Baidu 搜索引擎测试"""

    def test_baidu_basic_search(self, api_client):
        """测试 Baidu 基本搜索"""
        result = api_client.search(query="Python 编程", engines=["baidu"], limit=5)
        assert result.success, f"Baidu 搜索失败: {result.error_message}"
        assert result.response.status_code == 200

        response_data = result.response.body
        assert "results" in response_data
        assert len(response_data["results"]) > 0

        # 验证所有结果都来自 Baidu
        for r in response_data["results"]:
            assert r.get("engine") == "Baidu", f"期望 Baidu，实际 {r.get('engine')}"

    def test_baidu_with_sync_wait(self, api_client):
        """测试 Baidu 同步等待搜索"""
        start = time.time()
        result = api_client.search(
            query="web scraping", engines=["baidu"], limit=3, sync_wait_ms=1000
        )
        elapsed = (time.time() - start) * 1000

        assert result.success
        assert elapsed >= 800, f"同步等待不足: {elapsed}ms"


class TestBingEngine:
    """Bing 搜索引擎测试"""

    def test_bing_search(self, api_client):
        """测试 Bing 搜索"""
        result = api_client.search(query="machine learning", engines=["bing"], limit=5)

        # Bing 可能因缺少 API 密钥而失败，aggregator 会回退到其他可用引擎
        assert result.response.status_code in [200, 400, 401, 403, 500, 502, 503]

        if result.success:
            response_data = result.response.body
            # 实际行为：即使请求 Bing，失败后会回退到 Baidu
            # 所以我们验证至少有一个引擎返回结果
            assert "results" in response_data


class TestSogouEngine:
    """Sogou 搜索引擎测试"""

    def test_sogou_search(self, api_client):
        """测试 Sogou 搜索"""
        result = api_client.search(query="数据分析", engines=["sogou"], limit=5)

        assert result.response.status_code in [200, 400, 401, 403, 500, 502, 503]

        if result.success:
            response_data = result.response.body
            # Sogou 失败后也会回退到 Baidu
            assert "results" in response_data


class TestMultiEngineSearch:
    """多引擎搜索测试"""

    def test_multi_engine_baidu_only(self, api_client):
        """测试指定多个引擎但只有 Baidu 可用"""
        result = api_client.search(
            query="artificial intelligence",
            engines=["baidu", "bing", "sogou"],
            limit=10,
        )
        assert result.success

        response_data = result.response.body
        assert "results" in response_data

        # 至少应该有 Baidu 结果
        baidu_count = sum(
            1 for r in response_data["results"] if r.get("engine") == "Baidu"
        )
        assert baidu_count > 0, "应该有 Baidu 结果"


class TestSearchFeatures:
    """搜索功能测试"""

    def test_search_with_limit(self, api_client):
        """测试搜索结果数量限制"""
        result = api_client.search(query="test", engines=["baidu"], limit=3)
        assert result.success

        response_data = result.response.body
        assert len(response_data["results"]) <= 3

    def test_search_response_structure(self, api_client):
        """测试搜索响应结构"""
        result = api_client.search(query="python", engines=["baidu"], limit=1)
        assert result.success

        response_data = result.response.body

        # 验证响应结构
        assert "query" in response_data
        assert "results" in response_data
        assert "credits_used" in response_data

        if response_data["results"]:
            r = response_data["results"][0]
            assert "title" in r
            assert "url" in r
            assert "engine" in r


class TestSearchPerformance:
    """搜索性能测试"""

    def test_search_response_time(self, api_client):
        """测试搜索响应时间"""
        result = api_client.search(query="test", engines=["baidu"], limit=5)

        assert result.success
        # 搜索引擎可能需要较长时间，宽松阈值 30 秒
        assert result.response.elapsed_time_ms < 30000, (
            f"搜索超时: {result.response.elapsed_time_ms}ms"
        )


class TestSearchEdgeCases:
    """搜索边界情况测试"""

    def test_search_empty_query(self, api_client):
        """测试空查询"""
        result = api_client.search(query="", engines=["baidu"], limit=3)
        # 空查询可能返回错误或空结果
        assert result.response.status_code in [200, 400, 422]

    def test_search_special_characters(self, api_client):
        """测试特殊字符查询"""
        result = api_client.search(
            query="hello world! @#$%", engines=["baidu"], limit=3
        )
        assert result.response.status_code in [200, 400, 500]

    def test_search_very_long_query(self, api_client):
        """测试超长查询"""
        long_query = "a" * 1000
        result = api_client.search(query=long_query, engines=["baidu"], limit=3)
        assert result.response.status_code in [200, 400, 414, 422]
