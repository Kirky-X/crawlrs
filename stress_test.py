import requests
import json
import time
import threading
import uuid
from datetime import datetime

# Configuration
BASE_URL = "http://localhost:3000/v1"
API_KEY = "test_api_key_123"
NUM_CONCURRENT_CRAWLS = 5  # Number of concurrent crawl requests
POLL_INTERVAL = 1  # Seconds between status checks
MAX_RETRIES = 30  # Maximum number of status checks before giving up

# Headers
headers = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json"
}

# Test Data
def generate_crawl_request():
    return {
        "url": "https://news.sina.com.cn/",
        "config": {
            "max_depth": 1,  # Keep depth low for quick tests
            "strategy": "bfs",
            "include_patterns": ["news.sina.com.cn/c/"],
            "exclude_patterns": ["login", "register"]
        }
    }

def run_single_crawl(thread_id):
    print(f"[Thread-{thread_id}] Starting crawl test...")
    
    # 1. Create Crawl
    try:
        payload = generate_crawl_request()
        response = requests.post(f"{BASE_URL}/crawl", headers=headers, json=payload)
        
        if response.status_code not in [200, 201]:
            print(f"[Thread-{thread_id}] Failed to create crawl: {response.status_code} - {response.text}")
            return False
            
        crawl_data = response.json()
        crawl_id = crawl_data['id']
        print(f"[Thread-{thread_id}] Crawl created with ID: {crawl_id}")
        
    except Exception as e:
        print(f"[Thread-{thread_id}] Exception during creation: {e}")
        return False

    # 2. Poll Status
    start_time = time.time()
    for i in range(MAX_RETRIES):
        try:
            time.sleep(POLL_INTERVAL)
            status_response = requests.get(f"{BASE_URL}/crawl/{crawl_id}", headers=headers)
            
            if status_response.status_code != 200:
                print(f"[Thread-{thread_id}] Failed to get status: {status_response.status_code}")
                continue
                
            status_data = status_response.json()
            status = status_data.get('status')
            completed = status_data.get('completed_tasks', 0)
            total = status_data.get('total_tasks', 0)
            
            print(f"[Thread-{thread_id}] Status: {status} ({completed}/{total})")
            
            if status == 'completed':
                duration = time.time() - start_time
                print(f"[Thread-{thread_id}] SUCCESS: Crawl completed in {duration:.2f}s")
                return True
            
            if status == 'failed':
                print(f"[Thread-{thread_id}] FAILURE: Crawl failed")
                return False
                
        except Exception as e:
            print(f"[Thread-{thread_id}] Exception during polling: {e}")
            
    print(f"[Thread-{thread_id}] TIMEOUT: Crawl did not complete within {MAX_RETRIES * POLL_INTERVAL}s")
    return False

def main():
    print(f"Starting Stress Test with {NUM_CONCURRENT_CRAWLS} threads...")
    threads = []
    results = []
    
    # Wrapper to collect results
    def thread_wrapper(tid, result_list):
        success = run_single_crawl(tid)
        result_list.append(success)

    for i in range(NUM_CONCURRENT_CRAWLS):
        t = threading.Thread(target=thread_wrapper, args=(i, results))
        threads.append(t)
        t.start()
        
    for t in threads:
        t.join()
        
    success_count = results.count(True)
    print("\n=== Test Summary ===")
    print(f"Total Requests: {NUM_CONCURRENT_CRAWLS}")
    print(f"Successful: {success_count}")
    print(f"Failed: {NUM_CONCURRENT_CRAWLS - success_count}")
    
    if success_count == NUM_CONCURRENT_CRAWLS:
        print("RESULT: PASS")
    else:
        print("RESULT: FAIL")

if __name__ == "__main__":
    main()
