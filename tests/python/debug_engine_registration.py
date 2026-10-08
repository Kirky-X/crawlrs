#!/usr/bin/env python3
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

"""
调试搜索引擎注册问题
"""

import os

import requests


def test_engine_registration():
    base_url = os.getenv("CRAWLRS_API_URL", "http://localhost:8899")
    api_key = os.getenv("CRAWLRS_API_KEY", os.getenv("TEST_API_KEY", ""))
    if not api_key:
        raise SystemExit("未设置 API key（CRAWLRS_API_KEY / TEST_API_KEY）")
    headers = {"Authorization": f"Bearer {api_key}"}

    print("=== 搜索引擎注册测试 ===\n")

    # 测试各引擎
    test_cases = [
        ("baidu", "Baidu"),
        ("bing", "Bing"),
        ("sogou", "Sogou"),
    ]

    for request_engine, expected_engine in test_cases:
        resp = requests.post(
            f"{base_url}/v1/search",
            json={"query": "test", "engines": [request_engine]},
            headers=headers,
            timeout=30,
        )

        data = resp.json()
        results = (data.get("data") or {}).get("results", [])

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
