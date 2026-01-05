#!/bin/bash

# 优化后的集成测试运行脚本
#
# 反爬虫保护措施：
# - 随机延迟（3-8秒）避免固定模式
# - User-Agent轮换模拟不同浏览器
# - 完整的浏览器请求头
# - 减少并发请求数量
#
# 用法：
#   ./run_optimized_tests.sh                    # 运行所有优化测试
#   ./run_optimized_tests.sh scrape            # 只运行网页采集测试
#   ./run_optimized_tests.sh search            # 只运行搜索引擎测试
#   ./run_optimized_tests.sh combined          # 只运行综合测试
#   ./run_optimized_tests.sh test_random_news_scrape  # 运行特定测试

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 打印带颜色的消息
print_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_important() {
    echo -e "${MAGENTA}🔒 $1${NC}"
}

# 显示帮助信息
show_help() {
    cat << EOF
优化后的集成测试运行脚本

反爬虫保护措施：
  🔒 随机延迟（3-8秒）避免固定模式
  🔒 User-Agent轮换模拟不同浏览器
  🔒 完整的浏览器请求头
  🔒 减少并发请求数量

用法：
  $0 [选项] [测试名称]

选项：
  -h, --help     显示帮助信息
  -v, --verbose  显示详细输出

测试类别：
  scrape         只运行网页采集测试
  search         只运行搜索引擎测试
  combined       只运行综合测试
  all            运行所有优化测试（默认）

特定测试：
  test_random_news_scrape                    测试随机新闻网页采集
  test_multiple_random_news_scrape          测试多次随机新闻网页采集
  test_search_engines_with_random_keyword   测试搜索引擎（随机关键词）
  test_multiple_random_keyword_search       测试多次随机关键词搜索
  test_search_results_deduplication         测试搜索结果去重
  test_combined_random_scrape_and_search    综合测试

示例：
  $0                                    # 运行所有优化测试
  $0 -v                                 # 运行所有优化测试（详细输出）
  $0 scrape                             # 只运行网页采集测试
  $0 test_random_news_scrape             # 运行特定测试

注意事项：
  ⚠️  测试包含随机延迟（3-8秒），请耐心等待
  ⚠️  避免频繁运行测试，防止IP被封
  ⚠️  建议间隔至少1小时再运行测试

EOF
}

# 解析命令行参数
VERBOSE=""
TEST_CATEGORY="all"
TEST_NAME=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -v|--verbose)
            VERBOSE="--nocapture"
            shift
            ;;
        scrape|search|combined|all)
            TEST_CATEGORY="$1"
            shift
            ;;
        test_*)
            TEST_NAME="$1"
            shift
            ;;
        *)
            print_error "未知选项: $1"
            show_help
            exit 1
            ;;
    esac
done

# 检查 Cargo 是否安装
if ! command -v cargo &> /dev/null; then
    print_error "Cargo 未安装，请先安装 Rust 工具链"
    exit 1
fi

# 显示反爬虫保护信息
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
print_important "反爬虫保护措施已启用"
print_info "  • 随机延迟（3-8秒）避免固定模式"
print_info "  • User-Agent轮换模拟不同浏览器"
print_info "  • 完整的浏览器请求头"
print_info "  • 减少并发请求数量"
echo -e "${YELLOW}⚠️  测试包含随机延迟，请耐心等待${NC}"
echo -e "${YELLOW}⚠️  避免频繁运行测试，防止IP被封${NC}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# 构建测试命令
BASE_CMD="cargo test --test integration_tests"

case $TEST_CATEGORY in
    scrape)
        print_info "运行网页采集测试..."
        TEST_PATTERN="test_random_news_scrape|test_multiple_random_news_scrape"
        ;;
    search)
        print_info "运行搜索引擎测试..."
        TEST_PATTERN="test_search_engines_with_random_keyword|test_multiple_random_keyword_search|test_search_results_deduplication"
        ;;
    combined)
        print_info "运行综合测试..."
        TEST_PATTERN="test_combined_random_scrape_and_search"
        ;;
    all)
        print_info "运行所有优化测试..."
        TEST_PATTERN="optimized_tests"
        ;;
esac

# 如果指定了特定测试，则覆盖 TEST_PATTERN
if [ -n "$TEST_NAME" ]; then
    print_info "运行特定测试: $TEST_NAME"
    TEST_PATTERN="$TEST_NAME"
fi

# 构建完整命令
FULL_CMD="$BASE_CMD $TEST_PATTERN $VERBOSE"

print_info "执行命令: $FULL_CMD"
echo ""

# 运行测试
if eval $FULL_CMD; then
    echo ""
    print_success "测试完成！"
    echo ""
    print_info "反爬虫保护措施已成功应用"
    echo ""
    print_warning "建议："
    print_warning "  • 避免频繁运行测试（建议间隔至少1小时）"
    print_warning "  • 如遇IP被封，请更换IP地址或使用代理"
    print_warning "  • 查看反爬虫保护文档了解更多信息：tests/integration/ANTI_BOT_PROTECTION.md"
    echo ""
    exit 0
else
    echo ""
    print_error "测试失败！"
    echo ""
    print_warning "可能的原因："
    print_warning "  • 网络连接问题"
    print_warning "  • 目标网站反爬虫机制"
    print_warning "  • IP地址被封"
    print_warning "  • 搜索引擎限制"
    echo ""
    print_info "建议："
    print_info "  • 检查网络连接"
    print_info "  • 等待一段时间后重试"
    print_info "  • 更换IP地址或使用代理"
    print_info "  • 查看反爬虫保护文档：tests/integration/ANTI_BOT_PROTECTION.md"
    echo ""
    exit 1
fi