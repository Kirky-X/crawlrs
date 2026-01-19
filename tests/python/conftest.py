import pytest
import os
import sys
from typing import Generator

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from api_test_framework import CrawlrsAPIClient


@pytest.fixture(scope="session")
def api_client() -> Generator[CrawlrsAPIClient, None, None]:
    """提供 API 客户端实例"""
    base_url = os.getenv("CRAWLRS_API_URL", "http://localhost:3000")
    api_key = os.getenv("CRAWLRS_API_KEY", os.getenv("TEST_API_KEY", "test-api-key"))

    client = CrawlrsAPIClient(base_url=base_url, api_key=api_key)

    yield client

    report_path = os.getenv("TEST_REPORT_PATH", "test-results/report.json")
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    client.generate_report(report_path)


@pytest.fixture
def search_test_params():
    """搜索测试参数"""
    return {"query": "test query", "engines": ["bing"], "limit": 5}


@pytest.fixture
def crawl_test_params():
    """爬取测试参数"""
    return {
        "url": "https://example.com",
        "options": {"timeout": 30000, "extract_text": True},
    }


@pytest.fixture
def scrape_test_params():
    """抓取测试参数"""
    return {
        "url": "https://example.com",
        "options": {"wait_for": 1000, "timeout": 30},
    }


@pytest.fixture
def extract_test_params():
    """提取测试参数"""
    return {
        "url": "https://example.com",
        "selectors": {"title": "h1", "paragraphs": "p"},
    }


@pytest.fixture
def webhook_test_params():
    """Webhook 测试参数"""
    return {
        "url": "https://httpbin.org/post",
        "events": ["task.completed", "task.failed"],
        "secret": os.getenv("TEST_WEBHOOK_SECRET", "test-secret"),
    }
