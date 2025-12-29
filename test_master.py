#!/usr/bin/env python3
"""
统一测试脚本 - 合并所有API测试功能
支持：完整API测试、快速测试、搜索引擎测试（Sogou/Google/Baidu等）
"""

import requests
import json
import time
import uuid
from datetime import datetime
from typing import Dict, List, Tuple, Any, Optional
from urllib.parse import quote, unquote
from bs4 import BeautifulSoup
import re
import sys

BASE_URL = "http://localhost:3000"
SEARCH_API_DELAY = 3
REQUEST_TIMEOUT = 30
API_KEY = "test_api_key_fixed"
TEAM_ID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"


class TestResults:
    def __init__(self):
        self.total = 0
        self.passed = 0
        self.failed = 0
        self.results = []
        self.start_time = datetime.now()

    def add_result(self, name: str, method: str, path: str, status: bool,
                   response_code: int = None, error: str = None, details: str = None):
        self.total += 1
        if status:
            self.passed += 1
        else:
            self.failed += 1

        result = {
            "name": name,
            "method": method,
            "path": path,
            "status": "✓ PASS" if status else "✗ FAIL",
            "response_code": response_code,
            "error": error,
            "details": details,
            "timestamp": datetime.now().isoformat()
        }
        self.results.append(result)

        status_icon = "✓" if status else "✗"
        status_text = "PASS" if status else "FAIL"
        code_info = f" [{response_code}]" if response_code else ""
        error_info = f" - {error}" if error else ""
        print(f"{status_icon} {status_text}{code_info}: {method:6} {path}{error_info}")

    def print_summary(self):
        elapsed_time = (datetime.now() - self.start_time).total_seconds()
        print("\n" + "=" * 80)
        print(f"测试总结 (耗时: {elapsed_time:.2f}秒)")
        print("=" * 80)
        print(f"总计: {self.total} | 成功: {self.passed} | 失败: {self.failed}")
        print(f"成功率: {(self.passed / self.total * 100):.1f}%" if self.total > 0 else "成功率: 0%")
        print("=" * 80)

    def get_passed_interfaces(self) -> List[Dict]:
        return [r for r in self.results if r["status"] == "✓ PASS"]

    def get_failed_interfaces(self) -> List[Dict]:
        return [r for r in self.results if r["status"] == "✗ FAIL"]


