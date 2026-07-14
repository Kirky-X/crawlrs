#!/usr/bin/env python3
"""
性能测试套件

验证系统性能指标是否满足 PRD 要求
"""

import pytest
import time
import statistics
from typing import List, Dict
import concurrent.futures


class TestPerformanceTargets:
    """性能目标测试"""

    @pytest.fixture
    def performance_client(self, api_client):
        """提供性能测试客户端"""
        return api_client

    def test_response_time_p99(self, performance_client):
        """测试 P99 延迟是否小于 200ms"""
        response_times = []

        for i in range(100):
            result = performance_client.health_check()
            if result.response.elapsed_time_ms > 0:
                response_times.append(result.response.elapsed_time_ms)
            time.sleep(0.05)

        response_times.sort()
        p99_index = int(len(response_times) * 0.99)
        p99_latency = response_times[p99_index]

        print(f"\nP99 延迟: {p99_latency}ms")
        assert p99_latency < 200, f"P99 延迟 {p99_latency}ms 超过 200ms 目标"

    def test_response_time_p95(self, performance_client):
        """测试 P95 延迟"""
        response_times = []

        for i in range(50):
            result = performance_client.health_check()
            if result.response.elapsed_time_ms > 0:
                response_times.append(result.response.elapsed_time_ms)
            time.sleep(0.05)

        response_times.sort()
        p95_index = int(len(response_times) * 0.95)
        p95_latency = response_times[p95_index]

        print(f"P95 延迟: {p95_latency}ms")
        assert p95_latency < 150, f"P95 延迟 {p95_latency}ms 超过 150ms 目标"

    def test_average_response_time(self, performance_client):
        """测试平均响应时间"""
        response_times = []

        for i in range(50):
            result = performance_client.health_check()
            if result.response.elapsed_time_ms > 0:
                response_times.append(result.response.elapsed_time_ms)
            time.sleep(0.05)

        avg_time = statistics.mean(response_times)
        print(f"平均响应时间: {avg_time}ms")
        assert avg_time < 100, f"平均响应时间 {avg_time}ms 超过 100ms 目标"


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
        results = []

        def make_request():
            result = api_client.health_check()
            return result.success, result.response.elapsed_time_ms

        with concurrent.futures.ThreadPoolExecutor(max_workers=20) as executor:
            futures = [executor.submit(make_request) for _ in range(20)]
            results = [f.result() for f in concurrent.futures.as_completed(futures)]

        success_count = sum(1 for success, _ in results if success)
        response_times = [time_ms for _, time_ms in results if time_ms > 0]

        print(f"\n并发请求成功: {success_count}/20")
        print(f"平均响应时间: {statistics.mean(response_times):.2f}ms")

        assert success_count >= 18, f"并发请求成功率 {success_count}/20 不达标"
        assert statistics.mean(response_times) < 500, "并发请求响应时间过长"


class TestDatabasePerformance:
    """数据库性能测试"""

    def test_database_connection_pool(self, api_client):
        """测试数据库连接池"""
        results = []

        def query():
            result = api_client.get_audit_logs(limit=10)
            return result.success, result.response.elapsed_time_ms

        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(query) for _ in range(10)]
            results = [f.result() for f in concurrent.futures.as_completed(futures)]

        success_count = sum(1 for success, _ in results if success)
        response_times = [time_ms for _, time_ms in results if time_ms > 0]

        print(f"\n数据库查询成功: {success_count}/10")
        print(f"平均查询时间: {statistics.mean(response_times):.2f}ms")

        assert success_count >= 8, "数据库连接池性能不达标"
        assert statistics.mean(response_times) < 300, "数据库查询时间过长"


class TestResourceUtilization:
    """资源利用测试"""

    def test_memory_usage(self, api_client):
        """测试内存使用情况"""
        for i in range(100):
            api_client.health_check()
            time.sleep(0.01)

        result = api_client.health_check()
        assert result.success, "服务在压力下无响应"
        assert result.response.elapsed_time_ms < 500, "响应时间在压力下过长"

    def test_cpu_usage_indicators(self, api_client):
        """CPU 使用指标"""
        start_time = time.time()
        response_times = []

        for i in range(50):
            start = time.time()
            api_client.health_check()
            elapsed = (time.time() - start) * 1000
            response_times.append(elapsed)
            time.sleep(0.01)

        total_time = time.time() - start_time
        avg_latency = statistics.mean(response_times)

        print(f"\n总耗时: {total_time:.2f}s")
        print(f"平均延迟: {avg_latency:.2f}ms")
        print(f"处理速率: {50 / total_time:.2f} RPS")

        assert avg_latency < 200, f"高负载下延迟 {avg_latency}ms 过高"


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
