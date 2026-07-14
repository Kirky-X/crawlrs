#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs Docker 容器重建脚本
# =============================================================================
# 用于重建 Docker 容器、清理数据卷、重置网络
#
# 使用方法:
#   ./reset-containers.sh              # 重建所有测试容器
#   ./reset-containers.sh --partial db # 仅重建指定服务
#   ./reset-containers.sh --verify     # 仅验证状态
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
COMPOSE_FILE="${COMPOSE_FILE:-docker/docker-compose.test.yml}"
PROJECT_NAME="crawlrs-docker"
TIMEOUT_SECONDS=60

# 所有测试服务
ALL_SERVICES=("test-db" "chrome" "flaresolverr" "test-runner" "minio")

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

# 检查 Docker 是否运行
check_docker() {
    log_info "检查 Docker 环境..."

    if ! docker info &> /dev/null; then
        log_error "Docker 未运行，请先启动 Docker"
        return 1
    fi

    log_success "Docker 运行正常"
    return 0
}

# 获取容器状态
get_container_status() {
    local service=$1
    local status=$(docker inspect -f '{{.State.Status}}' "${PROJECT_NAME}-${service}" 2>/dev/null || echo "not_found")
    echo "$status"
}

# 检查容器是否存在
container_exists() {
    local service=$1
    docker inspect -f '{{.Id}}' "${PROJECT_NAME}-${service}" &>/dev/null
}

# 获取服务列表
get_running_services() {
    local running=()
    for service in "${ALL_SERVICES[@]}"; do
        if container_exists "$service"; then
            local status=$(get_container_status "$service")
            if [ "$status" == "running" ]; then
                running+=("$service")
            fi
        fi
    done
    echo "${running[@]}"
}

# 停止并删除容器
stop_and_remove_container() {
    local service=$1
    local timeout=${2:-10}

    if ! container_exists "$service"; then
        log_info "  ✓ $service 不存在，跳过"
        return 0
    fi

    local status=$(get_container_status "$service")

    if [ "$status" == "running" ]; then
        log_info "  停止容器: $service"
        docker stop -t "$timeout" "${PROJECT_NAME}-${service}" &>/dev/null || true
    fi

    log_info "  删除容器: $service"
    docker rm -v "${PROJECT_NAME}-${service}" &>/dev/null || true

    log_success "  ✓ $service 已删除"
}

# 清理数据卷
cleanup_volume() {
    local volume_name=$1

    log_info "  清理数据卷: $volume_name"

    if docker volume inspect "$volume_name" &>/dev/null; then
        docker volume rm "$volume_name" &>/dev/null || true
        log_success "    ✓ volume $volume_name 已删除"
    else
        log_info "    ✓ volume $volume_name 不存在，跳过"
    fi
}

# 清理网络
cleanup_network() {
    local network_name="${PROJECT_NAME}-test-network"

    log_info "  清理网络: $network_name"

    if docker network inspect "$network_name" &>/dev/null; then
        # 断开所有容器连接
        docker network disconnect -f "$network_name" "${PROJECT_NAME}-test-db" 2>/dev/null || true
        docker network disconnect -f "$network_name" "${PROJECT_NAME}-chrome" 2>/dev/null || true

        # 删除网络
        docker network rm "$network_name" &>/dev/null || true
        log_success "    ✓ 网络已删除"
    else
        log_info "    ✓ 网络不存在，跳过"
    fi
}

# 重置单个服务
reset_service() {
    local service=$1

    log_section "重置服务: $service"

    # 停止并删除容器
    stop_and_remove_container "$service"

    # 清理关联的数据卷
    case "$service" in
        test-db)
            cleanup_volume "${PROJECT_NAME}_test_db_data"
            ;;
        chrome)
            cleanup_volume "${PROJECT_NAME}_chrome_data"
            ;;
        flaresolverr)
            cleanup_volume "${PROJECT_NAME}_flaresolverr_data"
            ;;
        minio)
            cleanup_volume "${PROJECT_NAME}_minio_data"
            ;;
    esac
}

