#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs 文件系统清理脚本
# =============================================================================
# 用于在测试前清理文件系统中的所有临时文件
#
# 使用方法:
#   ./cleanup-files.sh                    # 清理所有测试文件
#   ./cleanup-files.sh --partial temp     # 仅清理临时文件
#   ./cleanup-files.sh --verify           # 仅验证（不清理）
# =============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# 默认配置
PROJECT_ROOT="${PROJECT_ROOT:-$(dirname "$0")/../../..}"
TEMP_DIR="${TEMP_DIR:-$PROJECT_ROOT/temp}"
LOGS_DIR="${LOGS_DIR:-$PROJECT_ROOT/logs}"
TEST_DATA_DIR="${TEST_DATA_DIR:-$PROJECT_ROOT/test-data}"
UPLOADS_DIR="${UPLOADS_DIR:-$PROJECT_ROOT/uploads}"
SCREENSHOTS_DIR="${SCREENSHOTS_DIR:-$PROJECT_ROOT/screenshots}"

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

# 获取目录大小
get_dir_size() {
    local dir=$1
    if [ -d "$dir" ]; then
        du -sh "$dir" 2>/dev/null | cut -f1 || echo "0"
    else
        echo "0"
    fi
}

# 检查目录是否存在
dir_exists() {
    [ -d "$1" ]
}

# 清理目录
cleanup_dir() {
    local dir=$1
    local size_before=$2

    if ! dir_exists "$dir"; then
        log_info "  ✓ $dir 不存在，跳过"
        return 0
    fi

    log_info "清理目录: $dir ($size_before)"

    # 删除目录中的所有内容，但保留目录本身
    find "$dir" -mindepth 1 -delete 2>/dev/null || {
        log_warning "  ⚠ 无法清理 $dir"
        return 1
    }

    local size_after=$(get_dir_size "$dir")
    log_success "  ✓ $dir 已清理 ($size_before → $size_after)"
    return 0
}

