#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 健康检查脚本
# =============================================================================
# 检查所有测试服务是否正常运行
#
# 使用方法:
#   ./scripts/health-check.sh              # 检查所有服务
#   ./scripts/health-check.sh postgres     # 仅检查 postgres
# =============================================================================

set -e

# 服务列表: 服务名:端口
SERVICES=(
    "postgres:5432"
    "chrome:9222"
    "flaresolverr:8191"
    "minio:9000"
    "crawlrs:3000"
)

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1"
}

# 检查单个服务
check_service() {
    local service=$1
    IFS=':' read -r host port <<< "$service"
    echo -n "检查 $host:$port ... "
    
    if nc -z -w5 "$host" "$port" 2>/dev/null; then
        log_success "健康"
        return 0
    else
        log_error "不可用"
        return 1
    fi
}

# 显示帮助
show_help() {
    echo "Crawlrs 服务健康检查"
    echo ""
    echo "使用方法: $0 [服务名]"
    echo ""
    echo "服务:"
    for service in "${SERVICES[@]}"; do
        IFS=':' read -r host port <<< "$service"
        echo "  - $host"
    done
    echo ""
    echo "示例:"
    echo "  $0              # 检查所有服务"
    echo "  $0 postgres     # 仅检查 postgres"
}

# 主函数
main() {
    local target_service=$1
    local failed=0
    
    echo ""
    echo "=== Crawlrs 服务健康检查 ==="
    echo ""
    
    if [ -n "$target_service" ]; then
        # 检查指定服务
        for service in "${SERVICES[@]}"; do
            IFS=':' read -r host port <<< "$service"
            if [ "$host" == "$target_service" ]; then
                if ! check_service "$service"; then
                    failed=1
                fi
                break
            fi
        done
    else
        # 检查所有服务
        for service in "${SERVICES[@]}"; do
            if ! check_service "$service"; then
                failed=1
            fi
        done
    fi
    
    echo ""
    if [ $failed -eq 0 ]; then
        log_success "所有服务正常运行！"
        exit 0
    else
        log_error "部分服务不可用"
        exit 1
    fi
}

main "$@"
