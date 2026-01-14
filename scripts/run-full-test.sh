#!/bin/bash
set -e

cd "$(dirname "$0")/.."

echo "========================================"
echo "Crawlrs 完整测试执行"
echo "========================================"

if ! docker info >/dev/null 2>&1; then
    echo "Docker 未运行，请先启动 Docker"
    exit 1
fi

echo "步骤 1: 启动测试环境"
docker-compose -f docker-compose.test.full.yml down -v 2>/dev/null || true
docker-compose -f docker-compose.test.full.yml up -d
echo "等待服务启动..."
sleep 30

echo "运行健康检查..."
if ! ./scripts/health-check.sh; then
    echo "服务健康检查失败"
    exit 1
fi

echo "步骤 2: 安装 Python 测试依赖"
pip install -r tests/python/requirements.txt

echo "步骤 3: 运行 API 测试"
mkdir -p test-results
python -m pytest tests/python/test_api_endpoints.py -v --tb=short

echo "步骤 4: 运行性能测试"
python -m pytest tests/python/test_performance.py -v --tb=short

echo "步骤 5: 运行错误处理测试"
python -m pytest tests/python/test_error_handling.py -v --tb=short

echo "步骤 6: 生成测试报告"
python -c "from tests.python.api_test_framework import CrawlrsAPIClient; c = CrawlrsAPIClient(); c.generate_report('test-results/report.json')"

echo ""
echo "========================================"
echo "测试执行完成！"
echo "========================================"
echo "测试报告: test-results/report.json"
echo ""
echo "清理测试环境..."
docker-compose -f docker-compose.test.full.yml down -v
echo "所有测试完成！"
