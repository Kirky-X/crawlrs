#!/bin/bash
# Docker 内部测试脚本

set -e

echo "=== 安装系统依赖 ==="
apt-get update -qq
apt-get install -y -qq \
    build-essential \
    pkg-config \
    libssl-dev \
    libpq-dev \
    redis-server \
    ca-certificates \
    curl \
    gnupg \
    lsb-release

echo "=== 配置 Redis ==="
# 启动 Redis 服务
redis-server --daemonize yes --port 6379
sleep 2
redis-cli PING || exit 1

echo "=== 运行测试 ==="
export CHROMIUM_REMOTE_DEBUGGING_URL="http://host.docker.internal:3000"
export CHROME_HOST="host.docker.internal"
export CHROME_PORT=3000

# 运行测试
cargo test --release 2>&1

echo "=== 测试完成 ==="