# 重置所有服务
reset_all_services() {
    local running_services=($(get_running_services))

    log_section "重置所有测试服务"

    if [ ${#running_services[@]} -eq 0 ]; then
        log_info "无运行中的测试服务"
    else
        log_info "停止运行中的服务: ${running_services[*]}"
        for service in "${running_services[@]}"; do
            docker stop -t 10 "${PROJECT_NAME}-${service}" &>/dev/null || true
        done
    fi

    # 停止所有测试容器
    log_info "停止所有测试容器..."
    docker-compose -f "$COMPOSE_FILE" --project-name "$PROJECT_NAME" down -v --remove-orphans &>/dev/null || true

    # 清理数据卷
    log_info "清理数据卷..."
    for volume in $(docker volume ls -q --filter "name=${PROJECT_NAME}" 2>/dev/null); do
        cleanup_volume "$volume"
    done

    # 清理网络
    cleanup_network

    # 清理临时镜像
    log_info "清理临时镜像..."
    docker image prune -f &>/dev/null || true

    log_success "所有服务已重置"
}

# 启动服务
start_service() {
    local service=$1
    local timeout=${2:-30}

    log_info "启动服务: $service"

    docker-compose -f "$COMPOSE_FILE" --project-name "$PROJECT_NAME" up -d "$service"

    # 等待服务健康
    local elapsed=0
    while [ $elapsed -lt $timeout ]; do
        local status=$(get_container_status "$service")
        if [ "$status" == "running" ]; then
            log_success "  ✓ $service 已启动"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done

    log_error "  ✗ $service 启动超时"
    return 1
}

# 启动所有服务
start_all_services() {
    log_section "启动所有测试服务"

    docker-compose -f "$COMPOSE_FILE" --project-name "$PROJECT_NAME" up -d

    # 等待所有服务健康
    log_info "等待服务健康检查..."

    local all_healthy=true
    for service in "${ALL_SERVICES[@]}"; do
        # 跳过 test-runner（不需要健康检查）
        if [ "$service" == "test-runner" ]; then
            continue
        fi

        local elapsed=0
        local timeout=30
        while [ $elapsed -lt $timeout ]; do
            if container_exists "$service"; then
                local status=$(get_container_status "$service")
                if [ "$status" == "running" ]; then
                    log_success "  ✓ $service 健康"
                    break
                fi
            else
                # 容器可能还没有创建
                sleep 1
                elapsed=$((elapsed + 1))
                continue
            fi

            # 检查健康检查状态
            local healthy=$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "${PROJECT_NAME}-${service}" 2>/dev/null || echo "none")
            if [ "$healthy" == "healthy" ]; then
                log_success "  ✓ $service 健康"
                break
            fi

            sleep 1
            elapsed=$((elapsed + 1))
        done

        if [ $elapsed -ge $timeout ]; then
            log_warning "  ⚠ $service 健康检查超时"
            all_healthy=false
        fi
    done

    if [ "$all_healthy" = true ]; then
        log_success "所有服务已启动并通过健康检查"
        return 0
    else
        log_warning "部分服务健康检查未通过"
        return 1
    fi
}

# 验证容器状态
verify_containers() {
    log_section "验证容器状态"

    local all_healthy=true

    echo -e "${CYAN}服务                        | 状态           | 健康${NC}"
    echo -e "${CYAN}--------------------------------------${NC}"

    for service in "${ALL_SERVICES[@]}"; do
        if container_exists "$service"; then
            local status=$(get_container_status "$service")
            local healthy=$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}N/A{{end}}' "${PROJECT_NAME}-${service}" 2>/dev/null || echo "N/A")

            if [ "$status" == "running" ] && [ "$healthy" == "healthy" ]; then
                echo -e "  ${GREEN}✓${NC} $service | $status | $healthy"
            elif [ "$status" == "running" ]; then
                echo -e "  ${YELLOW}?${NC} $service | $status | $healthy"
            else
                echo -e "  ${RED}✗${NC} $service | $status | $healthy"
                all_healthy=false
            fi
        else
            echo -e "  ${CYAN}○${NC} $service | 不存在"
        fi
    done

    if [ "$all_healthy" = true ]; then
        log_success "所有容器状态正常"
        return 0
    else
        log_error "部分容器状态异常"
        return 1
    fi
}

# 显示帮助信息
show_help() {
    echo "Crawlrs Docker 容器重建脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  (无)           重置并启动所有测试服务"
    echo "  --reset        仅重置（不启动）"
    echo "  --start        仅启动（不重置）"
    echo "  --verify       仅验证状态"
    echo "  --help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --partial SERVICES  仅重置指定服务（逗号分隔）"
    echo "  --file FILE         Docker Compose 文件 (默认: docker-compose.test.yml)"
    echo "  --timeout SEC       超时时间 (默认: 60秒)"
    echo ""
    echo "示例:"
    echo "  $0                           # 重置并启动所有"
    echo "  $0 --reset                   # 仅重置"
    echo "  $0 --start                   # 仅启动"
    echo "  $0 --verify                  # 仅验证"
    echo "  $0 --partial test-db,chrome  # 仅重置指定服务"
}

# 主函数
main() {
    local reset_only=false
    local start_only=false
    local verify_only=false
    local partial_services=""

    # 解析参数
    while [[ $# -gt 0 ]]; do
        case $1 in
            --reset)
                reset_only=true
                shift
                ;;
            --start)
                start_only=true
                shift
                ;;
            --verify)
                verify_only=true
                shift
                ;;
            --partial)
                partial_services="$2"
                shift 2
                ;;
            --file)
                COMPOSE_FILE="$2"
                shift 2
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
    log_section "Crawlrs Docker 容器管理"
    echo "Compose 文件: $COMPOSE_FILE"
    echo "项目名称: $PROJECT_NAME"

    # 检查 Docker
    if ! check_docker; then
        exit 1
    fi

    if [ "$verify_only" = true ]; then
        verify_containers
        exit $?
    fi

    if [ "$reset_only" = true ]; then
        if [ -n "$partial_services" ]; then
            IFS=',' read -ra services_array <<< "$partial_services"
            for service in "${services_array[@]}"; do
                reset_service "$service"
            done
        else
            reset_all_services
        fi
        exit $?
    fi

    if [ "$start_only" = true ]; then
        start_all_services
        exit $?
    fi

    # 重置并启动
    local start_time=$(date +%s)

    # 重置
    if [ -n "$partial_services" ]; then
        IFS=',' read -ra services_array <<< "$partial_services"
        for service in "${services_array[@]}"; do
            reset_service "$service"
        done
    else
        reset_all_services
    fi

    # 启动
    start_all_services

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    echo ""
    log_section "操作总结"
    echo "总耗时: ${duration}秒"

    # 验证
    if verify_containers; then
        log_success "Docker 环境就绪"
        exit 0
    else
        log_warning "部分服务可能需要手动检查"
        exit 1
    fi
}

# 启动
main "$@"
