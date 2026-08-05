#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 测试运行脚本
# =============================================================================
# 运行本地 Python 测试
#
# 使用方法:
#   ./scripts/run-tests.sh local    # 本地 Python 测试
#   ./scripts/run-tests.sh help     # 显示帮助
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

# 本地 Python 测试
run_local_tests() {
    log_section "运行本地 Python 测试"

    if ! command -v python3 &> /dev/null; then
        log_error "Python3 未安装"
        exit 1
    fi

    log_info "检查测试依赖..."
    if [ -f tests/python/requirements.txt ]; then
        pip install -r tests/python/requirements.txt -q
    fi

    # 创建测试结果目录
    mkdir -p test-results

    # 运行 API 测试
    log_info "运行 API 测试..."
    if [ -f tests/python/test_api_endpoints.py ]; then
        python -m pytest tests/python/test_api_endpoints.py -v --tb=short
    else
        log_info "跳过 API 测试 (文件不存在)"
    fi

    # 运行性能测试
    log_info "运行性能测试..."
    if [ -f tests/python/test_performance.py ]; then
        python -m pytest tests/python/test_performance.py -v --tb=short
    else
        log_info "跳过性能测试 (文件不存在)"
    fi

    # 运行错误处理测试
    log_info "运行错误处理测试..."
    if [ -f tests/python/test_error_handling.py ]; then
        python -m pytest tests/python/test_error_handling.py -v --tb=short
    else
        log_info "跳过错误处理测试 (文件不存在)"
    fi

    log_success "Python 测试完成!"
}


# 显示帮助
show_help() {
    echo "Crawlrs 测试运行脚本"
    echo ""
    echo "使用方法: $0 <命令>"
    echo ""
    echo "命令:"
    echo "  local   运行本地 Python 测试"
    echo "  help    显示此帮助信息"
    echo ""
    echo "示例:"
    echo "  $0 local   # 运行 Python 测试"
}

# 主函数
main() {
    local command=${1:-help}

    case $command in
        local)
            run_local_tests
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
