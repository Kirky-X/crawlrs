#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 测试环境管理脚本
# =============================================================================
# 启动、停止测试环境，或初始化测试数据
#
# 使用方法:
#   ./scripts/test-env.sh start    # 启动测试环境
#   ./scripts/test-env.sh stop     # 停止测试环境
#   ./scripts/test-env.sh init     # 初始化测试数据
#   ./scripts/test-env.sh all      # 启动环境并初始化数据
#   ./scripts/test-env.sh status   # 检查服务状态
#   ./scripts/test-env.sh help     # 显示帮助
# =============================================================================

set -e

cd "$(dirname "$0")/.."

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() {
    echo -e "${CYAN}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1"
}

log_section() {
    echo ""
    echo -e "${YELLOW}========================================${NC}"
    echo -e "${YELLOW}$1${NC}"
    echo -e "${YELLOW}========================================${NC}"
    echo ""
}

# 启动测试环境
start_env() {
    log_section "启动 Crawlrs 测试环境"

    if ! docker info >/dev/null 2>&1; then
        log_error "Docker 未运行，请先启动 Docker"
        exit 1
    fi

    # 启动基础设施
    log_info "启动 PostgreSQL..."
    docker-compose -f docker/docker-compose.test.yml up -d test-db

    # 等待基础设施就绪
    log_info "等待服务就绪..."
    sleep 10

    # 启动浏览器服务
    log_info "启动 Chrome 和 FlareSolverr..."
    docker-compose -f docker/docker-compose.test.yml up -d chrome flaresolverr

    # 等待浏览器服务就绪
    log_info "等待浏览器服务就绪..."
    sleep 15

    # 启动 MinIO
    log_info "启动 MinIO..."
    docker-compose -f docker/docker-compose.test.yml up -d minio

    # 启动应用
    log_info "启动 Crawlrs 应用..."
    docker-compose -f docker/docker-compose.test.yml up -d crawlrs

    # 等待应用就绪
    log_info "等待应用启动..."
    sleep 10

    # 运行健康检查
    log_info "运行健康检查..."
    ./scripts/health-check.sh

    log_section "测试环境启动完成"
    echo "API 服务:      http://localhost:3000"
    echo "PostgreSQL:    localhost:5443"
    echo "Chrome:        localhost:9223"
    echo "MinIO:         localhost:9000"
    echo "Prometheus:    localhost:9090"
    echo "Grafana:       localhost:3001"
}

# 停止测试环境
stop_env() {
    log_section "停止测试环境"

    log_info "停止并清理 Docker 容器..."
    docker-compose -f docker/docker-compose.test.yml down -v --remove-orphans 2>/dev/null || true

    log_success "测试环境已停止"
}

# 初始化测试数据
init_data() {
    log_section "初始化测试数据"

    # 数据库连接配置
    DB_HOST="${DB_HOST:-localhost}"
    DB_PORT="${DB_PORT:-5443}"
    DB_NAME="${DB_NAME:-crawlrs_test}"
    DB_USER="${DB_USER:-crawlrs}"
    DB_PASSWORD="${DB_PASSWORD:-password}"

    # 测试API密钥
    TEST_API_KEY="test_api_key_$(date +%s)"
    TEAM_ID="a1b2c3d4-e5f6-7890-abcd-ef1234567890"

    log_info "API Key: $TEST_API_KEY"
    log_info "Team ID: $TEAM_ID"

    # 执行SQL初始化
    log_info "插入测试数据..."
    PGPASSWORD="$DB_PASSWORD" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" << EOF
-- 插入测试团队
INSERT INTO teams (id, name, created_at, updated_at)
VALUES ('$TEAM_ID'::uuid, 'Test Team', NOW(), NOW())
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = NOW();

-- 插入测试API密钥
INSERT INTO api_keys (id, key, team_id, created_at, updated_at)
VALUES (gen_random_uuid(), '$TEST_API_KEY', '$TEAM_ID'::uuid, NOW(), NOW())
ON CONFLICT (key) DO UPDATE SET team_id = EXCLUDED.team_id, updated_at = NOW();

-- 插入测试积分余额 (credits)
INSERT INTO credits (team_id, total_credits, used_credits, created_at, updated_at)
VALUES ('$TEAM_ID'::uuid, 10000, 0, NOW(), NOW())
ON CONFLICT (team_id) DO UPDATE SET total_credits = EXCLUDED.total_credits, used_credits = EXCLUDED.used_credits, updated_at = NOW();

-- 插入积分交易记录
INSERT INTO credits_transactions (id, team_id, api_key_id, amount, transaction_type, description, created_at)
VALUES (gen_random_uuid(), '$TEAM_ID'::uuid, (SELECT id FROM api_keys WHERE key = '$TEST_API_KEY'), 10000, 'credit', 'Initial test credits', NOW())
ON CONFLICT DO NOTHING;
EOF

    log_success "测试数据初始化完成!"
    echo "API Key: $TEST_API_KEY"
    echo "Team ID: $TEAM_ID"
}

# 显示服务状态
show_status() {
    log_section "服务状态"

    docker-compose -f docker/docker-compose.test.yml ps

    echo ""
    log_info "运行健康检查..."
    ./scripts/health-check.sh || true
}

# 显示帮助
show_help() {
    echo "Crawlrs 测试环境管理脚本"
    echo ""
    echo "使用方法: $0 <命令>"
    echo ""
    echo "命令:"
    echo "  start   启动测试环境"
    echo "  stop    停止测试环境"
    echo "  init    初始化测试数据"
    echo "  all     启动环境并初始化数据"
    echo "  status  显示服务状态"
    echo "  help    显示此帮助信息"
    echo ""
    echo "示例:"
    echo "  $0 start     # 启动测试环境"
    echo "  $0 stop      # 停止测试环境"
    echo "  $0 all       # 启动环境并初始化数据"
}

# 主函数
main() {
    local command=${1:-help}

    case $command in
        start)
            start_env
            ;;
        stop)
            stop_env
            ;;
        init)
            init_data
            ;;
        all)
            start_env
            init_data
            ;;
        status)
            show_status
            ;;
        help|--help|-h)
            show_help
            exit 0
            ;;
        *)
            log_error "未知命令: $command"
            show_help
            exit 1
            ;;
    esac
}

main "$@"
