#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs Docker 环境管理工具
# =============================================================================
# 统一管理测试环境的数据库清理、文件清理、容器重建。
#
# 使用方法:
#   ./docker/manage.sh <command> [options]
#
# 命令:
#   cleanup          清理所有（数据库 + 文件）
#   cleanup-db       仅清理数据库
#   cleanup-files    仅清理文件系统
#   reset            重建所有测试容器
#   status           显示环境状态
#   verify           仅验证（不清理）
#
# 运行 ./docker/manage.sh help 查看完整帮助
# =============================================================================

set -euo pipefail

# ─── 颜色与日志 ───────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info()    { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_warn()    { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1"; }
log_section() { echo -e "\n${CYAN}══ $1 ══${NC}\n"; }

# ─── 全局配置 ─────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
COMPOSE_FILE="${COMPOSE_FILE:-$SCRIPT_DIR/docker-compose.test.yml}"
PROJECT_NAME="crawlrs-docker"
TIMEOUT_SECONDS=60

# 数据库配置
DB_HOST="${CRAWLRS__DATABASE__HOST:-}"
DB_PORT="${CRAWLRS__DATABASE__PORT:-}"
DB_NAME="${CRAWLRS__DATABASE__NAME:-crawlrs_test}"
DB_USER="${CRAWLRS__DATABASE__USER:-crawlrs}"
DB_PASSWORD="${CRAWLRS__DATABASE__PASSWORD:-password}"

# 自动检测运行环境（Docker 容器内 vs 宿主机）
if [ -f /.dockerenv ] || grep -q "docker\|containerd" /proc/1/cgroup 2>/dev/null; then
    DB_HOST="${DB_HOST:-test-db}"
    DB_PORT="${DB_PORT:-5432}"
else
    DB_HOST="${DB_HOST:-localhost}"
    DB_PORT="${DB_PORT:-5443}"
fi

# 文件系统清理目标
DIRS_TO_CLEAN=(
    "$PROJECT_ROOT/temp"
    "$PROJECT_ROOT/logs"
    "$PROJECT_ROOT/test-data"
    "$PROJECT_ROOT/uploads"
    "$PROJECT_ROOT/screenshots"
    "$SCRIPT_DIR/test-results"
)

# 需要清理的数据库表（按依赖顺序）
TABLES=(
    "auth_audit_log" "webhook_event" "webhook" "scrape_result"
    "tasks_backlog" "task" "crawl" "geo_restriction_log"
    "credits_transactions" "credits" "auth_scopes" "api_keys" "team"
)

# 测试服务列表
TEST_SERVICES=("test-db" "chrome" "flaresolverr" "test-runner")

# ─── 数据库操作 ───────────────────────────────────────────────────────────────

psql_cmd() {
    export PGPASSWORD="$DB_PASSWORD"
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" "$@"
}

db_check() {
    psql_cmd -c "SELECT 1" &>/dev/null
}

table_exists() {
    psql_cmd -t -c "SELECT EXISTS (
        SELECT FROM information_schema.tables
        WHERE table_schema = 'public' AND table_name = '$1'
    )" 2>/dev/null | tr -d '[:space:]' | grep -q "t"
}

table_count() {
    psql_cmd -t -c "SELECT COUNT(*) FROM $1" 2>/dev/null | tr -d '[:space:]'
}

cleanup_table() {
    local table="$1"
    psql_cmd -c "TRUNCATE TABLE $table CASCADE;
        ALTER SEQUENCE IF EXISTS ${table}_id_seq RESTART WITH 1;" &>/dev/null
}

do_cleanup_db() {
    log_section "清理数据库 ($DB_HOST:$DB_PORT/$DB_NAME)"

    if ! db_check; then
        log_error "无法连接数据库 $DB_HOST:$DB_PORT/$DB_NAME"
        return 1
    fi

    local failed=0
    for table in "${TABLES[@]}"; do
        if table_exists "$table"; then
            local count
            count=$(table_count "$table")
            if [ "$count" -gt 0 ]; then
                cleanup_table "$table"
                local after
                after=$(table_count "$table")
                if [ "$after" -eq 0 ]; then
                    log_success "  $table ($count 行 → 0)"
                else
                    log_warn "  $table 仍有 $after 行"
                    ((failed++))
                fi
            else
                log_info "  $table 已空"
            fi
        else
            log_info "  $table 不存在，跳过"
        fi
    done

    [ "$failed" -eq 0 ]
}

# ─── 文件清理操作 ─────────────────────────────────────────────────────────────

