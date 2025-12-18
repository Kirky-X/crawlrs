#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the MIT License
# See LICENSE file in the project root for full license information.

import requests
import time
import json
import sys

BASE_URL = "http://localhost:8899/v1"
API_KEY = "test_api_key_123"
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
        # Create Crawl
        response = requests.post(f"{BASE_URL}/crawl", json=payload, headers=HEADERS)
        if response.status_code != 201:
            print(f"❌ Failed to create crawl: {response.status_code} - {response.text}")
            return

        data = response.json()
        crawl_id = data["id"]
        print(f"✅ Crawl created with ID: {crawl_id}")

        # Poll Status
        print("⏳ Waiting for crawl to complete...")
        for i in range(30): # Wait up to 30 seconds
            status_res = requests.get(f"{BASE_URL}/crawl/{crawl_id}", headers=HEADERS)
            if status_res.status_code != 200:
                print(f"⚠️ Failed to get status: {status_res.status_code}")
                time.sleep(1)
                continue
            
            crawl_data = status_res.json()
            status = crawl_data["status"]
            completed = crawl_data["completed_tasks"]
            failed = crawl_data["failed_tasks"]
            total = crawl_data["total_tasks"]

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
    print("🌍 Starting Real-World Crawl Test")
    
    # Verify Health First
    try:
        h = requests.get("http://localhost:8899/health")
        if h.status_code != 200:
            print("❌ Health check failed. Is the server running?")
            sys.exit(1)
        print("✅ Server is healthy")
    except:
        print("❌ Could not connect to server")
        sys.exit(1)

    for url in URLS:
        run_crawl(url)
        time.sleep(2) # Graceful pause between tests
