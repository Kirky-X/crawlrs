"""
特性测试矩阵配置

定义不同的测试配置组合
"""

from dataclasses import dataclass
from typing import List
from enum import Enum


class Feature(Enum):
    MINIO = "minio"
    BROWSER = "browser"
    SEARCH = "search"
    POSTGRES = "postgres"
    FLARESOLVERR = "flaresolverr"


@dataclass
class TestConfiguration:
    name: str
    description: str
    enabled_features: List[Feature]
    expected_services: List[str]


CONFIGURATIONS = {
    "minio_only": TestConfiguration(
        name="minio_only",
        description="仅启用 MinIO 存储服务",
        enabled_features=[Feature.MINIO, Feature.POSTGRES],
        expected_services=["postgres", "minio"],
    ),
    "browser_only": TestConfiguration(
        name="browser_only",
        description="仅启用浏览器服务",
        enabled_features=[
            Feature.BROWSER,
            Feature.FLARESOLVERR,
            Feature.POSTGRES,
        ],
        expected_services=["postgres", "chrome", "flaresolverr"],
    ),
    "search_only": TestConfiguration(
        name="search_only",
        description="仅启用搜索功能",
        enabled_features=[
            Feature.SEARCH,
            Feature.POSTGRES,
            Feature.FLARESOLVERR,
        ],
        expected_services=["postgres", "flaresolverr"],
    ),
    "full_features": TestConfiguration(
        name="full_features",
        description="启用所有功能",
        enabled_features=[
            Feature.MINIO,
            Feature.BROWSER,
            Feature.SEARCH,
            Feature.POSTGRES,
            Feature.FLARESOLVERR,
        ],
        expected_services=["postgres", "chrome", "flaresolverr", "minio"],
    ),
}


def get_configuration(name: str):
    """获取指定配置"""
    return CONFIGURATIONS.get(name)


def list_configurations():
    """列出所有可用配置"""
    return list(CONFIGURATIONS.keys())
