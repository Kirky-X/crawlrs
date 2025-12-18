#!/usr/bin/env python3
# Copyright (c) 2025 Kirky.X
#
# Licensed under the MIT License
# See LICENSE file in the project root for full license information.

"""
端到端测试套件 - 完整业务流程验证

该模块包含对 crawlrs 系统核心功能的端到端测试，验证从任务创建到结果获取的完整业务流程。
"""

import requests
import time
import json
import uuid
from typing import Dict, Any, Optional, List
from concurrent.futures import ThreadPoolExecutor, as_completed

# 基础配置
BASE_URL = "http://localhost:3000"
HEADERS = {
    "Authorization": "Bearer test-api-key",
    "Content-Type": "application/json"
}

# 测试数据
TEST_URLS = {
    "simple": "https://httpbin.org/html",
    "complex": "https://httpbin.org/json",
    "javascript": "https://httpbin.org/html",  # 模拟需要JS渲染的页面
    "error": "https://httpbin.org/status/500"
}

def create_task(endpoint: str, payload: Dict[str, Any]) -> Optional[str]:
    """创建任务并返回任务ID"""
    try:
        response = requests.post(f"{BASE_URL}{endpoint}", json=payload, headers=HEADERS)
        if response.status_code in [201, 202]:
            result = response.json()
            return result.get("id")
        else:
            print(f"创建任务失败: {response.status_code} - {response.text}")
            return None
    except Exception as e:
        print(f"创建任务异常: {e}")
        return None

def get_task_status(task_id: str, endpoint: str = "/v1/scrape") -> Optional[Dict[str, Any]]:
    """获取任务状态"""
    try:
        response = requests.get(f"{BASE_URL}{endpoint}/{task_id}", headers=HEADERS)
        if response.status_code == 200:
            return response.json()
        else:
            print(f"获取任务状态失败: {response.status_code}")
            return None
    except Exception as e:
        print(f"获取任务状态异常: {e}")
        return None

def wait_for_task_completion(task_id: str, endpoint: str = "/v1/scrape", 
                           timeout: int = 60, poll_interval: int = 2) -> Optional[Dict[str, Any]]:
    """等待任务完成并返回最终结果"""
    start_time = time.time()
    
    while time.time() - start_time < timeout:
        status = get_task_status(task_id, endpoint)
        if not status:
            return None
            
        task_status = status.get("status", "").lower()
        
        if task_status == "completed":
            return status
        elif task_status == "failed":
            return status
        elif task_status in ["cancelled", "timeout"]:
            return status
        
        time.sleep(poll_interval)
    
    print(f"任务 {task_id} 超时未完成")
    return None

def cancel_task(task_id: str, endpoint: str = "/v1/scrape") -> bool:
    """取消任务"""
    try:
        response = requests.delete(f"{BASE_URL}{endpoint}/{task_id}", headers=HEADERS)
        return response.status_code == 204
    except Exception as e:
        print(f"取消任务异常: {e}")
        return False

def test_scrape_basic():
    """测试基础抓取功能"""
    print("🧪 测试基础抓取功能...")
    
    payload = {
        "url": TEST_URLS["simple"],
        "task_type": "scrape",
        "payload": {
            "extract_rules": {
                "title": {
                    "selector": "title",
                    "is_array": False
                }
            }
        }
    }
    
    task_id = create_task("/v1/scrape", payload)
    if not task_id:
        print("❌ 创建抓取任务失败")
        return False
    
    print(f"✅ 创建任务成功: {task_id}")
    
    # 等待任务完成
    result = wait_for_task_completion(task_id)
    if not result:
        print("❌ 任务未完成或失败")
        return False
    
    # 验证结果
    if result.get("status") == "completed":
        print("✅ 任务成功完成")
        
        # 验证提取的内容
        content = result.get("content", {})
        if content.get("title"):
            print(f"✅ 成功提取标题: {content['title']}")
            return True
        else:
            print("❌ 未提取到标题内容")
            return False
    else:
        print(f"❌ 任务状态: {result.get('status')}")
        print(f"错误信息: {result.get('error', '未知错误')}")
        return False