class APITester:
    def __init__(self):
        self.session = requests.Session()
        self.results = TestResults()
        self.session.headers.update({
            "Content-Type": "application/json",
            "Authorization": f"Bearer {API_KEY}",
            "X-Team-ID": TEAM_ID,
            "User-Agent": "Crawlrs-Master-Tester/1.0"
        })

    def _make_request(self, method: str, endpoint: str, data: Dict = None,
                      params: Dict = None, expect_codes: List[int] = None) -> Tuple[bool, int, str, Any]:
        try:
            url = f"{BASE_URL}{endpoint}"

            if method == "GET":
                response = self.session.get(url, params=params, timeout=REQUEST_TIMEOUT)
            elif method == "POST":
                response = self.session.post(url, json=data, params=params, timeout=REQUEST_TIMEOUT)
            elif method == "PUT":
                response = self.session.put(url, json=data, params=params, timeout=REQUEST_TIMEOUT)
            elif method == "DELETE":
                response = self.session.delete(url, params=params, timeout=REQUEST_TIMEOUT)
            else:
                return False, None, f"Unsupported method: {method}", None

            try:
                response_data = response.json()
            except:
                response_data = response.text

            if expect_codes is None:
                is_success = response.status_code < 300
            else:
                is_success = response.status_code in expect_codes

            error_msg = None if is_success else f"HTTP {response.status_code}"
            return is_success, response.status_code, error_msg, response_data

        except requests.exceptions.Timeout:
            return False, None, "Request timeout", None
        except requests.exceptions.ConnectionError as e:
            return False, None, f"Connection error: {str(e)}", None
        except Exception as e:
            return False, None, f"Exception: {str(e)}", None

    def test_health_check(self):
        print("\n[1] 公开接口测试 - 健康检查")
        print("-" * 80)
        success, code, error, data = self._make_request("GET", "/health")
        self.results.add_result("Health Check", "GET", "/health", success, code, error,
                                json.dumps(data) if data else None)

    def test_version(self):
        print("\n[2] 公开接口测试 - 版本信息")
        print("-" * 80)
        success, code, error, data = self._make_request("GET", "/v1/version")
        self.results.add_result("Version Info", "GET", "/v1/version", success, code, error,
                                data if data else None)

    def test_metrics(self):
        print("\n[3] 公开接口测试 - 监控指标")
        print("-" * 80)
        success, code, error, data = self._make_request("GET", "/metrics")
        self.results.add_result("Metrics", "GET", "/metrics", success, code, error,
                                "Prometheus metrics data" if success else None)

    def test_scrape_endpoints(self):
        print("\n[4] Scrape接口测试")
        print("-" * 80)

        scrape_payload = {"url": "https://example.com", "wait_for_selector": None}
        success, code, error, data = self._make_request("POST", "/v1/scrape", scrape_payload)
        self.results.add_result("Create Scrape", "POST", "/v1/scrape", success, code, error,
                                json.dumps(data)[:200] if data else None)

        if success and isinstance(data, dict) and 'id' in data:
            scrape_id = data['id']
            success2, code2, error2, data2 = self._make_request("GET", f"/v1/scrape/{scrape_id}")
            self.results.add_result("Get Scrape Status", "GET", f"/v1/scrape/{{id}}", success2, code2, error2,
                                    json.dumps(data2)[:200] if data2 else None)
        else:
            test_id = str(uuid.uuid4())
            success2, code2, error2, data2 = self._make_request("GET", f"/v1/scrape/{test_id}")
            self.results.add_result("Get Scrape Status", "GET", f"/v1/scrape/{{id}}", code2 == 404, code2, error2,
                                    None)

        test_id2 = str(uuid.uuid4())
        success3, code3, error3, data3 = self._make_request("DELETE", f"/v1/scrape/{test_id2}")
        self.results.add_result("Cancel Scrape", "DELETE", f"/v1/scrape/{{id}}", code3 in [404, 204], code3, error3,
                                None)

    def test_extract_endpoints(self):
        print("\n[5] Extract接口测试")
        print("-" * 80)
        extract_payload = {"urls": ["https://example.com"], "schema": {"type": "object", "properties": {"title": {"type": "string"}}}}
        success, code, error, data = self._make_request("POST", "/v1/extract", extract_payload)
        self.results.add_result("Create Extract", "POST", "/v1/extract", success, code, error,
                                json.dumps(data)[:200] if data else None)

    def test_crawl_endpoints(self):
        print("\n[6] Crawl接口测试")
        print("-" * 80)
        crawl_payload = {"url": "https://example.com", "config": {"max_depth": 2, "strategy": "breadth_first"}}
        success, code, error, data = self._make_request("POST", "/v1/crawl", crawl_payload)
        self.results.add_result("Create Crawl", "POST", "/v1/crawl", success, code, error,
                                json.dumps(data)[:200] if data else None)

        test_id = str(uuid.uuid4())
        success, code, error, data = self._make_request("GET", f"/v1/crawl/{test_id}")
        self.results.add_result("Get Crawl", "GET", f"/v1/crawl/{{id}}", code == 404, code, error, None)

        success, code, error, data = self._make_request("GET", f"/v1/crawl/{test_id}/results")
        self.results.add_result("Get Crawl Results", "GET", f"/v1/crawl/{{id}}/results", code == 404, code, error,
                                None)

        success, code, error, data = self._make_request("DELETE", f"/v1/crawl/{test_id}")
        self.results.add_result("Cancel Crawl", "DELETE", f"/v1/crawl/{{id}}", code == 404, code, error, None)

    def test_search_endpoints(self):
        print("\n[7] Search接口测试 (串行模式 - 避免IP封禁)")
        print("-" * 80)
        search_engines = ["baidu", "bing", "sogou"]

        for engine in search_engines:
            if engine == "sogou":
                query = "新闻"
            elif engine == "baidu":
                query = "新闻"
            else:
                query = "news"

            search_payload = {"query": query, "engine": engine, "limit": 5, "sync_wait_ms": 0}
            success, code, error, data = self._make_request("POST", "/v1/search", search_payload)
            self.results.add_result(f"Search ({engine.upper()})", "POST", f"/v1/search", success, code, error,
                                    json.dumps(data)[:100] if data else None)

            if engine != search_engines[-1]:
                print(f"   ⏳ 等待 {SEARCH_API_DELAY}秒 再进行下一个搜索引擎请求...")
                time.sleep(SEARCH_API_DELAY)

    def test_webhook_endpoints(self):
        print("\n[8] Webhook接口测试")
        print("-" * 80)
        webhook_payload = {"url": "https://example.com/webhook", "events": ["scrape_completed", "crawl_completed"]}
        success, code, error, data = self._make_request("POST", "/v1/webhooks", webhook_payload)
        self.results.add_result("Create Webhook", "POST", "/v1/webhooks", success, code, error,
                                json.dumps(data)[:100] if data else None)

    def test_team_endpoints(self):
        print("\n[9] Team接口测试")
        print("-" * 80)
        success, code, error, data = self._make_request("GET", "/v1/teams/geo-restrictions")
        is_success = success or code in [401, 404]
        self.results.add_result("Get Geo Restrictions", "GET", "/v1/teams/geo-restrictions", is_success, code, error,
                                json.dumps(data) if data else None)

        geo_payload = {"allowed_countries": ["US", "GB", "CN"], "blocked_countries": []}
        success, code, error, data = self._make_request("PUT", "/v1/teams/geo-restrictions", geo_payload)
        is_success = success or code in [401, 404]
        self.results.add_result("Update Geo Restrictions", "PUT", "/v1/teams/geo-restrictions", is_success, code, error,
                                None)

    def test_task_v2_endpoints(self):
        print("\n[10] Task V2接口测试")
        print("-" * 80)
        query_payload = {"team_id": TEAM_ID, "status": None, "limit": 10, "offset": 0}
        success, code, error, data = self._make_request("POST", "/v2/tasks/query", query_payload)
        self.results.add_result("Query Tasks (V2)", "POST", "/v2/tasks/query", success or (code == 401), code, error,
                                json.dumps(data)[:100] if data else None)

        cancel_payload = {"team_id": TEAM_ID, "task_ids": [str(uuid.uuid4())], "force": False}
        success, code, error, data = self._make_request("DELETE", "/v2/tasks/cancel", cancel_payload)
        is_success = success or code in [401, 404, 400, 422]
        self.results.add_result("Cancel Tasks (V2)", "DELETE", "/v2/tasks/cancel", is_success, code, error, None)

    def run_all_tests(self):
        print("\n" + "=" * 80)
        print("开始 Crawlrs API 全面测试")
        print("=" * 80)
        print(f"基础URL: {BASE_URL}")
        print(f"测试时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
        print(f"团队ID: {TEAM_ID}")
        print("=" * 80)

        try:
            self.test_health_check()
            self.test_version()
            self.test_metrics()
            self.test_scrape_endpoints()
            self.test_extract_endpoints()
            self.test_crawl_endpoints()
            self.test_search_endpoints()
            self.test_webhook_endpoints()
            self.test_team_endpoints()
            self.test_task_v2_endpoints()
        except KeyboardInterrupt:
            print("\n\n⚠️  测试被用户中断")
        except Exception as e:
            print(f"\n\n❌ 测试执行出错: {e}")
            import traceback
            traceback.print_exc()

        self.results.print_summary()
        return self.results


class SogouTester:
    def __init__(self):
        self.session = requests.Session()
        self.session.headers.update({
            "Authorization": f"Bearer {API_KEY}",
            "X-Team-ID": TEAM_ID,
            "Content-Type": "application/json"
        })

    def test_sogou_search(self) -> Dict:
        print("\n[搜索引擎] 测试Sogou搜索...")
        search_payload = {"query": "人工智能", "engine": "sogou", "limit": 10}
        print(f"   发送请求: {search_payload}")

        try:
            response = self.session.post(f"{BASE_URL}/v1/search", json=search_payload, timeout=30)
            print(f"   响应状态码: {response.status_code}")

            if response.status_code == 200:
                result = response.json()
                results_count = len(result.get('results', []))
                print(f"   ✅ 找到 {results_count} 个搜索结果")

                for i, item in enumerate(result.get('results', [])[:3]):
                    print(f"   [{i + 1}] {item.get('title', 'N/A')[:50]}")

                return {"success": True, "count": results_count, "results": result.get('results', [])}
            else:
                print(f"   ❌ 搜索失败: {response.text[:200]}")
                return {"success": False, "error": response.text}
        except Exception as e:
            print(f"   ❌ 异常: {str(e)}")
            return {"success": False, "error": str(e)}

    def test_sogou_url_accessibility(self):
        print("\n[搜索引擎] 验证Sogou搜索结果URL可访问性...")

        search_result = self.test_sogou_search()
        if not search_result.get("success") or not search_result.get("results"):
            print("   ⚠️  无搜索结果可验证")
            return

        for i, item in enumerate(search_result["results"][:5]):
            url = item.get('url', '')
            title = item.get('title', '无标题')

            print(f"   [{i + 1}] {title[:30]}...")
            print(f"       原始URL: {url[:80]}...")

            try:
                url_response = requests.head(url, timeout=10, allow_redirects=True)
                print(f"       访问状态: {url_response.status_code}")

                if url_response.status_code >= 400:
                    print(f"       HEAD失败，尝试GET...")
                    url_response = requests.get(url, timeout=10, allow_redirects=True)
                    print(f"       GET访问状态: {url_response.status_code}")
            except Exception as e:
                print(f"       访问失败: {str(e)[:50]}")

            time.sleep(1)

    def test_sogou_parsing(self):
        print("\n[搜索引擎] 测试Sogou HTML解析...")
        query = "人工智能"
        encoded_query = quote(query, safe='')
        sogou_url = f"https://www.sogou.com/web?query={encoded_query}&ie=utf8"

        browser_headers = {
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
            'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8',
            'Accept-Language': 'zh-CN,zh;q=0.9,en;q=0.8',
        }

        try:
            response = requests.get(sogou_url, headers=browser_headers, timeout=10)
            print(f"   请求状态码: {response.status_code}")
            print(f"   HTML内容长度: {len(response.text)} 字符")

            if response.status_code == 200:
                soup = BeautifulSoup(response.text, 'html.parser')
                vrwrap_elements = soup.find_all(class_='vrwrap')
                print(f"   找到 {len(vrwrap_elements)} 个 .vrwrap 元素")

                for i, element in enumerate(vrwrap_elements[:2]):
                    h3_tags = element.find_all('h3')
                    for h3 in h3_tags:
                        title = h3.get_text(strip=True)
                        if title:
                            print(f"   解析到标题: {title[:40]}...")
                            break
        except Exception as e:
            print(f"   解析异常: {str(e)}")

    def test_all_search_engines(self):
        print("\n[搜索引擎] 测试所有搜索引擎...")
        engines = ["baidu", "bing", "sogou"]

        results = {}
        for engine in engines:
            query = "新闻" if engine in ["sogou", "baidu"] else "news"
            payload = {"query": query, "engine": engine, "limit": 5}

            try:
                response = self.session.post(f"{BASE_URL}/v1/search", json=payload, timeout=30)
                if response.status_code == 200:
                    result = response.json()
                    count = len(result.get('results', []))
                    print(f"   {engine.upper()}: {count} 个结果")
                    results[engine] = {"success": True, "count": count}
                else:
                    print(f"   {engine.upper()}: 失败 ({response.status_code})")
                    results[engine] = {"success": False, "code": response.status_code}
            except Exception as e:
                print(f"   {engine.upper()}: 异常 ({str(e)[:30]})")
                results[engine] = {"success": False, "error": str(e)}

            if engine != engines[-1]:
                time.sleep(SEARCH_API_DELAY)

        return results


class SogouDebugTester:
    def __init__(self):
        self.session = requests.Session()
        self.session.headers.update({
            "Authorization": f"Bearer {API_KEY}",
            "X-Team-ID": TEAM_ID,
            "Content-Type": "application/json"
        })

    def debug_sogou_raw_response(self):
        print("\n[调试] 测试Sogou原始API响应...")
        search_url = f"{BASE_URL}/v1/search"
        payload = {"query": "test", "engine": "sogou", "limit": 3}

        try:
            response = self.session.post(search_url, json=payload, timeout=30)
            print(f"   状态码: {response.status_code}")

            if response.status_code == 200:
                data = response.json()
                print(f"   返回结果数量: {len(data.get('results', []))}")
                print(f"   完整响应: {json.dumps(data, indent=2, ensure_ascii=False)[:500]}...")

                if data.get('results'):
                    print("\n   前3个结果:")
                    for i, result in enumerate(data['results'][:3]):
                        print(f"   {i + 1}. 标题: {result.get('title', 'N/A')}")
                        print(f"      URL: {result.get('url', 'N/A')}")
                        print(f"      描述: {result.get('description', 'N/A')}")
                else:
                    print("   ⚠️  没有返回搜索结果")
            else:
                print(f"   错误响应: {response.text[:200]}")

            return response.status_code, response.json() if response.status_code == 200 else None
        except Exception as e:
            print(f"   请求异常: {e}")
            return None, None

    def debug_manual_sogou_html(self):
        print("\n[调试] 手动请求Sogou网站查看原始HTML...")
        headers = {
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
            'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8',
            'Accept-Language': 'zh-CN,zh;q=0.9,en;q=0.8',
        }

        query = "test"
        encoded_query = quote(query, safe='')
        sogou_url = f"https://www.sogou.com/web?query={encoded_query}&ie=utf8"

        try:
            response = requests.get(sogou_url, headers=headers, timeout=30)
            print(f"   状态码: {response.status_code}")
            print(f"   HTML长度: {len(response.text)} 字符")

            if response.status_code == 200:
                html_content = response.text

                if 'vrwrap' in html_content:
                    print("   ✅ 检测到Sogou搜索结果容器 (vrwrap)")
                    soup = BeautifulSoup(html_content, 'html.parser')
                    results = soup.find_all('div', class_='vrwrap')
                    print(f"   检测到 {len(results)} 个搜索结果")
                elif '验证码' in html_content or '安全验证' in html_content:
                    print("   ⚠️  检测到反爬虫验证页面")
                else:
                    print("   ⚠️  未检测到Sogou搜索结果容器")
                    print(f"   HTML预览: {html_content[:300]}...")

                return html_content
        except Exception as e:
            print(f"   请求异常: {e}")
            return None

    def run_debug_tests(self):
        print("\n" + "=" * 60)
        print("🔧 Sogou调试测试")
        print("=" * 60)

        status_code, response_data = self.debug_sogou_raw_response()

        print("\n" + "-" * 60)

        raw_html = self.debug_manual_sogou_html()

        print("\n📋 调试结论:")
        if status_code == 200:
            if isinstance(response_data, dict) and len(response_data.get('results', [])) == 0:
                print("   1. API接口正常 (返回200)，但解析结果为空")
                print("   2. 可能是解析逻辑问题或Sogou反爬虫导致")
        else:
            print("   1. API接口存在问题")

        if raw_html:
            if 'vrwrap' in raw_html:
                print("   2. Sogou网站返回了正常搜索结果")
                print("   3. 问题可能在解析逻辑或请求参数")
            else:
                print("   2. Sogou网站可能返回了反爬虫页面")


class GoogleTester:
    def __init__(self):
        self.session = requests.Session()
        self.session.headers.update({
            "Authorization": f"Bearer {API_KEY}",
            "X-Team-ID": TEAM_ID,
            "Content-Type": "application/json"
        })

    def test_google_search(self, query: str = "test", limit: int = 5) -> Dict:
        print(f"\n   🔍 Google搜索: {query}")
        payload = {"query": query, "engine": "google", "limit": limit}

        try:
            response = self.session.post(f"{BASE_URL}/v1/search", json=payload, timeout=30)
            print(f"   状态码: {response.status_code}")

            if response.status_code == 200:
                result = response.json()
                count = len(result.get('results', []))
                print(f"   ✅ 找到 {count} 个结果")
                for i, r in enumerate(result.get('results', [])[:2]):
                    print(f"   [{i + 1}] {r.get('title', 'N/A')[:40]}")
                return {"success": True, "count": count}
            else:
                print(f"   ❌ 失败: {response.text[:100]}")
                return {"success": False, "error": response.text}
        except Exception as e:
            print(f"   ❌ 异常: {str(e)}")
            return {"success": False, "error": str(e)}

    def test_multiple_queries(self):
        print("\n[Google] 测试多个查询...")
        queries = ["Hello World", "Python programming", "web development"]

        for i, query in enumerate(queries):
            self.test_google_search(query, 3)
            if i < len(queries) - 1:
                time.sleep(3)

    def run_google_tests(self):
        print("\n" + "=" * 60)
        print("🔍 Google搜索测试")
        print("=" * 60)
        self.test_google_search("Hello World", 5)
        self.test_multiple_queries()


def generate_report(results: TestResults) -> Dict:
    return {
        "test_summary": {
            "total_tests": results.total,
            "passed": results.passed,
            "failed": results.failed,
            "pass_rate": f"{(results.passed / results.total * 100):.1f}%" if results.total > 0 else "0%",
            "timestamp": datetime.now().isoformat()
        },
        "passed_interfaces": results.get_passed_interfaces(),
        "failed_interfaces": results.get_failed_interfaces(),
        "all_results": results.results
    }


def print_detailed_report(report: Dict):
    print("\n" + "=" * 80)
    print("详细测试报告")
    print("=" * 80)

    print("\n【成功的接口】")
    print("-" * 80)
    passed = report["passed_interfaces"]
    if passed:
        for i, item in enumerate(passed, 1):
            print(f"{i:2}. {item['method']:6} {item['path']:40} [{item['response_code']}]")
    else:
        print("  无")
    print(f"\n  共计 {len(passed)} 个接口测试通过")

    print("\n【失败的接口】")
    print("-" * 80)
    failed = report["failed_interfaces"]
    if failed:
        for i, item in enumerate(failed, 1):
            error_msg = item.get('error', 'Unknown error')
            print(f"{i:2}. {item['method']:6} {item['path']:40}")
            print(f"    错误: {error_msg} [Code: {item['response_code']}]")
    else:
        print("  无")
    print(f"\n  共计 {len(failed)} 个接口测试失败")
    print("\n" + "=" * 80)


def run_quick_test():
    print("\n" + "=" * 60)
    print("🚀 快速验证测试")
    print("=" * 60)

    sogou_tester = SogouTester()
    sogou_tester.test_sogou_search()

    time.sleep(3)

    google_tester = GoogleTester()
    google_tester.test_google_search("Hello World", 3)

    print("\n📊 快速测试完成")


def run_full_test():
    print("\n" + "=" * 80)
    print("🧪 完整API测试")
    print("=" * 80)
    tester = APITester()
    results = tester.run_all_tests()
    report = generate_report(results)
    print_detailed_report(report)
    return results


def run_search_engine_tests():
    print("\n" + "=" * 60)
    print("🔍 搜索引擎测试")
    print("=" * 60)

    sogou_tester = SogouTester()
    sogou_tester.test_all_search_engines()
    sogou_tester.test_sogou_url_accessibility()
    sogou_tester.test_sogou_parsing()


def run_debug_tests():
    print("\n" + "=" * 60)
    print("🔧 Sogou调试测试")
    print("=" * 60)
    debug_tester = SogouDebugTester()
    debug_tester.run_debug_tests()


def show_menu():
    print("\n" + "=" * 60)
    print("📋 测试脚本菜单")
    print("=" * 60)
    print("1. 完整API测试 (所有接口)")
    print("2. 快速测试 (Sogou + Google)")
    print("3. 搜索引擎专项测试")
    print("4. 搜索引擎调试测试")
    print("5. 运行所有测试")
    print("0. 退出")
    print("=" * 60)


def main():
    if len(sys.argv) > 1:
        mode = sys.argv[1]
        if mode == "full":
            run_full_test()
            return
        elif mode == "quick":
            run_quick_test()
            return
        elif mode == "search":
            run_search_engine_tests()
            return
        elif mode == "debug":
            run_debug_tests()
            return
        elif mode == "all":
            run_full_test()
            run_search_engine_tests()
            return

    while True:
        show_menu()
        choice = input("请选择测试模式 [0-5]: ").strip()

        if choice == "1":
            run_full_test()
        elif choice == "2":
            run_quick_test()
        elif choice == "3":
            run_search_engine_tests()
        elif choice == "4":
            run_debug_tests()
        elif choice == "5":
            print("\n运行所有测试...")
            run_full_test()
            print()
            run_search_engine_tests()
        elif choice == "0":
            print("退出测试")
            break
        else:
            print("无效选择，请重新输入")

        input("\n按回车键继续...")


if __name__ == "__main__":
    main()
