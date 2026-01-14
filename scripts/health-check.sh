#!/bin/bash
set -e

SERVICES=("postgres:5432" "redis:6379" "chrome:9222" "flaresolverr:8191" "minio:9000" "crawlrs:3000")

echo "=== Crawlrs 服务健康检查 ==="
for service in "${SERVICES[@]}"; do
    IFS=':' read -r host port <<< "$service"
    echo -n "检查 $host:$port ... "
    if nc -z -w5 "$host" "$port" 2>/dev/null; then
        echo "✓ 健康"
    else
        echo "✗ 不可用"
        exit 1
    fi
done
echo "所有服务正常运行！"