# 清理所有测试文件
cleanup_all() {
    local total_size_before=0
    local failed_dirs=()

    log_section "清理所有测试文件"

    # 临时文件
    if dir_exists "$TEMP_DIR"; then
        local size=$(get_dir_size "$TEMP_DIR")
        total_size_before=$((total_size_before + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))
        if ! cleanup_dir "$TEMP_DIR" "$size"; then
            failed_dirs+=("temp")
        fi
    fi

    # 日志文件
    if dir_exists "$LOGS_DIR"; then
        local size=$(get_dir_size "$LOGS_DIR")
        total_size_before=$((total_size_before + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))
        if ! cleanup_dir "$LOGS_DIR" "$size"; then
            failed_dirs+=("logs")
        fi
    fi

    # 测试数据
    if dir_exists "$TEST_DATA_DIR"; then
        local size=$(get_dir_size "$TEST_DATA_DIR")
        total_size_before=$((total_size_before + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))
        if ! cleanup_dir "$TEST_DATA_DIR" "$size"; then
            failed_dirs+=("test-data")
        fi
    fi

    # 上传文件
    if dir_exists "$UPLOADS_DIR"; then
        local size=$(get_dir_size "$UPLOADS_DIR")
        total_size_before=$((total_size_before + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))
        if ! cleanup_dir "$UPLOADS_DIR" "$size"; then
            failed_dirs+=("uploads")
        fi
    fi

    # 截图文件
    if dir_exists "$SCREENSHOTS_DIR"; then
        local size=$(get_dir_size "$SCREENSHOTS_DIR")
        total_size_before=$((total_size_before + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))
        if ! cleanup_dir "$SCREENSHOTS_DIR" "$size"; then
            failed_dirs+=("screenshots")
        fi
    fi

    # 清理 Docker test-results
    if dir_exists "$PROJECT_ROOT/docker/test-results"; then
        local size=$(get_dir_size "$PROJECT_ROOT/docker/test-results")
        if ! cleanup_dir "$PROJECT_ROOT/docker/test-results" "$size"; then
            failed_dirs+=("test-results")
        fi
    fi

    if [ ${#failed_dirs[@]} -eq 0 ]; then
        log_success "所有目录清理完成"
        return 0
    else
        log_error "以下目录清理失败: ${failed_dirs[*]}"
        return 1
    fi
}

# 清理特定目录
cleanup_specific_dirs() {
    local dirs=("$@")
    local failed_dirs=()

    log_section "清理指定目录: ${dirs[*]}"

    for dir in "${dirs[@]}"; do
        local full_path="$dir"
        if ! [[ "$dir" == /* ]]; then
            full_path="$PROJECT_ROOT/$dir"
        fi

        if dir_exists "$full_path"; then
            local size=$(get_dir_size "$full_path")
            if ! cleanup_dir "$full_path" "$size"; then
                failed_dirs+=("$dir")
            fi
        else
            log_info "  ✓ $dir 不存在，跳过"
        fi
    done

    if [ ${#failed_dirs[@]} -eq 0 ]; then
        log_success "指定目录清理完成"
        return 0
    else
        log_error "以下目录清理失败: ${failed_dirs[*]}"
        return 1
    fi
}

# 清理旧文件（按修改时间）
cleanup_old_files() {
    local days=${1:-7}
    local cleaned=0

    log_section "清理 ${days} 天前的文件"

    for dir in "$TEMP_DIR" "$LOGS_DIR" "$TEST_DATA_DIR" "$UPLOADS_DIR" "$SCREENSHOTS_DIR"; do
        if dir_exists "$dir"; then
            local count=$(find "$dir" -type f -mtime +${days} 2>/dev/null | wc -l)
            if [ "$count" -gt 0 ]; then
                find "$dir" -type f -mtime +${days} -delete 2>/dev/null
                log_info "  ✓ $dir: 清理了 $count 个旧文件"
                cleaned=$((cleaned + count))
            else
                log_info "  ✓ $dir: 无旧文件需要清理"
            fi
        fi
    done

    log_success "共清理 $cleaned 个旧文件"
}

# 验证清理结果
verify_cleanup() {
    local total_files=0
    local total_size=0
    local dirs_with_data=()

    log_section "验证清理结果"

    echo -e "${CYAN}目录                        | 文件数 | 大小${NC}"
    echo -e "${CYAN}--------------------------------------${NC}"

    for dir in "$TEMP_DIR" "$LOGS_DIR" "$TEST_DATA_DIR" "$UPLOADS_DIR" "$SCREENSHOTS_DIR" "$PROJECT_ROOT/docker/test-results"; do
        if dir_exists "$dir"; then
            local file_count=$(find "$dir" -type f 2>/dev/null | wc -l)
            local size=$(get_dir_size "$dir")
            total_files=$((total_files + file_count))
            total_size=$((total_size + $(numfmt --from=iec "$size" 2>/dev/null || echo 0)))

            if [ "$file_count" -gt 0 ]; then
                echo -e "  ${RED}✗ $(basename $dir): $file_count 文件 | $size${NC}"
                dirs_with_data+=("$(basename $dir)")
            else
                echo -e "  ${GREEN}✓ $(basename $dir): 0 文件 | $size${NC}"
            fi
        else
            echo -e "  ${GREEN}✓ $(basename $dir): 不存在${NC}"
        fi
    done

    echo ""
    echo -e "总文件数: $total_files"

    if [ "$total_files" -eq 0 ]; then
        log_success "验证通过: 文件系统中无测试文件残留"
        return 0
    else
        log_error "验证失败: 发现 $total_files 个文件残留"
        log_warning "含文件的目录: ${dirs_with_data[*]}"
        return 1
    fi
}

# 显示文件状态
show_file_status() {
    log_section "文件系统状态"

    echo -e "${CYAN}目录                        | 文件数 | 大小${NC}"
    echo -e "${CYAN}--------------------------------------${NC}"

    for dir in "$TEMP_DIR" "$LOGS_DIR" "$TEST_DATA_DIR" "$UPLOADS_DIR" "$SCREENSHOTS_DIR"; do
        if dir_exists "$dir"; then
            local file_count=$(find "$dir" -type f 2>/dev/null | wc -l)
            local size=$(get_dir_size "$dir")
            printf "  %-30s | %7d | %s\n" "$(basename $dir)" "$file_count" "$size"
        else
            printf "  %-30s | %7s | %s\n" "$(basename $dir)" "N/A" "不存在"
        fi
    done
}

# 显示帮助信息
show_help() {
    echo "Crawlrs 文件系统清理脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  (无)           清理所有测试文件"
    echo "  --old DAYS     清理 N 天前的文件"
    echo "  --verify       仅验证（不清理）"
    echo "  --status       显示文件状态"
    echo "  --help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --partial DIRS   仅清理指定目录（逗号分隔）"
    echo "  --root DIR       项目根目录 (默认: 自动检测)"
    echo ""
    echo "示例:"
    echo "  $0                           # 清理所有"
    echo "  $0 --old 3                   # 清理3天前的文件"
    echo "  $0 --partial temp,logs       # 仅清理指定目录"
    echo "  $0 --verify                  # 仅验证"
    echo "  $0 --status                  # 查看状态"
}

# 主函数
main() {
    local command=""
    local partial_dirs=""
    local verify_only=false
    local show_status=false
    local old_files_days=0

    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            --partial)
                partial_dirs="$2"
                shift 2
                ;;
            --old)
                old_files_days="$2"
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
            --root)
                PROJECT_ROOT="$2"
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
    log_section "Crawlrs 文件系统清理工具"
    echo "项目根目录: $PROJECT_ROOT"

    if [ "$show_status" = true ]; then
        show_file_status
        exit 0
    fi

    if [ "$verify_only" = true ]; then
        verify_cleanup
        exit $?
    fi

    if [ "$old_files_days" -gt 0 ]; then
        cleanup_old_files "$old_files_days"
        exit $?
    fi

    if [ -n "$partial_dirs" ]; then
        IFS=',' read -ra dirs_array <<< "$partial_dirs"
        cleanup_specific_dirs "${dirs_array[@]}"
    else
        cleanup_all
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