def test_crawl_basic():
    """测试基础爬取功能"""
    print("🧪 测试基础爬取功能...")
    
    payload = {
        "url": TEST_URLS["simple"],
        "crawler_options": {
            "max_depth": 1,
            "limit": 10,
            "strategy": "bfs"
        }
    }
    
    task_id = create_task("/v1/crawl", payload)
    if not task_id:
        print("❌ 创建爬取任务失败")
        return False
    
    print(f"✅ 创建爬取任务成功: {task_id}")
    
    # 等待任务完成
    result = wait_for_task_completion(task_id, "/v1/crawl")
    if not result:
        print("❌ 爬取任务未完成或失败")
        return False
    
    # 验证结果
    if result.get("status") == "completed":
        print("✅ 爬取任务成功完成")
        
        # 验证爬取结果
        urls_crawled = result.get("urls_crawled", 0)
        print(f"✅ 爬取URL数量: {urls_crawled}")
        
        if urls_crawled > 0:
            return True
        else:
            print("❌ 未爬取到任何URL")
            return False
    else:
        print(f"❌ 爬取任务状态: {result.get('status')}")
        print(f"错误信息: {result.get('error', '未知错误')}")
        return False

def test_search_basic():
    """测试基础搜索功能"""
    print("🧪 测试基础搜索功能...")
    
    payload = {
        "query": "rust programming language",
        "sources": ["web"],
        "limit": 5
    }
    
    task_id = create_task("/v1/search", payload)
    if not task_id:
        print("❌ 创建搜索任务失败")
        return False
    
    print(f"✅ 创建搜索任务成功: {task_id}")
    
    # 等待任务完成
    result = wait_for_task_completion(task_id, "/v1/search")
    if not result:
        print("❌ 搜索任务未完成或失败")
        return False
    
    # 验证结果
    if result.get("status") == "completed":
        print("✅ 搜索任务成功完成")
        
        # 验证搜索结果
        data = result.get("data", {})
        web_results = data.get("web", [])
        
        if web_results and len(web_results) > 0:
            print(f"✅ 搜索到 {len(web_results)} 个结果")
            # 验证第一个结果的结构
            first_result = web_results[0]
            if all(key in first_result for key in ["title", "url", "snippet"]):
                print("✅ 搜索结果格式正确")
                return True
            else:
                print("❌ 搜索结果格式不正确")
                return False
        else:
            print("❌ 未搜索到任何结果")
            return False
    else:
        print(f"❌ 搜索任务状态: {result.get('status')}")
        print(f"错误信息: {result.get('error', '未知错误')}")
        return False

def test_extract_basic():
    """测试基础提取功能"""
    print("🧪 测试基础提取功能...")
    
    payload = {
        "urls": [TEST_URLS["simple"]],
        "prompt": "Extract the page title and any headings (h1, h2, h3)"
    }
    
    task_id = create_task("/v1/extract", payload)
    if not task_id:
        print("❌ 创建提取任务失败")
        return False
    
    print(f"✅ 创建提取任务成功: {task_id}")
    
    # 等待任务完成
    result = wait_for_task_completion(task_id, "/v1/extract")
    if not result:
        print("❌ 提取任务未完成或失败")
        return False
    
    # 验证结果
    if result.get("status") == "completed":
        print("✅ 提取任务成功完成")
        
        # 验证提取结果
        data = result.get("data", {})
        if data:
            print(f"✅ 成功提取数据: {json.dumps(data, indent=2, ensure_ascii=False)}")
            return True
        else:
            print("❌ 未提取到任何数据")
            return False
    else:
        print(f"❌ 提取任务状态: {result.get('status')}")
        print(f"错误信息: {result.get('error', '未知错误')}")
        return False

