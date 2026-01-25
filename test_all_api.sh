#!/bin/bash

# =============================================================================
# Crawlrs API 全接口测试脚本
# =============================================================================
# 测试所有 API 接口，输出完整的请求和响应信息
# =============================================================================

# set -e  # 注释掉，避免单个测试失败导致整个脚本退出

# 配置
BASE_URL="${BASE_URL:-http://localhost:8899}"
API_KEY="${API_KEY:-crawlrs_test_key_12345678901234567890}"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m'
BOLD='\033[1m'

# 统计变量
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# 打印分隔线
print_separator() {
    echo -e "${CYAN}=========================================${NC}"
}

# 打印测试标题
print_test_title() {
    local test_num=$1
    local test_name=$2
    echo -e "\n${BOLD}${PURPLE}[TEST ${test_num}] ${test_name}${NC}"
    print_separator
}

# 执行测试
run_test() {
    local test_num=$1
    local test_name=$2
    local method=$3
    local endpoint=$4
    local headers=$5
    local body=$6

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    print_test_title "$test_num" "$test_name"

    local url="${BASE_URL}${endpoint}"

    # 打印请求
    echo -e "${BLUE}📤 REQUEST:${NC}"
    echo -e "  ${CYAN}Method:${NC} ${BOLD}${method}${NC}"
    echo -e "  ${CYAN}URL:${NC}    ${BOLD}${url}${NC}"

    if [ -n "$headers" ]; then
        echo -e "  ${CYAN}Headers:${NC}    $headers"
    fi

    if [ -n "$body" ]; then
        echo -e "  ${CYAN}Body:${NC}"
        echo "$body" | jq '.' 2>/dev/null || echo "$body" | sed 's/^/    /'
    fi
    echo ""

    # 执行请求
    local full_response
    if [ "$method" = "GET" ]; then
        full_response=$(curl -s -i -X GET "$url" $headers 2>&1)
    elif [ "$method" = "POST" ]; then
        full_response=$(curl -s -i -X POST "$url" $headers -d "$body" 2>&1)
    elif [ "$method" = "DELETE" ]; then
        full_response=$(curl -s -i -X DELETE "$url" $headers 2>&1)
    elif [ "$method" = "PUT" ]; then
        full_response=$(curl -s -i -X PUT "$url" $headers -d "$body" 2>&1)
    fi

    # 解析响应
    local status_code=$(echo "$full_response" | grep -i "^HTTP/" | head -1 | awk '{print $2}')
    local response_body=$(echo "$full_response" | sed -n '/^\r$/q;p' | sed '1,/^$/d')

    # 打印响应
    echo -e "${GREEN}📥 RESPONSE:${NC}"
    echo -e "  ${CYAN}Status Code:${NC} ${BOLD}${status_code}${NC}"

    if [ -n "$response_body" ]; then
        echo -e "  ${CYAN}Body:${NC}"
        echo "$response_body" | jq '.' 2>/dev/null || echo "$response_body" | sed 's/^/    /'
    fi
    echo ""

    # 检查状态码
    local status_first_digit=${status_code:0:1}
    if [ "$status_first_digit" = "2" ]; then
        echo -e "${GREEN}✓ PASSED${NC}"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    elif [ "$status_first_digit" = "4" ] || [ "$status_first_digit" = "5" ]; then
        echo -e "${RED}✗ FAILED${NC} - HTTP $status_code"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    else
        echo -e "${YELLOW}⚠ WARNING${NC} - HTTP $status_code"
    fi
}

# 默认请求头
AUTH_HEADERS="-H 'Authorization: Bearer $API_KEY' -H 'Content-Type: application/json'"

# 打印标题
echo -e "${BOLD}${BLUE}"
cat << "EOF"
╔════════════════════════════════════════════════════════════╗
║                                                            ║
║   Crawlrs API 全接口测试脚本                                ║
║                                                            ║
╚════════════════════════════════════════════════════════════╝
EOF
echo -e "${NC}"
echo -e "${CYAN}配置:${NC}  Base URL: $BASE_URL"
echo -e "${CYAN}       API Key:  ${API_KEY:0:25}..."
print_separator

