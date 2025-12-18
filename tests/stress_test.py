#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the MIT License
# See LICENSE file in the project root for full license information.

import requests
import time
import concurrent.futures
import random

BASE_URL = "http://localhost:8899/v1"
CONCURRENT_USERS = 50
TOTAL_REQUESTS = 500

def make_request(request_id):
    start_time = time.time()
    try:
        # Simulate search request
        payload = {
            "query": f"load test {request_id}",
            "limit": 1
        }
        response = requests.post(f"{BASE_URL}/search", json=payload, timeout=5)
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
    
    print("\n📊 Stress Test Results:")
    print(f"Total Time: {total_time:.2f}s")
    print(f"Requests per Second (RPS): {total_requests / total_time:.2f}")
    print(f"Total Requests: {total_requests}")
    print(f"Successful: {successful_requests}")
    print(f"Failed: {failed_requests}")
    print(f"Avg Latency: {avg_latency:.2f}ms")
    print(f"Max Latency: {max_latency:.2f}ms")
    print(f"P95 Latency: {p95_latency:.2f}ms")

if __name__ == "__main__":
    run_stress_test()
