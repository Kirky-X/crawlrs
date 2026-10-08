#!/usr/bin/env python3
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

import requests
import time
import concurrent.futures
import random
import os
import sys

BASE_URL = os.getenv("CRAWLRS_STRESS_TEST_URL", "http://localhost:8899/v1")
CONCURRENT_USERS = int(os.getenv("CRAWLRS_STRESS_TEST_CONCURRENT_USERS", "50"))
TOTAL_REQUESTS = int(os.getenv("CRAWLRS_STRESS_TEST_TOTAL_REQUESTS", "500"))
# 真实引擎搜索（上游秒级延迟）+ 限流排队下，固定 5s 客户端超时会把高并发
# 全部计为失败；压测长延迟场景时经环境变量放宽，默认保持 5s
REQUEST_TIMEOUT_S = int(os.getenv("CRAWLRS_STRESS_TEST_TIMEOUT_MS", "5000")) / 1000
# 业务端点要求认证：不带 key 的请求全部落在 401 拒绝路径，
# 测得的 RPS/延迟只是认证失败耗时，不构成有效吞吐结论
API_KEY = os.getenv("CRAWLRS_API_KEY", os.getenv("TEST_API_KEY", ""))
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
}

def make_request(request_id):
    start_time = time.time()
    try:
        # Simulate search request
        payload = {
            "query": f"load test {request_id}",
            "limit": 1
        }
        response = requests.post(f"{BASE_URL}/search", json=payload, headers=HEADERS, timeout=REQUEST_TIMEOUT_S)
        latency = (time.time() - start_time) * 1000
        return {
            "status": response.status_code,
            "latency": latency,
            "success": response.status_code == 200
        }
    except Exception as e:
        latency = (time.time() - start_time) * 1000
        return {
            "status": 0,
            "latency": latency,
            "success": False,
            "error": str(e)
        }

def run_stress_test():
    if not API_KEY:
        print("❌ 未设置 API key（CRAWLRS_API_KEY / TEST_API_KEY）："
              "不带认证的请求全部 401，无法对业务路径产出有效压测数据")
        sys.exit(1)

    print(f"🚀 Starting Stress Test: {TOTAL_REQUESTS} requests with {CONCURRENT_USERS} concurrent users")

    results = []
    start_time = time.time()

    with concurrent.futures.ThreadPoolExecutor(max_workers=CONCURRENT_USERS) as executor:
        futures = [executor.submit(make_request, i) for i in range(TOTAL_REQUESTS)]
        for future in concurrent.futures.as_completed(futures):
            results.append(future.result())

    total_time = time.time() - start_time

    # Analyze results
    total_requests = len(results)
    successful_requests = sum(1 for r in results if r['success'])
    failed_requests = total_requests - successful_requests
    latencies = [r['latency'] for r in results]
    avg_latency = sum(latencies) / total_requests if total_requests > 0 else 0
    max_latency = max(latencies) if latencies else 0
    p95_latency = sorted(latencies)[int(total_requests * 0.95)] if latencies else 0

    status_counts = {}
    for r in results:
        status_counts[r["status"]] = status_counts.get(r["status"], 0) + 1

    print("\n📊 Stress Test Results:")
    print(f"Total Time: {total_time:.2f}s")
    print(f"Requests per Second (RPS): {total_requests / total_time:.2f}")
    print(f"Total Requests: {total_requests}")
    print(f"Successful: {successful_requests}")
    print(f"Failed: {failed_requests}")
    print(f"Status Codes: {dict(sorted(status_counts.items()))}")
    print(f"Avg Latency: {avg_latency:.2f}ms")
    print(f"Max Latency: {max_latency:.2f}ms")
    print(f"P95 Latency: {p95_latency:.2f}ms")

if __name__ == "__main__":
    run_stress_test()
