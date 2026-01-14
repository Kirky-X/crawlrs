#!/bin/bash
set -e

cd "$(dirname "$0")/.."

echo "=== 启动 Crawlrs 测试环境 ==="

# 启动基础设施
echo "启动 PostgreSQL 和 Redis..."
docker-compose -f docker-compose.test.full.yml up -d postgres redis

# 等待基础设施就绪
echo "等待服务就绪..."
sleep 10

# 启动浏览器服务
echo "启动 Chrome 和 FlareSolverr..."
docker-compose -f docker-compose.test.full.yml up -d chrome flaresolverr

# 等待浏览器服务就绪
echo "等待浏览器服务就绪..."
sleep 15

# 启动 MinIO
echo "启动 MinIO..."
docker-compose -f docker-compose.test.full.yml up -d minio

# 启动应用
echo "启动 Crawlrs 应用..."
docker-compose -f docker-compose.test.full.yml up -d crawlrs

# 等待应用就绪
echo "等待应用启动..."
sleep 10

# 运行健康检查
echo "运行健康检查..."
./scripts/health-check.sh

echo ""
echo "=== 测试环境启动完成 ==="
echo "API 服务: http://localhost:3000"
echo "Prometheus: http://localhost:9090"
echo "Grafana: http://localhost:3001"
echo "MinIO: http://localhost:9000"
echo "Chrome DevTools: http://localhost:9222"
