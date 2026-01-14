#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 测试环境清理主脚本
# =============================================================================
# 协调清理所有测试环境：数据库、Redis、文件系统
#
# 使用方法:
#   ./cleanup-test-env.sh              # 清理所有（数据库+Redis+文件）
#   ./cleanup-test-env.sh --db-only    # 仅清理数据库
#   ./cleanup-test-env.sh --redis-only # 仅清理 Redis
#   ./cleanup-test-env.sh --files-only # 仅清理文件
#   ./cleanup-test-env.sh --verify     # 仅验证（不清理）
# =============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# 脚本目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# 默认配置
DB_CLEANUP=true
REDIS_CLEANUP=true
FILES_CLEANUP=true
PARALLEL_CLEANUP=true
TIMEOUT_SECONDS=30

# 日志函数
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_section() {
    echo -e "\n${CYAN}========================================${NC}"
    echo -e "${CYAN}$1${NC}"
    echo -e "${CYAN}========================================${NC}\n"
}

# 检查依赖工具
check_dependencies() {
    log_info "检查依赖工具..."

    local missing=()

    if ! command -v psql &> /dev/null; then
        missing+=("psql")
    fi

    if ! command -v redis-cli &> /dev/null; then
        missing+=("redis-cli")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        log_error "缺少依赖工具: ${missing[*]}"
        log_warning "请安装相应的客户端工具"
        return 1
    fi

    log_success "所有依赖工具可用"
    return 0
}

# 清理数据库
cleanup_database() {
    log_section "清理数据库"

    if [ "$DB_CLEANUP" = false ]; then
        log_info "跳过数据库清理"
        return 0
    fi

    local start_time=$(date +%s)

    if "$SCRIPT_DIR/cleanup-db.sh" --verify &>/dev/null; then
        log_success "数据库已经是干净的"
    else
        if "$SCRIPT_DIR/cleanup-db.sh"; then
            log_success "数据库清理完成"
        else
            log_error "数据库清理失败"
            return 1
        fi
    fi

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    if [ "$duration" -gt "$TIMEOUT_SECONDS" ]; then
        log_warning "数据库清理耗时 ${duration}秒，超过目标 ${TIMEOUT_SECONDS}秒"
    else
        log_info "数据库清理耗时 ${duration}秒"
    fi

    return 0
}

# 清理 Redis
cleanup_redis() {
    log_section "清理 Redis"

    if [ "$REDIS_CLEANUP" = false ]; then
        log_info "跳过 Redis 清理"
        return 0
    fi

    local start_time=$(date +%s)

    if "$SCRIPT_DIR/cleanup-redis.sh" --verify &>/dev/null; then
        log_success "Redis 已经是干净的"
    else
        if "$SCRIPT_DIR/cleanup-redis.sh"; then
            log_success "Redis 清理完成"
        else
            log_error "Redis 清理失败"
            return 1
        fi
    fi

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    if [ "$duration" -gt "$TIMEOUT_SECONDS" ]; then
        log_warning "Redis 清理耗时 ${duration}秒，超过目标 ${TIMEOUT_SECONDS}秒"
    else
        log_info "Redis 清理耗时 ${duration}秒"
    fi

    return 0
}

# 清理文件系统
cleanup_files() {
    log_section "清理文件系统"

    if [ "$FILES_CLEANUP" = false ]; then
        log_info "跳过文件系统清理"
        return 0
    fi

    local start_time=$(date +%s)

    if "$SCRIPT_DIR/cleanup-files.sh" --verify &>/dev/null; then
        log_success "文件系统已经是干净的"
    else
        if "$SCRIPT_DIR/cleanup-files.sh"; then
            log_success "文件系统清理完成"
        else
            log_error "文件系统清理失败"
            return 1
        fi
    fi

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))
    log_info "文件系统清理耗时 ${duration}秒"

    return 0
}

# 并行清理
cleanup_parallel() {
    log_section "并行清理所有环境"

    local start_time=$(date +%s)
    local failed=0

    # 并行启动清理任务
    if [ "$DB_CLEANUP" = true ]; then
        "$SCRIPT_DIR/cleanup-db.sh" > /tmp/cleanup-db.log 2>&1 &
        local db_pid=$!
    fi

    if [ "$REDIS_CLEANUP" = true ]; then
        "$SCRIPT_DIR/cleanup-redis.sh" > /tmp/cleanup-redis.log 2>&1 &
        local redis_pid=$!
    fi

    if [ "$FILES_CLEANUP" = true ]; then
        "$SCRIPT_DIR/cleanup-files.sh" > /tmp/cleanup-files.log 2>&1 &
        local files_pid=$!
    fi

    # 等待所有任务完成
    local pids=()
    if [ "$DB_CLEANUP" = true ]; then
        pids+=("$db_pid")
    fi
    if [ "$REDIS_CLEANUP" = true ]; then
        pids+=("$redis_pid")
    fi
    if [ "$FILES_CLEANUP" = true ]; then
        pids+=("$files_pid")
    fi

    for pid in "${pids[@]}"; do
        if ! wait "$pid"; then
            failed=$((failed + 1))
        fi
    done

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    if [ "$failed" -eq 0 ]; then
        log_success "并行清理完成 (${duration}秒)"
        return 0
    else
        log_error "$failed 个清理任务失败"
        # 显示失败任务的日志
        [ "$DB_CLEANUP" = true ] && [ $db_pid -gt 0 ] && cat /tmp/cleanup-db.log
        [ "$REDIS_CLEANUP" = true ] && [ $redis_pid -gt 0 ] && cat /tmp/cleanup-redis.log
        [ "$FILES_CLEANUP" = true ] && [ $files_pid -gt 0 ] && cat /tmp/cleanup-files.log
        return 1
    fi
}

