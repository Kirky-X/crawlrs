#!/bin/bash

# =============================================================================
# Crawlrs Docker 测试运行脚本
# =============================================================================
# 使用 Docker Compose 测试环境运行集成测试
#
# 使用方法:
#   ./run_tests_docker.sh              # 运行所有测试
#   ./run_tests_docker.sh api         # 仅运行 API 测试
#   ./run_tests_docker.sh unit        # 仅运行单元测试
#   ./run_tests_docker.sh full        # 运行所有测试 (包含被忽略的)
#   ./run_tests_docker.sh cleanup     # 清理 Docker 测试环境
# =============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 默认配置
COMPOSE_FILE="docker/docker-compose.test.yml"
RESULTS_DIR="docker/test-results"

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

# 显示帮助信息
show_help() {
    echo "Crawlrs Docker 测试运行脚本"
    echo ""
    echo "使用方法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  api         仅运行 API 集成测试"
    echo "  unit        仅运行单元测试"
    echo "  full        运行所有测试 (包含被忽略的)"
    echo "  cleanup      清理 Docker 测试环境"
    echo "  help         显示此帮助信息"
    echo ""
    echo "选项:"
    echo "  --with-minio     启动 MinIO (用于 S3 存储测试)"
    echo "  --with-browser   启动浏览器服务 (用于浏览器测试)"
    echo "  --with-search    启用搜索测试 (需要 API 密钥)"
    echo "  --verbose        显示详细输出"
    echo ""
    echo "示例:"
    echo "  $0 api                    # 运行 API 测试"
    echo "  $0 full --with-minio      # 运行所有测试 (含 S3)"
    echo "  $0 cleanup                # 清理环境"
}

# 解析参数
PARAMS=""
WITH_MINIO=false
WITH_BROWSER=false
WITH_SEARCH=false
VERBOSE=false
COMMAND="all"

while [[ $# -gt 0 ]]; do
    case $1 in
        api|unit|full|cleanup|help)
            COMMAND=$1
            shift
            ;;
        --with-minio)
            WITH_MINIO=true
            shift
            ;;
        --with-browser)
            WITH_BROWSER=true
            shift
            ;;
        --with-search)
            WITH_SEARCH=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        *)
            log_error "未知参数: $1"
            show_help
            exit 1
            ;;
    esac
done

# 切换到项目根目录
cd "$(dirname "$0")"

# 创建必要目录
mkdir -p "$RESULTS_DIR"
mkdir -p docker/data/test-postgres
mkdir -p docker/data/minio

# 检查 Docker 是否运行
check_docker() {
    log_section "检查 Docker 环境"
    if ! docker info &> /dev/null; then
        log_error "Docker 未运行，请先启动 Docker"
        exit 1
    fi
    log_success "Docker 运行正常"
}

# 停止并清理 Docker 测试环境
cleanup() {
    log_section "清理 Docker 测试环境"

    # 停止服务
    docker-compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true

    # 清理数据卷
    docker volume rm crawlrs-docker_test_db_data 2>/dev/null || true
    docker volume rm crawlrs-docker_test_redis_data 2>/dev/null || true
    docker volume rm crawlrs-docker_minio_data 2>/dev/null || true

    # 清理目录
    rm -rf docker/data/test-postgres/* 2>/dev/null || true
    rm -rf docker/data/minio/* 2>/dev/null || true
    rm -rf "$RESULTS_DIR"/* 2>/dev/null || true

    log_success "清理完成"
}

# 启动测试环境
start_environment() {
    log_section "启动 Docker 测试环境"

    # 启动基础设施
    log_info "启动数据库和 Redis..."
    docker-compose -f "$COMPOSE_FILE" up -d test-db test-redis

    # 等待数据库就绪
    log_info "等待数据库就绪..."
    for i in {1..30}; do
        if docker exec crawlrs-test-db pg_isready -U crawlrs &>/dev/null; then
            log_success "数据库已就绪"
            break
        fi
        if [ $i -eq 30 ]; then
            log_error "数据库启动超时"
            exit 1
        fi
        sleep 1
    done

    # 启动 MinIO (如果需要)
    if [ "$WITH_MINIO" = true ]; then
        log_info "启动 MinIO..."
        docker-compose -f "$COMPOSE_FILE" up -d minio
        sleep 3
    fi

    log_success "测试环境已启动"
}

# 构建测试镜像
build_test_image() {
    log_section "构建测试镜像"
    docker-compose -f "$COMPOSE_FILE" build test-runner
    log_success "测试镜像构建完成"
}

# 运行测试
run_tests() {
    local test_args="$1"
    local test_name="$2"

    log_section "运行 $test_name"

    # 构建并运行测试
    if [ "$VERBOSE" = true ]; then
        docker-compose -f "$COMPOSE_FILE" run --rm test-runner cargo test --features full $test_args
    else
        docker-compose -f "$COMPOSE_FILE" run --rm test-runner cargo test --features full $test_args 2>&1 | tee "$RESULTS_DIR/test_output.log"
    fi

    local exit_code=$?

    if [ $exit_code -eq 0 ]; then
        log_success "$test_name 测试通过"
    else
        log_error "$test_name 测试失败"
        log_warning "查看测试日志: $RESULTS_DIR/test_output.log"
    fi

    return $exit_code
}

# 显示测试结果摘要
show_summary() {
    log_section "测试结果摘要"

    if [ -f "$RESULTS_DIR/test_output.log" ]; then
        local passed=$(grep -c "test result:.*passed" "$RESULTS_DIR/test_output.log" 2>/dev/null || echo "0")
        local failed=$(grep -c "test result:.*failed" "$RESULTS_DIR/test_output.log" 2>/dev/null || echo "0")
        local ignored=$(grep -c "test result:.*ignored" "$RESULTS_DIR/test_output.log" 2>/dev/null || echo "0")

        echo -e "通过: ${GREEN}$passed${NC}"
        echo -e "失败: ${RED}$failed${NC}"
        echo -e "忽略: ${YELLOW}$ignored${NC}"
    else
        log_warning "未找到测试结果日志"
    fi

    log_info "详细结果: $RESULTS_DIR/"
}

# 主函数
main() {
    echo ""
    log_section "Crawlrs Docker 测试环境"
    echo "命令: $COMMAND"
    echo "MinIO: $WITH_MINIO, 浏览器: $WITH_BROWSER, 搜索: $WITH_SEARCH"
    echo ""

    case $COMMAND in
        help)
            show_help
            exit 0
            ;;
        cleanup)
            cleanup
            exit 0
            ;;
        *)
            check_docker
            cleanup
            start_environment
            build_test_image

            # 根据命令运行不同测试
            case $COMMAND in
                api)
                    run_tests "--test integration_tests api_tests" "API 集成"
                    ;;
                unit)
                    run_tests "--lib" "单元"
                    ;;
                full)
                    run_tests "--test integration_tests -- --include-ignored" "完整集成"
                    ;;
                all)
                    run_tests "--test integration_tests" "集成"
                    ;;
            esac

            show_summary
            ;;
    esac
}

# 启动
main