def test_task_cancellation():
    """测试任务取消功能"""
    print("🧪 测试任务取消功能...")
    
    # 创建一个长时间运行的爬取任务
    payload = {
        "url": "https://httpbin.org/html",
        "crawler_options": {
            "max_depth": 3,
            "limit": 100,
            "strategy": "bfs"
        }
    }
    
    task_id = create_task("/v1/crawl", payload)
    if not task_id:
        print("❌ 创建任务失败")
        return False
    
    print(f"✅ 创建任务成功: {task_id}")
    
    # 等待一小段时间让任务开始
    time.sleep(3)
    
    # 尝试取消任务
    if cancel_task(task_id, "/v1/crawl"):
        print("✅ 任务取消请求成功")
        
        # 等待并验证任务状态
        time.sleep(2)
        final_status = get_task_status(task_id, "/v1/crawl")
        
        if final_status and final_status.get("status") in ["cancelled", "cancelling"]:
            print("✅ 任务已成功取消")
            return True
        else:
            print(f"❌ 任务状态未正确更新: {final_status.get('status') if final_status else '未知'}")
            return False
    else:
        print("❌ 任务取消失败")
        return False

def test_concurrent_tasks():
    """测试并发任务处理"""
    print("🧪 测试并发任务处理...")
    
    def create_and_wait_task(task_num: int) -> Dict[str, Any]:
        """创建并等待单个任务完成"""
        payload = {
            "url": TEST_URLS["simple"],
            "task_type": "scrape",
            "payload": {
                "extract_rules": {
                    "title": {
                        "selector": "title",
                        "is_array": False
                    }
                }
            }
        }
        
        task_id = create_task("/v1/scrape", payload)
        if not task_id:
            return {"success": False, "error": "创建任务失败"}
        
        result = wait_for_task_completion(task_id)
        if not result:
            return {"success": False, "error": "任务未完成"}
        
        return {
            "success": result.get("status") == "completed",
            "task_id": task_id,
            "task_num": task_num
        }
    
    # 并发创建5个任务
    num_tasks = 5
    results = []
    
    with ThreadPoolExecutor(max_workers=num_tasks) as executor:
        futures = [executor.submit(create_and_wait_task, i) for i in range(num_tasks)]
        
        for future in as_completed(futures):
            results.append(future.result())
    
    # 验证结果
    successful_tasks = sum(1 for r in results if r.get("success", False))
    
    print(f"✅ 并发任务测试结果: {successful_tasks}/{num_tasks} 成功")
    
    if successful_tasks == num_tasks:
        print("✅ 所有并发任务都成功完成")
        return True
    else:
        print("❌ 部分并发任务失败")
        for result in results:
            if not result.get("success", False):
                print(f"  任务 {result.get('task_num')}: {result.get('error', '未知错误')}")
        return False

def test_error_handling():
    """测试错误处理机制"""
    print("🧪 测试错误处理机制...")
    
    test_cases = [
        {
            "name": "无效URL格式",
            "payload": {
                "url": "not-a-valid-url",
                "task_type": "scrape",
                "payload": {}
            },
            "expected_status": 422
        },
        {
            "name": "缺少必需参数",
            "payload": {
                "task_type": "scrape",
                "payload": {}
            },
            "expected_status": 422
        },
        {
            "name": "无效认证",
            "payload": {
                "url": "https://example.com",
                "task_type": "scrape",
                "payload": {}
            },
            "headers": {"Authorization": "Bearer invalid-key"},
            "expected_status": 401
        }
    ]
    
    all_passed = True
    
    for test_case in test_cases:
        print(f"  测试: {test_case['name']}")
        
        headers = test_case.get("headers", HEADERS)
        response = requests.post(f"{BASE_URL}/v1/scrape", 
                               json=test_case["payload"], 
                               headers=headers)
        
        if response.status_code == test_case["expected_status"]:
            print(f"    ✅ 返回正确的状态码: {response.status_code}")
        else:
            print(f"    ❌ 期望状态码 {test_case['expected_status']}, 实际: {response.status_code}")
            all_passed = False
    
    return all_passed

