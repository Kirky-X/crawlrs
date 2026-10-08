# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

import pytest
import os
import sys
from typing import Generator

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from api_test_framework import CrawlrsAPIClient


@pytest.fixture(scope="session", autouse=True)
def geo_restrictions_disabled(api_client) -> Generator[None, None, None]:
    """会话开始时关闭团队地理限制（显式恢复基线，防跨套件状态泄漏）。

    test_api_endpoints 的 geo 用例会开启限制；一旦残留开启，后续所有真实
    爬取/搜索都会在 geo 校验处因出口 IP 无法定位（默认 ipapi.co 对受限
    网络返回 403）而 fail-closed 500。这里在会话入口统一关闭；PUT 失败仅
    警告不阻断——geo 可能本就关闭，或 key 无 teams 写权限（此时用例以
    实际行为为准）。
    """
    result = api_client.update_team_geo_restrictions({"enable_geo_restrictions": False})
    if not result.success:
        print(
            f"\n[conftest] 警告: 关闭地理限制未成功 "
            f"(status={result.response.status_code})，真实爬取用例可能受 geo 校验影响"
        )
    yield


@pytest.fixture(scope="session")
def api_client() -> Generator[CrawlrsAPIClient, None, None]:
    """提供 API 客户端实例"""
    base_url = os.getenv("CRAWLRS_API_URL", "http://localhost:8899")
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
        "url": "https://text.npr.org",
        "options": {"timeout": 30000, "extract_text": True},
    }


@pytest.fixture
def scrape_test_params():
    """抓取测试参数"""
    return {
        "url": "https://text.npr.org",
        "options": {"wait_for": 1000, "timeout": 30},
    }


@pytest.fixture
def extract_test_params():
    """提取测试参数"""
    return {
        "url": "https://text.npr.org",
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
