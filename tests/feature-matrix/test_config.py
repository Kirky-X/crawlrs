# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

"""
特性测试矩阵配置

定义不同的测试配置组合。

轴口径（2026-11-08 与 docs/FEATURE_MATRIX.md §5 同步）：
- 服务轴以 docker/docker-compose.test.yml 为准：test-db（PostgreSQL）、
  chrome、flaresolverr、test-runner；
- Cargo feature 轴（auth/webhook/metrics/...）见 docs/FEATURE_MATRIX.md，
  属构建面而非容器服务面，两者不混写。
"""

from dataclasses import dataclass
from typing import List
from enum import Enum


class Feature(Enum):
    BROWSER = "browser"
    FLARESOLVERR = "flaresolverr"
    POSTGRES = "postgres"


@dataclass
class TestConfiguration:
    name: str
    description: str
    enabled_features: List[Feature]
    expected_services: List[str]


CONFIGURATIONS = {
    "db_only": TestConfiguration(
        name="db_only",
        description="仅 PostgreSQL（BDD/集成测试最小轴）",
        enabled_features=[Feature.POSTGRES],
        expected_services=["postgres"],
    ),
    "browser_only": TestConfiguration(
        name="browser_only",
        description="浏览器引擎轴（chrome + flaresolverr）",
        enabled_features=[
            Feature.BROWSER,
            Feature.FLARESOLVERR,
            Feature.POSTGRES,
        ],
        expected_services=["postgres", "chrome", "flaresolverr"],
    ),
    "full_services": TestConfiguration(
        name="full_services",
        description="全部服务轴（与 browser_only 当前同构，保留扩展位）",
        enabled_features=[
            Feature.BROWSER,
            Feature.FLARESOLVERR,
            Feature.POSTGRES,
        ],
        expected_services=["postgres", "chrome", "flaresolverr"],
    ),
}


def get_configuration(name: str):
    """获取指定配置"""
    return CONFIGURATIONS.get(name)


def list_configurations():
    """列出所有可用配置"""
    return list(CONFIGURATIONS.keys())
