#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 测试运行脚本
# =============================================================================
# 支持本地 Python 测试和 Docker 内 Rust 测试
#
# 使用方法:
#   ./scripts/run-tests.sh local    # 本地 Python 测试
#   ./scripts/run-tests.sh docker   # Docker 内 Rust 测试
#   ./scripts/run-tests.sh full     # 完整测试流程
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

# Docker 内 Rust 测试
run_docker_tests() {
    log_section "运行 Docker 内 Rust 测试"

    if ! docker info >/dev/null 2>&1; then
        log_error "Docker 未运行，请先启动 Docker"
        exit 1
    fi

    # 清理并启动测试环境
    log_info "启动测试环境..."
    ./scripts/test-env.sh stop 2>/dev/null || true
    ./scripts/test-env.sh start

    log_info "运行 Rust 测试..."
    docker-compose -f docker/docker-compose.test.yml run --rm test-runner

    log_success "Docker 测试完成!"

    log_info "清理测试环境..."
    ./scripts/test-env.sh stop
}

# 完整测试流程
run_full_tests() {
    log_section "Crawlrs 完整测试流程"

    if ! docker info >/dev/null 2>&1; then
        log_error "Docker 未运行，请先启动 Docker"
        exit 1
    fi

    # 步骤 1: 启动测试环境
    log_section "步骤 1: 启动测试环境"
    ./scripts/test-env.sh stop 2>/dev/null || true
    ./scripts/test-env.sh start

    # 步骤 2: 初始化测试数据
    log_section "步骤 2: 初始化测试数据"
    ./scripts/test-env.sh init

    # 步骤 3: 运行 Python 测试
    log_section "步骤 3: 运行 Python 测试"
    run_local_tests

    # 步骤 4: 生成测试报告
    log_section "步骤 4: 生成测试报告"
    if [ -f test-results/report.json ]; then
        log_success "测试报告已生成: test-results/report.json"
    fi

    # 步骤 5: 运行 Rust 测试
    log_section "步骤 5: 运行 Rust 测试"
    docker-compose -f docker/docker-compose.test.yml run --rm test-runner cargo test --release --features full

    # 步骤 6: 清理环境
    log_section "步骤 6: 清理环境"
    ./scripts/test-env.sh stop

    log_section "完整测试流程完成!"
    echo ""
    echo "测试报告: test-results/report.json"
}

# 显示帮助
show_help() {
    echo "Crawlrs 测试运行脚本"
    echo ""
    echo "使用方法: $0 <命令>"
    echo ""
    echo "命令:"
    echo "  local   运行本地 Python 测试"
    echo "  docker  运行 Docker 内 Rust 测试"
    echo "  full    完整测试流程 (推荐)"
    echo "  help    显示此帮助信息"
    echo ""
    echo "示例:"
    echo "  $0 local   # 仅运行 Python 测试"
    echo "  $0 docker  # 运行 Docker 内 Rust 测试"
    echo "  $0 full    # 运行完整测试流程"
}

# 主函数
main() {
    local command=${1:-help}

    case $command in
        local)
            run_local_tests
            ;;
        docker)
            run_docker_tests
            ;;
        full)
            run_full_tests
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
