import requests
import time
import uuid
import json

BASE_URL = "http://localhost:3000/v1"
API_KEY = "test_api_key_123"
HEADERS = {
    "Authorization": f"Bearer {API_KEY}"
}

def test_health_check():
    print("🏥 Checking API health...")
    try:
        response = requests.get("http://localhost:3000/health")
        assert response.status_code == 200
        print("✅ Health check passed")
    except Exception as e:
        print(f"❌ Health check failed: {e}")

def test_create_and_cancel_crawl():
    # 1. Create a crawl task
    payload = {
        "url": "https://example.com",
        "name": "E2E Test Crawl",
        "config": {
            "max_depth": 1,
            "strategy": "bfs",
            "extraction_rules": {
                "title": {
                    "selector": "title",
                    "is_array": False
                }
            }
        }
    }
    
    # 2. Mock Authentication (if needed) or assuming dev environment allows access
    print("🚀 Creating crawl task...")
    try:
        response = requests.post(f"{BASE_URL}/crawl", json=payload, headers=HEADERS)
        if response.status_code == 201:
            data = response.json()
            crawl_id = data["id"]
            print(f"✅ Crawl created with ID: {crawl_id}")
            
            # 3. Wait a bit
            time.sleep(1)
            
            # 4. Cancel the crawl
            print(f"🛑 Cancelling crawl {crawl_id}...")
            cancel_response = requests.delete(f"{BASE_URL}/crawl/{crawl_id}", headers=HEADERS)
            
            if cancel_response.status_code == 204:
                print("✅ Crawl cancelled successfully")
            else:
                print(f"❌ Failed to cancel crawl: {cancel_response.status_code} - {cancel_response.text}")
        else:
            print(f"❌ Failed to create crawl: {response.status_code} - {response.text}")
    except Exception as e:
        print(f"❌ Crawl test failed: {e}")

def test_search_and_crawl():
    print("🔍 Testing Search + Async Crawl...")
    
    # 1. Search request
    search_payload = {
        "query": "rust programming",
        "url": "https://www.rust-lang.org",
        "max_results": 5,
        "crawl_results": True,
        "crawl_config": {
            "max_depth": 1
        }
    }
    
    try:
        response = requests.post(f"{BASE_URL}/search", json=search_payload, headers=HEADERS)
        if response.status_code == 200:
            data = response.json()
            print(f"✅ Search successful, found {len(data['results'])} results")
            
            if "crawl_id" in data and data["crawl_id"]:
                print(f"✅ Async crawl triggered with ID: {data['crawl_id']}")
            else:
                print("❌ Async crawl ID missing in response")
        else:
             print(f"❌ Search failed: {response.status_code} - {response.text}")
    except Exception as e:
        print(f"❌ Exception during search test: {e}")


if __name__ == "__main__":
    print("Running E2E Tests...")
    test_health_check()
    test_create_and_cancel_crawl()
    test_search_and_crawl()
    test_search_and_crawl()
