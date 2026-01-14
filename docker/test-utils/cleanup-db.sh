#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 数据库清理脚本
# =============================================================================
# 用于在测试前清理 PostgreSQL 数据库中的所有测试数据
#
# 使用方法:
#   ./cleanup-db.sh                    # 清理所有数据
#   ./cleanup-db.sh --partial table1,table2  # 仅清理指定表
#   ./cleanup-db.sh --verify           # 仅验证（不清理）
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
DB_HOST="${CRAWLRS__DATABASE__HOST:-localhost}"
DB_PORT="${CRAWLRS__DATABASE__PORT:-5443}"
DB_NAME="${CRAWLRS__DATABASE__NAME:-crawlrs_test}"
DB_USER="${CRAWLRS__DATABASE__USER:-crawlrs}"
DB_PASSWORD="${CRAWLRS__DATABASE__PASSWORD:-password}"

# 检测是否在 Docker Compose 网络中（使用服务名）
if [ -f /.dockerenv ] || grep -q "docker\|containerd" /proc/1/cgroup 2>/dev/null; then
    # 在 Docker 容器内，使用服务名
    DB_HOST="${CRAWLRS__DATABASE__HOST:-test-db}"
    DB_PORT="${CRAWLRS__DATABASE__PORT:-5432}"
fi

# 如果明确指定了环境变量，使用环境变量的值
[ -n "$CRAWLRS__DATABASE__HOST" ] && DB_HOST="$CRAWLRS__DATABASE__HOST"
[ -n "$CRAWLRS__DATABASE__PORT" ] && DB_PORT="$CRAWLRS__DATABASE__PORT"

