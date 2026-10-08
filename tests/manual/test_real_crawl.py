#!/usr/bin/env python3
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

import requests
import time
import json
import os
import sys

BASE_URL = os.getenv("CRAWLRS_API_URL", "http://localhost:8899") + "/v1"
# garrison 只认证真实签发的 key（自举 key 随进程轮换），硬编码假 key 必然 401；
# 必须经 CRAWLRS_API_KEY / TEST_API_KEY 注入有效凭证，未注入则直接拒绝运行
API_KEY = os.getenv("CRAWLRS_API_KEY", os.getenv("TEST_API_KEY", ""))
HEADERS = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json"
}

URLS = [
    "https://news.sina.com.cn/c/xl/2025-12-14/doc-inhatxvm5239308.shtml",
    "https://www.chinanews.com.cn/sh/2025/12-14/10533307.shtml"
]

def run_crawl(url):
    print(f"\n🚀 Starting crawl for: {url}")
    payload = {
        "url": url,
        "name": f"Test Crawl - {url[-20:]}",
        "config": {
            "max_depth": 1, # Fetch the page and maybe one level deep, or just 0 if we want strictly one page.
                            # But wait, if max_depth is 0, process_crawl_result returns empty immediately.
                            # The initial task has depth 0.
                            # If max_depth is 1, depth 0 < 1, so it extracts links and creates depth 1 tasks.
                            # If max_depth is 0, depth 0 >= 0, so it returns empty.
                            # So max_depth 0 means "only this page".
            "strategy": "bfs",
            "headers": {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
            }
        }
    }

    try:
        # Create Crawl（响应为 {success, data:{...}} 信封，业务字段在 data 下）；
        # 202 = 异步入队 Accepted（任务 queued 由 worker 消费），语义正确
        response = requests.post(f"{BASE_URL}/crawl", json=payload, headers=HEADERS)
        if response.status_code not in [200, 201, 202]:
            print(f"❌ Failed to create crawl: {response.status_code} - {response.text}")
            return

        data = response.json().get("data") or {}
        crawl_id = data.get("id")
        if not crawl_id:
            print(f"❌ Create crawl response missing data.id: {response.text}")
            return
        print(f"✅ Crawl created with ID: {crawl_id}")

        # Poll Status
        print("⏳ Waiting for crawl to complete...")
        for i in range(30): # Wait up to 30 seconds
            status_res = requests.get(f"{BASE_URL}/crawl/{crawl_id}", headers=HEADERS)
            if status_res.status_code != 200:
                print(f"⚠️ Failed to get status: {status_res.status_code}")
                time.sleep(1)
                continue

            crawl_data = status_res.json().get("data") or {}
            status = crawl_data.get("status")
            completed = crawl_data.get("completed_tasks", 0)
            failed = crawl_data.get("failed_tasks", 0)
            total = crawl_data.get("total_tasks", 0)

            print(f"   [{i+1}s] Status: {status}, Completed: {completed}, Failed: {failed}, Total: {total}")

            if status in ["completed", "failed", "cancelled"]:
                if completed > 0:
                    print(f"✅ Crawl finished successfully! Completed tasks: {completed}")
                else:
                    print(f"❌ Crawl finished but no tasks completed. Status: {status}")
                break

            time.sleep(1)
        else:
            print("❌ Timeout waiting for crawl to complete")

    except Exception as e:
        print(f"❌ Exception: {e}")

if __name__ == "__main__":
    if not API_KEY:
        print("❌ 未设置 API key（CRAWLRS_API_KEY / TEST_API_KEY）："
              "garrison 只认证真实签发的 key，硬编码凭证必然 401")
        sys.exit(1)

    print("🌍 Starting Real-World Crawl Test")

    # Verify Health First
    try:
        h = requests.get(f"{os.getenv('CRAWLRS_API_URL', 'http://localhost:8899')}/health")
        if h.status_code != 200:
            print("❌ Health check failed. Is the server running?")
            sys.exit(1)
        print("✅ Server is healthy")
    except Exception:
        print("❌ Could not connect to server")
        sys.exit(1)

    for url in URLS:
        run_crawl(url)
        time.sleep(2) # Graceful pause between tests