dir_size() {
    [ -d "$1" ] && du -sh "$1" 2>/dev/null | cut -f1 || echo "0"
}

cleanup_dir() {
    local dir="$1"
    [ -d "$dir" ] || return 0
    local size_before
    size_before=$(dir_size "$dir")
    find "$dir" -mindepth 1 -delete 2>/dev/null || {
        log_warn "  无法清理 $dir"
        return 1
    }
    log_success "  $(basename "$dir") ($size_before → $(dir_size "$dir"))"
}

do_cleanup_files() {
    log_section "清理文件系统"

    local failed=0
    for dir in "${DIRS_TO_CLEAN[@]}"; do
        if ! cleanup_dir "$dir"; then
            ((failed++))
        fi
    done

    [ "$failed" -eq 0 ]
}

# ─── 容器操作 ─────────────────────────────────────────────────────────────────

container_status() {
    docker inspect -f '{{.State.Status}}' "$1" 2>/dev/null || echo "not_found"
}

container_exists() {
    docker inspect -f '{{.Id}}' "$1" &>/dev/null
}

container_healthy() {
    local h
    h=$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$1" 2>/dev/null || echo "none")
    [ "$h" = "healthy" ] || [ "$h" = "none" ]
}

do_reset() {
    log_section "重建测试容器"

    if ! docker info &>/dev/null; then
        log_error "Docker 未运行"
        return 1
    fi

    log_info "停止并清理旧容器..."
    docker compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down -v --remove-orphans &>/dev/null || true

    # 清理关联数据卷
    for vol in $(docker volume ls -q --filter "name=${PROJECT_NAME}" 2>/dev/null); do
        docker volume rm "$vol" &>/dev/null || true
    done

    # 清理网络
    docker network rm "${PROJECT_NAME}-crawlrs-test-network" &>/dev/null || true

    log_info "启动服务..."
    docker compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" up -d

    # 等待健康检查
    log_info "等待服务就绪..."
    local all_ok=true
    for svc in "${TEST_SERVICES[@]}"; do
        [ "$svc" = "test-runner" ] && continue
        local elapsed=0
        while [ $elapsed -lt 30 ]; do
            if container_exists "${PROJECT_NAME}-${svc}" && container_healthy "${PROJECT_NAME}-${svc}"; then
                log_success "  $svc 就绪"
                break
            fi
            sleep 1
            ((elapsed++))
        done
        if [ $elapsed -ge 30 ]; then
            log_warn "  $svc 等待超时"
            all_ok=false
        fi
    done

    $all_ok
}

# ─── 状态与验证 ───────────────────────────────────────────────────────────────

do_status() {
    log_section "环境状态"

    # 数据库
    echo -e "${CYAN}数据库:${NC}"
    if db_check 2>/dev/null; then
        local total=0
        for table in "${TABLES[@]}"; do
            if table_exists "$table" 2>/dev/null; then
                local c
                c=$(table_count "$table" 2>/dev/null || echo "0")
                total=$((total + c))
                [ "$c" -gt 0 ] && printf "  %-25s %s 行\n" "$table" "$c"
            fi
        done
        echo "  总计: $total 行"
    else
        log_warn "  无法连接或未启动"
    fi

    # 文件系统
    echo ""
    echo -e "${CYAN}文件系统:${NC}"
    for dir in "${DIRS_TO_CLEAN[@]}"; do
        if [ -d "$dir" ]; then
            local fc
            fc=$(find "$dir" -type f 2>/dev/null | wc -l)
            printf "  %-25s %d 文件 | %s\n" "$(basename "$dir")" "$fc" "$(dir_size "$dir")"
        fi
    done

    # 容器
    echo ""
    echo -e "${CYAN}容器:${NC}"
    for svc in "${TEST_SERVICES[@]}"; do
        if container_exists "${PROJECT_NAME}-${svc}"; then
            local st
            st=$(container_status "${PROJECT_NAME}-${svc}")
            local healthy
            healthy=$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}N/A{{end}}' "${PROJECT_NAME}-${svc}" 2>/dev/null || echo "N/A")
            if [ "$st" = "running" ]; then
                echo -e "  ${GREEN}✓${NC} $svc | $st | $healthy"
            else
                echo -e "  ${RED}✗${NC} $svc | $st | $healthy"
            fi
        else
            echo -e "  ${CYAN}○${NC} $svc | 未创建"
        fi
    done
}

