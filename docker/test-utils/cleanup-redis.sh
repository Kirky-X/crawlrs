#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs Redis 清理脚本
# =============================================================================
# 用于在测试前清理 Redis 中的所有测试数据
#
# 使用方法:
#   ./cleanup-redis.sh                    # 清理所有数据
#   ./cleanup-redis.sh --partial pattern  # 仅清理匹配模式的key
#   ./cleanup-redis.sh --verify           # 仅验证（不清理）
# =============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# 默认配置 - 检测是否在 Docker 容器内运行
REDIS_HOST="${CRAWLRS__REDIS__HOST:-localhost}"
REDIS_PORT="${CRAWLRS__REDIS__PORT:-6380}"

# 检测是否在 Docker Compose 网络中（使用服务名）
if [ -f /.dockerenv ] || grep -q "docker\|containerd" /proc/1/cgroup 2>/dev/null; then
    # 在 Docker 容器内，使用服务名
    REDIS_HOST="${CRAWLRS__REDIS__HOST:-test-redis}"
    REDIS_PORT="${CRAWLRS__REDIS__PORT:-6379}"
fi

# 如果明确指定了环境变量，使用环境变量的值
[ -n "$CRAWLRS__REDIS__HOST" ] && REDIS_HOST="$CRAWLRS__REDIS__HOST"
[ -n "$CRAWLRS__REDIS__PORT" ] && REDIS_PORT="$CRAWLRS__REDIS__PORT"

# Crawlrs 使用的 key 前缀模式
KEY_PATTERNS=(
    "crawlrs:*"          # 通用前缀
    "rate:*"             # 速率限制
    "circuit:*"          # 熔断器状态
    "task:*"             # 任务状态
    "cache:*"            # 缓存数据
    "lock:*"             # 分布式锁
    "sess:*"             # 会话数据
    "limit:*"            # 限制计数器
)

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

# 构建 redis-cli 命令
build_redis_cmd() {
    echo "redis-cli -h $REDIS_HOST -p $REDIS_PORT"
}

# 检查 Redis 连接
check_redis_connection() {
    log_info "检查 Redis 连接..."

    if redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" PING 2>/dev/null | grep -q "PONG"; then
        log_success "Redis 连接成功"
        return 0
    else
        log_error "无法连接到 Redis"
        return 1
    fi
}

# 获取 key 数量
get_key_count() {
    local pattern=$1
    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" DBSIZE 2>/dev/null || echo "0"
}

# 获取所有 key 数量
get_total_key_count() {
    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" DBSIZE 2>/dev/null | tail -1 || echo "0"
}