def test_rate_limiting():
    """测试速率限制机制"""
    print("🧪 测试速率限制机制...")
    
    # 快速发送多个请求以触发速率限制
    payload = {
        "url": "https://example.com",
        "task_type": "scrape",
        "payload": {}
    }
    
    # 发送超过速率限制的请求（假设限制为100 RPM）
    rate_limited = False
    
    for i in range(105):
        response = requests.post(f"{BASE_URL}/v1/scrape", json=payload, headers=HEADERS)
        
        if response.status_code == 429:  # Too Many Requests
            rate_limited = True
            print(f"✅ 在第 {i+1} 个请求时触发速率限制")
            break
    
    if rate_limited:
        # 验证错误响应格式
        error_data = response.json()
        if "error" in error_data and "rate limit" in error_data["error"].lower():
            print("✅ 速率限制错误信息正确")
            return True
        else:
            print("❌ 速率限制错误信息格式不正确")
            return False
    else:
        print("❌ 未触发速率限制")
        return False

def test_scrape_screenshot():
    """测试页面截图功能 (UAT-005)"""
    print("🧪 测试页面截图功能...")
    payload = {
        "url": TEST_URLS["simple"],
        "formats": ["screenshot"]
    }
    
    task_id = create_task("/v1/scrape", payload)
    if not task_id:
        print("❌ 创建截图任务失败")
        return False
        
    result = wait_for_task_completion(task_id)
    if result and result.get("status") == "completed":
        # Check if screenshot data exists
        data = result.get("data", {})
        if "screenshot" in data and data["screenshot"]:
            print(f"✅ 截图任务完成且包含截图数据")
            return True
        else:
             print(f"❌ 截图任务完成但缺少截图数据: {data.keys()}")
             return False
    else:
        print(f"❌ 截图任务失败: {result.get('status') if result else 'None'}")
        return False

def test_crawl_full():
    """测试全站爬取功能 (UAT-006)"""
    print("🧪 测试全站爬取功能...")
    # Use a small real site for full crawl test
    payload = {
        "url": "https://httpbin.org/links/5/0", # Returns a page with 5 links
        "crawler_options": {
            "max_depth": 1,
            "limit": 10
        }
    }
    crawl_id = create_task("/v1/crawl", payload)
    if not crawl_id:
        print("❌ 创建全站爬取任务失败")
        return False
    
    # Wait longer for crawl
    result = wait_for_task_completion(crawl_id, endpoint="/v1/crawl", timeout=120)
    if result and result.get("status") == "completed":
        print(f"✅ 全站爬取任务完成: {crawl_id}")
        # Verify stats if available
        stats = result.get("stats", {})
        print(f"   爬取统计: {stats}")
        return True
    else:
        print(f"❌ 全站爬取任务失败: {result.get('status') if result else 'None'}")
        return False

def run_all_tests():
    """运行所有端到端测试"""
    print("🚀 开始端到端测试套件\n")
    
    tests = [
        ("基础抓取功能", test_scrape_basic),
        ("基础爬取功能", test_crawl_basic),
        ("基础搜索功能", test_search_basic),
        ("基础提取功能", test_extract_basic),
        ("页面截图功能", test_scrape_screenshot),
        ("全站爬取功能", test_crawl_full),
        ("任务取消功能", test_task_cancellation),
        ("并发任务处理", test_concurrent_tasks),
        ("错误处理机制", test_error_handling),
        ("速率限制机制", test_rate_limiting),
    ]
    
    results = []
    
    for test_name, test_func in tests:
        try:
            print(f"\n{'='*50}")
            result = test_func()
            results.append((test_name, result))
            
            if result:
                print(f"\n✅ {test_name} - 通过")
            else:
                print(f"\n❌ {test_name} - 失败")
                
        except Exception as e:
            print(f"\n❌ {test_name} - 异常: {e}")
            results.append((test_name, False))
    
    # 总结报告
    print(f"\n{'='*60}")
    print("📊 端到端测试总结报告")
    print(f"{'='*60}")
    
    passed = sum(1 for _, result in results if result)
    total = len(results)
    
    print(f"总测试数: {total}")
    print(f"通过数: {passed}")
    print(f"失败数: {total - passed}")
    print(f"通过率: {(passed/total)*100:.1f}%")
    
    print("\n详细结果:")
    for test_name, result in results:
        status = "✅ 通过" if result else "❌ 失败"
        print(f"  {status} {test_name}")
    
    return passed == total

if __name__ == "__main__":
    success = run_all_tests()
    exit(0 if success else 1)