do_verify() {
    log_section "验证清理结果"
    local ok=true

    # 数据库验证
    if db_check 2>/dev/null; then
        for table in "${TABLES[@]}"; do
            if table_exists "$table" 2>/dev/null; then
                local c
                c=$(table_count "$table" 2>/dev/null || echo "0")
                if [ "$c" -gt 0 ]; then
                    log_error "  $table: $c 行残留"
                    ok=false
                fi
            fi
        done
        $ok && log_success "  数据库: 干净"
    else
        log_info "  数据库: 未连接"
    fi

    # 文件验证
    local files_ok=true
    for dir in "${DIRS_TO_CLEAN[@]}"; do
        if [ -d "$dir" ]; then
            local fc
            fc=$(find "$dir" -type f 2>/dev/null | wc -l)
            if [ "$fc" -gt 0 ]; then
                log_error "  $(basename "$dir"): $fc 文件残留"
                files_ok=false
            fi
        fi
    done
    $files_ok && log_success "  文件系统: 干净"

    $ok && $files_ok
}

# ─── 完整清理 ─────────────────────────────────────────────────────────────────

do_cleanup() {
    local start_time
    start_time=$(date +%s)

    # 并行清理数据库和文件
    local db_ok=true files_ok=true

    do_cleanup_db > /tmp/crawlrs-cleanup-db.log 2>&1 &
    local db_pid=$!

    do_cleanup_files > /tmp/crawlrs-cleanup-files.log 2>&1 &
    local files_pid=$!

    wait "$db_pid" || db_ok=false
    wait "$files_pid" || files_ok=false

    local duration=$(( $(date +%s) - start_time ))

    if $db_ok && $files_ok; then
        log_success "清理完成 (${duration}秒)"
    else
        ! $db_ok && log_error "数据库清理失败，日志: /tmp/crawlrs-cleanup-db.log" && cat /tmp/crawlrs-cleanup-db.log
        ! $files_ok && log_error "文件清理失败，日志: /tmp/crawlrs-cleanup-files.log" && cat /tmp/crawlrs-cleanup-files.log
        return 1
    fi

    do_verify
}

# ─── 帮助 ─────────────────────────────────────────────────────────────────────

show_help() {
    cat <<'EOF'
Crawlrs Docker 环境管理工具

用法: ./docker/manage.sh <command> [options]

命令:
  cleanup          清理所有（数据库 + 文件），并行执行并验证
  cleanup-db       仅清理数据库（TRUNCATE 所有测试表 + 重置序列）
  cleanup-files    仅清理文件系统（temp/logs/test-data/uploads/screenshots）
  reset            重建所有测试容器（down -v → up）
  status           显示环境状态（数据库行数、文件数、容器状态）
  verify           验证清理结果（不执行清理）
  help             显示此帮助

选项:
  --host HOST      数据库主机 (默认: 自动检测)
  --port PORT      数据库端口 (默认: 自动检测)
  --name NAME      数据库名 (默认: crawlrs_test)
  --user USER      数据库用户 (默认: crawlrs)
  --password PASS  数据库密码
  --timeout SEC    容器启动超时 (默认: 60秒)

示例:
  ./docker/manage.sh cleanup                  # 完整清理
  ./docker/manage.sh cleanup-db               # 仅清数据库
  ./docker/manage.sh cleanup-files            # 仅清文件
  ./docker/manage.sh reset                    # 重建容器
  ./docker/manage.sh status                   # 查看状态
  ./docker/manage.sh cleanup --host mydb      # 指定数据库主机
EOF
}

# ─── 入口 ─────────────────────────────────────────────────────────────────────

main() {
    [ $# -eq 0 ] && { show_help; exit 0; }

    local cmd="$1"; shift

    # 解析全局选项
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --host)     DB_HOST="$2";     shift 2 ;;
            --port)     DB_PORT="$2";     shift 2 ;;
            --name)     DB_NAME="$2";     shift 2 ;;
            --user)     DB_USER="$2";     shift 2 ;;
            --password) DB_PASSWORD="$2"; shift 2 ;;
            --timeout)  TIMEOUT_SECONDS="$2"; shift 2 ;;
            *)          shift ;;
        esac
    done

    case "$cmd" in
        cleanup)       do_cleanup ;;
        cleanup-db)    do_cleanup_db ;;
        cleanup-files) do_cleanup_files ;;
        reset)         do_reset ;;
        status)        do_status ;;
        verify)        do_verify ;;
        help|-h|--help) show_help ;;
        *)
            log_error "未知命令: $cmd"
            show_help
            exit 1
            ;;
    esac
}

main "$@"