# 清理匹配模式的 keys
cleanup_pattern() {
    local pattern=$1
    local count=$2

    if [ "$count" -eq 0 ]; then
        log_info "  ✓ 模式 '$pattern': 已经是空的"
        return 0
    fi

    log_info "清理模式: $pattern ($count keys)"

    # 使用 SCAN + DEL 以避免阻塞
    local deleted=0
    local cursor=0
    local batch_size=100

    while true; do
        local result=$(redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" SCAN "$cursor" MATCH "$pattern" COUNT "$batch_size")
        cursor=$(echo "$result" | head -1)
        local keys=$(echo "$result" | tail -1 | tr ',' ' ')

        if [ -n "$keys" ] && [ "$keys" != "" ]; then
            redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" DEL $keys &>/dev/null
            deleted=$((deleted + $(echo "$keys" | wc -w)))
        fi

        if [ "$cursor" -eq 0 ]; then
            break
        fi
    done

    log_success "  ✓ 已删除 $deleted keys"
    return 0
}

# 清理所有 Crawlrs 相关 keys
cleanup_all_crawlrs_keys() {
    local failed_patterns=()

    log_section "清理所有 Crawlrs 测试数据"

    for pattern in "${KEY_PATTERNS[@]}"; do
        local count=$(redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" --scan --pattern "$pattern" 2>/dev/null | wc -l)
        if [ "$count" -gt 0 ]; then
            if ! cleanup_pattern "$pattern" "$count"; then
                failed_patterns+=("$pattern")
            fi
        else
            log_info "  ✓ 模式 '$pattern': 已经是空的"
        fi
    done

    if [ ${#failed_patterns[@]} -eq 0 ]; then
        log_success "所有 Crawlrs 数据清理完成"
        return 0
    else
        log_error "以下模式清理失败: ${failed_patterns[*]}"
        return 1
    fi
}

# 完全清理（FLUSHALL）
cleanup_all() {
    local total_before=$(get_total_key_count)

    log_section "完全清理 Redis (FLUSHALL)"
    log_warning "将删除 Redis 中的所有数据 ($total_before keys)"

    read -p "确认执行? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_info "取消清理"
        return 1
    fi

    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" FLUSHALL &>/dev/null

    local total_after=$(get_total_key_count)
    if [ "$total_after" -eq 0 ]; then
        log_success "Redis 已完全清理 ($total_before → 0 keys)"
        return 0
    else
        log_error "清理后仍有 $total_after keys"
        return 1
    fi
}

# 清理特定模式
cleanup_specific_patterns() {
    local patterns=("$@")
    local failed_patterns=()

    log_section "清理指定模式: ${patterns[*]}"

    for pattern in "${patterns[@]}"; do
        local count=$(redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" --scan --pattern "$pattern" 2>/dev/null | wc -l)
        if ! cleanup_pattern "$pattern" "$count"; then
            failed_patterns+=("$pattern")
        fi
    done

    if [ ${#failed_patterns[@]} -eq 0 ]; then
        log_success "指定模式清理完成"
        return 0
    else
        log_error "以下模式清理失败: ${failed_patterns[*]}"
        return 1
    fi
}

# 重置统计信息
reset_stats() {
    log_info "重置 Redis 统计信息..."

    # 重置 INFO 中的统计
    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" DEBUG SLEEP 0 &>/dev/null || true

    # 重置连接统计
    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" CLIENT KILL TYPE normal &>/dev/null || true

    log_success "统计信息已重置"
}

# 验证清理结果
verify_cleanup() {
    local total_keys=$(get_total_key_count)
    local crawlrs_keys=0

    log_section "验证清理结果"

    for pattern in "${KEY_PATTERNS[@]}"; do
        local count=$(redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" --scan --pattern "$pattern" 2>/dev/null | wc -l)
        crawlrs_keys=$((crawlrs_keys + count))
        if [ "$count" -gt 0 ]; then
            echo -e "  ${RED}✗ $pattern: $count keys${NC}"
        else
            echo -e "  ${GREEN}✓ $pattern: 0 keys${NC}"
        fi
    done

    echo ""
    echo -e "总 keys: $total_keys"
    echo -e "Crawlrs keys: $crawlrs_keys"

    if [ "$crawlrs_keys" -eq 0 ]; then
        log_success "验证通过: Redis 中无 Crawlrs 测试数据残留"
        return 0
    else
        log_error "验证失败: 发现 $crawlrs_keys 个 Crawlrs keys 残留"
        return 1
    fi
}

# 显示 Redis 状态
show_redis_status() {
    log_section "Redis 状态"

    local total_keys=$(get_total_key_count)

    echo -e "${CYAN}模式                        | keys${NC}"
    echo -e "${CYAN}--------------------------------------${NC}"

    for pattern in "${KEY_PATTERNS[@]}"; do
        local count=$(redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" --scan --pattern "$pattern" 2>/dev/null | wc -l)
        printf "  %-30s | %d\n" "$pattern" "$count"
    done

    echo -e "${CYAN}--------------------------------------${NC}"
    echo -e "  ${YELLOW}总计: $total_keys keys${NC}"

    # 显示 Redis 服务器信息
    echo ""
    log_info "Redis 服务器信息:"
    redis-cli -h "$REDIS_HOST" -p "$REDIS_PORT" INFO server 2>/dev/null | grep -E "redis_version|os|used_memory_human" | head -5
}

# 显示帮助信息
show_help() {
    echo "Crawlrs Redis 清理脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  (无)          清理所有 Crawlrs 测试数据"
    echo "  --flush       完全清理 (FLUSHALL - 删除所有数据)"
    echo "  --verify       仅验证（不清理）"
    echo "  --status       显示 Redis 状态"
    echo "  --help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --partial PATTERNS  仅清理指定模式（逗号分隔）"
    echo "  --host HOST         Redis 主机 (默认: test-redis)"
    echo "  --port PORT         Redis 端口 (默认: 6379)"
    echo ""
    echo "示例:"
    echo "  $0                                    # 清理 Crawlrs 数据"
    echo "  $0 --flush                            # 完全清理（危险！）"
    echo "  $0 --partial 'rate:*,task:*'          # 仅清理指定模式"
    echo "  $0 --verify                           # 仅验证"
    echo "  $0 --status                           # 查看状态"
}

# 主函数
main() {
    local command=""
    local partial_patterns=""
    local verify_only=false
    local show_status=false
    local flush_all=false

    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            --partial)
                partial_patterns="$2"
                shift 2
                ;;
            --flush)
                flush_all=true
                shift
                ;;
            --verify)
                verify_only=true
                shift
                ;;
            --status)
                show_status=true
                shift
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            --host)
                REDIS_HOST="$2"
                shift 2
                ;;
            --port)
                REDIS_PORT="$2"
                shift 2
                ;;
            *)
                log_error "未知参数: $1"
                show_help
                exit 1
                ;;
        esac
    done

    echo ""
    log_section "Crawlrs Redis 清理工具"
    echo "主机: $REDIS_HOST:$REDIS_PORT"

    # 检查 Redis 连接
    if ! check_redis_connection; then
        log_error "无法连接到 Redis，请检查配置"
        exit 1
    fi

    if [ "$show_status" = true ]; then
        show_redis_status
        exit 0
    fi

    if [ "$verify_only" = true ]; then
        verify_cleanup
        exit $?
    fi

    if [ "$flush_all" = true ]; then
        cleanup_all
        exit $?
    fi

    if [ -n "$partial_patterns" ]; then
        IFS=',' read -ra patterns_array <<< "$partial_patterns"
        cleanup_specific_patterns "${patterns_array[@]}"
    else
        cleanup_all_crawlrs_keys
    fi

    # 重置统计信息
    reset_stats

    # 验证清理结果
    if verify_cleanup; then
        exit 0
    else
        exit 1
    fi
}

# 启动
main "$@"
