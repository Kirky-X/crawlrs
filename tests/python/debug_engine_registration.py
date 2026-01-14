#!/usr/bin/env python3
"""
调试搜索引擎注册问题
"""

import requests


def test_engine_registration():
    headers = {"Authorization": "Bearer test-api-key"}

    print("=== 搜索引擎注册测试 ===\n")

    # 测试各引擎
    test_cases = [
        ("baidu", "Baidu"),
        ("bing", "Bing"),
        ("sogou", "Sogou"),
    ]

    for request_engine, expected_engine in test_cases:
        resp = requests.post(
            "http://localhost:3000/v1/search",
            json={"query": "test", "engines": [request_engine]},
            headers=headers,
            timeout=30,
        )

        data = resp.json()
        results = data.get("results", [])

        # 统计各引擎返回结果
        engine_counts = {}
        for r in results:
            engine = r.get("engine", "Unknown")
            engine_counts[engine] = engine_counts.get(engine, 0) + 1

        status = "✅ 正确" if expected_engine in engine_counts else "❌ 错误"
        print(f"请求 engines=['{request_engine}'] → {status}")
        print(f"  期望: {expected_engine}")
        print(f"  实际: {engine_counts}")
        print()


if __name__ == "__main__":
    test_engine_registration()