# 所有需要清理的表（按依赖顺序）
TABLES=(
    "auth_feature_flag_overrides"
    "auth_audit_log"
    "webhook_event"
    "webhook"
    "scrape_result"
    "tasks_backlog"
    "task"
    "crawl"
    "geo_restriction_log"
    "credits_transactions"
    "credits"
    "auth_scopes"
    "auth_feature_flags"
    "api_keys"
    "team"
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

# 构建 psql 连接字符串
build_psql_cmd() {
    export PGPASSWORD="$DB_PASSWORD"
    echo "psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d $DB_NAME"
}

# 检查数据库连接
check_db_connection() {
    log_info "检查数据库连接..."

    export PGPASSWORD="$DB_PASSWORD"
    if psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -c "SELECT 1" &>/dev/null; then
        log_success "数据库连接成功"
        return 0
    else
        log_error "无法连接到数据库"
        return 1
    fi
}

# 检查表是否存在
table_exists() {
    local table_name=$1
    export PGPASSWORD="$DB_PASSWORD"

    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -t -c "
        SELECT EXISTS (
            SELECT FROM information_schema.tables
            WHERE table_schema = 'public'
            AND table_name = '$table_name'
        );
    " | tr -d '[:space:]' | grep -q "t"
}

# 获取表行数
get_table_count() {
    local table_name=$1
    export PGPASSWORD="$DB_PASSWORD"

    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -t -c "
        SELECT COUNT(*) FROM $table_name;
    " | tr -d '[:space:]'
}

# 清理单个表
cleanup_table() {
    local table_name=$1
    local count_before=$2

    log_info "清理表: $table_name"

    # 使用 TRUNCATE 并重启序列
    export PGPASSWORD="$DB_PASSWORD"
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -c "
        TRUNCATE TABLE $table_name CASCADE;
        ALTER SEQUENCE IF EXISTS ${table_name}_id_seq RESTART WITH 1;
    " 2>/dev/null

    local count_after=$(get_table_count "$table_name")
    if [ "$count_after" -eq 0 ]; then
        log_success "  ✓ $table_name 已清理 ($count_before 行 → 0 行)"
        return 0
    else
        log_warning "  ⚠ $table_name 清理后仍有 $count_after 行"
        return 1
    fi
}

# 清理所有表
cleanup_all_tables() {
    local failed_tables=()

    log_section "清理所有测试数据"

    for table in "${TABLES[@]}"; do
        if table_exists "$table"; then
            local count_before=$(get_table_count "$table")
            if [ "$count_before" -gt 0 ]; then
                if ! cleanup_table "$table" "$count_before"; then
                    failed_tables+=("$table")
                fi
            else
                log_info "  ✓ $table 已经是空的"
            fi
        else
            log_warning "  ⚠ 表 $table 不存在，跳过"
        fi
    done

    if [ ${#failed_tables[@]} -eq 0 ]; then
        log_success "所有表清理完成"
        return 0
    else
        log_error "以下表清理失败: ${failed_tables[*]}"
        return 1
    fi
}

# 清理指定表（部分清理）
cleanup_specific_tables() {
    local tables_to_clean=("$@")
    local failed_tables=()

    log_section "清理指定表: ${tables_to_clean[*]}"

    for table in "${tables_to_clean[@]}"; do
        if table_exists "$table"; then
            local count_before=$(get_table_count "$table")
            if [ "$count_before" -gt 0 ]; then
                if ! cleanup_table "$table" "$count_before"; then
                    failed_tables+=("$table")
                fi
            else
                log_info "  ✓ $table 已经是空的"
            fi
        else
            log_warning "  ⚠ 表 $table 不存在，跳过"
        fi
    done

    if [ ${#failed_tables[@]} -eq 0 ]; then
        log_success "指定表清理完成"
        return 0
    else
        log_error "以下表清理失败: ${failed_tables[*]}"
        return 1
    fi
}

# 验证清理结果
verify_cleanup() {
    local total_rows=0
    local tables_with_data=()

    log_section "验证清理结果"

    for table in "${TABLES[@]}"; do
        if table_exists "$table"; then
            local count=$(get_table_count "$table")
            total_rows=$((total_rows + count))
            if [ "$count" -gt 0 ]; then
                tables_with_data+=("$table:$count")
                echo -e "  ${RED}✗ $table: $count 行${NC}"
            else
                echo -e "  ${GREEN}✓ $table: 0 行${NC}"
            fi
        fi
    done

    echo ""
    if [ "$total_rows" -eq 0 ]; then
        log_success "验证通过: 数据库中无测试数据残留"
        return 0
    else
        log_error "验证失败: 发现 $total_rows 行残留数据"
        log_warning "含数据的表: ${tables_with_data[*]}"
        return 1
    fi
}

# 显示数据库状态
show_db_status() {
    log_section "数据库状态"

    local total_rows=0

    echo -e "${CYAN}表名                        | 行数${NC}"
    echo -e "${CYAN}--------------------------------------${NC}"

    for table in "${TABLES[@]}"; do
        if table_exists "$table"; then
            local count=$(get_table_count "$table")
            total_rows=$((total_rows + count))
            printf "  %-30s | %d\n" "$table" "$count"
        fi
    done

    echo -e "${CYAN}--------------------------------------${NC}"
    echo -e "  ${YELLOW}总计: $total_rows 行${NC}"
}

# 显示帮助信息
show_help() {
    echo "Crawlrs 数据库清理脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  (无)          清理所有测试数据"
    echo "  --verify       仅验证（不清理）"
    echo "  --status       显示数据库状态"
    echo "  --help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --partial TABLES   仅清理指定表（逗号分隔）"
    echo "  --host HOST        数据库主机 (默认: test-db)"
    echo "  --port PORT        数据库端口 (默认: 5432)"
    echo "  --name NAME        数据库名 (默认: crawlrs_test)"
    echo "  --user USER        用户名 (默认: crawlrs)"
    echo "  --password PASS    密码 (默认: password)"
    echo ""
    echo "示例:"
    echo "  $0                                    # 清理所有"
    echo "  $0 --partial task,scrape_result       # 仅清理指定表"
    echo "  $0 --verify                           # 仅验证"
    echo "  $0 --status                           # 查看状态"
}

# 主函数
main() {
    local command=""
    local partial_tables=""
    local verify_only=false
    local show_status=false

    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            --partial)
                partial_tables="$2"
                shift 2
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
                DB_HOST="$2"
                shift 2
                ;;
            --port)
                DB_PORT="$2"
                shift 2
                ;;
            --name)
                DB_NAME="$2"
                shift 2
                ;;
            --user)
                DB_USER="$2"
                shift 2
                ;;
            --password)
                DB_PASSWORD="$2"
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
    log_section "Crawlrs 数据库清理工具"
    echo "主机: $DB_HOST:$DB_PORT"
    echo "数据库: $DB_NAME"
    echo "用户: $DB_USER"

    # 检查数据库连接
    if ! check_db_connection; then
        log_error "无法连接到数据库，请检查配置"
        exit 1
    fi

    if [ "$show_status" = true ]; then
        show_db_status
        exit 0
    fi

    if [ "$verify_only" = true ]; then
        verify_cleanup
        exit $?
    fi

    if [ -n "$partial_tables" ]; then
        IFS=',' read -ra tables_array <<< "$partial_tables"
        cleanup_specific_tables "${tables_array[@]}"
    else
        cleanup_all_tables
    fi

    # 验证清理结果
    if verify_cleanup; then
        exit 0
    else
        exit 1
    fi
}

# 启动
main "$@"