# 验证清理结果
verify_cleanup() {
    log_section "验证清理结果"

    local all_passed=true

    if [ "$DB_CLEANUP" = true ]; then
        log_info "验证数据库..."
        if ! "$SCRIPT_DIR/cleanup-db.sh" --verify; then
            all_passed=false
        fi
    fi

    if [ "$REDIS_CLEANUP" = true ]; then
        log_info "验证 Redis..."
        if ! "$SCRIPT_DIR/cleanup-redis.sh" --verify; then
            all_passed=false
        fi
    fi

    if [ "$FILES_CLEANUP" = true ]; then
        log_info "验证文件系统..."
        if ! "$SCRIPT_DIR/cleanup-files.sh" --verify; then
            all_passed=false
        fi
    fi

    if [ "$all_passed" = true ]; then
        log_success "所有验证通过"
        return 0
    else
        log_error "验证失败"
        return 1
    fi
}

# 显示清理状态
show_status() {
    log_section "清理前状态"

    log_info "数据库:"
    "$SCRIPT_DIR/cleanup-db.sh" --status 2>/dev/null || log_warning "无法获取数据库状态"

    echo ""
    log_info "Redis:"
    "$SCRIPT_DIR/cleanup-redis.sh" --status 2>/dev/null || log_warning "无法获取 Redis 状态"

    echo ""
    log_info "文件系统:"
    "$SCRIPT_DIR/cleanup-files.sh" --status 2>/dev/null || log_warning "无法获取文件系统状态"
}

# 显示帮助信息
show_help() {
    echo "Crawlrs 测试环境清理主脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  (无)           清理所有测试环境（数据库+Redis+文件）"
    echo "  --db-only      仅清理数据库"
    echo "  --redis-only   仅清理 Redis"
    echo "  --files-only   仅清理文件系统"
    echo "  --status       显示清理前状态"
    echo "  --verify       仅验证（不清理）"
    echo "  --help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --sequential   顺序清理（默认并行）"
    echo "  --timeout SEC  清理超时时间 (默认: 30秒)"
    echo ""
    echo "示例:"
    echo "  $0                           # 并行清理所有"
    echo "  $0 --db-only                 # 仅清理数据库"
    echo "  $0 --sequential              # 顺序清理（便于调试）"
    echo "  $0 --verify                  # 仅验证"
    echo "  $0 --status                  # 查看状态"
}

# 主函数
main() {
    local cleanup_db=true
    local cleanup_redis=true
    local cleanup_files=true
    local verify_only=false
    local show_status_flag=false

    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            --db-only)
                cleanup_db=true
                cleanup_redis=false
                cleanup_files=false
                shift
                ;;
            --redis-only)
                cleanup_db=false
                cleanup_redis=true
                cleanup_files=false
                shift
                ;;
            --files-only)
                cleanup_db=false
                cleanup_redis=false
                cleanup_files=true
                shift
                ;;
            --sequential)
                PARALLEL_CLEANUP=false
                shift
                ;;
            --verify)
                verify_only=true
                shift
                ;;
            --status)
                show_status_flag=true
                shift
                ;;
            --timeout)
                TIMEOUT_SECONDS="$2"
                shift 2
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            *)
                log_error "未知参数: $1"
                show_help
                exit 1
                ;;
        esac
    done

    echo ""
    log_section "Crawlrs 测试环境清理"
    echo "数据库: $cleanup_db | Redis: $cleanup_redis | 文件: $cleanup_files"
    echo "模式: $([ "$PARALLEL_CLEANUP" = true ] && echo "并行" || echo "顺序")"
    echo "超时: ${TIMEOUT_SECONDS}秒"

    # 检查依赖
    if ! check_dependencies; then
        exit 1
    fi

    if [ "$show_status_flag" = true ]; then
        show_status
        exit 0
    fi

    if [ "$verify_only" = true ]; then
        verify_cleanup
        exit $?
    fi

    # 执行清理
    local start_time=$(date +%s)
    local cleanup_failed=false

    if [ "$PARALLEL_CLEANUP" = true ]; then
        if ! cleanup_parallel; then
            cleanup_failed=true
        fi
    else
        if [ "$cleanup_db" = true ]; then
            if ! cleanup_database; then
                cleanup_failed=true
            fi
        fi

        if [ "$cleanup_redis" = true ]; then
            if ! cleanup_redis; then
                cleanup_failed=true
            fi
        fi

        if [ "$cleanup_files" = true ]; then
            if ! cleanup_files; then
                cleanup_failed=true
            fi
        fi
    fi

    local end_time=$(date +%s)
    local total_duration=$((end_time - start_time))

    echo ""
    log_section "清理总结"
    echo "总耗时: ${total_duration}秒"

    # 最终验证
    if [ "$cleanup_failed" = false ]; then
        if verify_cleanup; then
            log_success "测试环境清理完成"
            exit 0
        else
            log_error "验证失败"
            exit 1
        fi
    else
        log_error "清理过程中出现错误"
        exit 1
    fi
}

# 启动
main "$@"