# =============================================================================
# 1. 公开接口
# =============================================================================

run_test "1" "健康检查" "GET" "/health" "" ""

run_test "2" "版本信息" "GET" "/v1/version" "" ""

# =============================================================================
# 2. 团队接口
# =============================================================================

run_test "3" "获取团队信息" "GET" "/v1/teams/me" "$AUTH_HEADERS" ""

run_test "4" "获取使用统计" "GET" "/v1/teams/me/usage" "$AUTH_HEADERS" ""

# =============================================================================
# 3. Webhook 接口
# =============================================================================

run_test "5" "Webhook 列表" "GET" "/v1/webhooks" "$AUTH_HEADERS" ""

run_test "6" "创建 Webhook" "POST" "/v1/webhooks" "$AUTH_HEADERS" \
    '{"url":"https://httpbin.org/webhook/test"}'

# =============================================================================
# 4. Scrape 接口
# =============================================================================

run_test "7" "创建抓取任务" "POST" "/v1/scrape" "$AUTH_HEADERS" \
    '{"url":"https://example.com"}'

# 获取任务ID
SCRAPE_RESPONSE=$(curl -s -X POST "$BASE_URL/v1/scrape" \
    -H "Authorization: Bearer $API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"url":"https://example.com"}')
SCRAPE_ID=$(echo "$SCRAPE_RESPONSE" | jq -r '.data.id // .id // empty' 2>/dev/null)

if [ -n "$SCRAPE_ID" ]; then
    run_test "8" "获取抓取状态" "GET" "/v1/scrape/$SCRAPE_ID" "$AUTH_HEADERS" ""
fi

# =============================================================================
# 5. Extract 接口
# =============================================================================

run_test "9" "创建提取任务" "POST" "/v1/extract" "$AUTH_HEADERS" \
    '{"urls":["https://example.com"],"schema":{"title":"h1"}}'

# =============================================================================
# 6. Crawl 接口
# =============================================================================

run_test "10" "创建爬取任务" "POST" "/v1/crawl" "$AUTH_HEADERS" \
    '{"url":"https://example.com","name":"API Test Crawl","config":{"max_depth":1}}'

# 获取任务ID
CRAWL_RESPONSE=$(curl -s -X POST "$BASE_URL/v1/crawl" \
    -H "Authorization: Bearer $API_KEY" \
    -H "Content-Type: application/json" \
    -d '{"url":"https://example.com","name":"API Test Crawl 2","config":{"max_depth":1}}')
CRAWL_ID=$(echo "$CRAWL_RESPONSE" | jq -r '.data.id // .id // empty' 2>/dev/null)

if [ -n "$CRAWL_ID" ]; then
    run_test "11" "获取爬取状态" "GET" "/v1/crawl/$CRAWL_ID" "$AUTH_HEADERS" ""

    run_test "12" "获取爬取结果" "GET" "/v1/crawl/$CRAWL_ID/results" "$AUTH_HEADERS" ""
fi

# =============================================================================
# 7. Search 接口
# =============================================================================

run_test "13" "搜索" "POST" "/v1/search" "$AUTH_HEADERS" \
    '{"query":"test","limit":3}'

# =============================================================================
# 8. 地理限制接口
# =============================================================================

run_test "14" "获取地理限制" "GET" "/v1/teams/geo-restrictions" "$AUTH_HEADERS" ""

run_test "15" "更新地理限制" "PUT" "/v1/teams/geo-restrictions" "$AUTH_HEADERS" \
    '{"enabled":false,"allowed_countries":["*"]}'

# =============================================================================
# 统计
# =============================================================================

echo -e "\n"
print_separator
echo -e "${BOLD}${BLUE}测试统计${NC}"
print_separator
echo -e "  ${CYAN}总测试数:${NC} $TOTAL_TESTS"
echo -e "  ${GREEN}通过:${NC}     $PASSED_TESTS"
echo -e "  ${RED}失败:${NC}     $FAILED_TESTS"

if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "\n${GREEN}${BOLD}✓ 所有测试通过！${NC}"
    exit 0
else
    echo -e "\n${RED}${BOLD}✗ 有 $FAILED_TESTS 个测试失败${NC}"
    exit 1
fi
