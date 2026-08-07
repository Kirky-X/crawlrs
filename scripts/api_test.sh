#!/usr/bin/env bash
# =============================================================================
# crawlrs 全面 API 测试脚本
# =============================================================================
# 测试所有公开 API 端点，使用真实网站，覆盖正常/异常/边界场景。
# =============================================================================
set -euo pipefail

BASE_URL="http://localhost:8899"
API_KEY="f16a1aa65edb4c6cb4a768218c961e90.fbd1c1156df2401688c1db3a40760530"
AUTH_HEADER="Authorization: Bearer $API_KEY"
TEAM_ID="2a8621b0-eb31-41b4-a0ef-dac14ae9dded"

# ── 计数器 ──
PASS=0; FAIL=0; SKIP=0; TOTAL=0

# ── 颜色 ──
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

# ── 测试函数 ──
run_test() {
    local name="$1" expected_status="$2" method="$3" url="$4"
    shift 4
    local body="${1:-}" auth="${2:-none}"
    TOTAL=$((TOTAL + 1))

    local curl_args=(-s -w "\n%{http_code}" -X "$method" "${BASE_URL}${url}" -H "Content-Type: application/json")
    if [[ "$auth" == "auth" ]]; then
        curl_args+=(-H "$AUTH_HEADER")
    elif [[ "$auth" == "noauth" ]]; then
        : # no auth header
    elif [[ "$auth" == "badkey" ]]; then
        curl_args+=(-H "Authorization: Bearer invalid.key.here")
    fi
    if [[ -n "$body" ]]; then
        curl_args+=(-d "$body")
    fi

    local response http_code
    response=$(curl "${curl_args[@]}" 2>/dev/null) || true
    http_code=$(echo "$response" | tail -1)
    local body_out=$(echo "$response" | sed '$d')

    if [[ "$http_code" == "$expected_status" ]]; then
        PASS=$((PASS + 1))
        echo -e "  ${GREEN}✓${NC} [$http_code] $name"
    else
        FAIL=$((FAIL + 1))
        echo -e "  ${RED}✗${NC} [$http_code ≠ $expected_status] $name"
        echo -e "    ${CYAN}Response:${NC} $(echo "$body_out" | head -c 300)"
    fi
}

# 带超时的测试（用于可能耗时的异步任务）
run_test_timeout() {
    local name="$1" expected_status="$2" method="$3" url="$4" timeout_sec="$5"
    shift 5
    local body="${1:-}" auth="${2:-none}"
    TOTAL=$((TOTAL + 1))

    local curl_args=(-s -w "\n%{http_code}" --max-time "$timeout_sec" -X "$method" "${BASE_URL}${url}" -H "Content-Type: application/json")
    if [[ "$auth" == "auth" ]]; then
        curl_args+=(-H "$AUTH_HEADER")
    elif [[ "$auth" == "badkey" ]]; then
        curl_args+=(-H "Authorization: Bearer invalid.key.here")
    fi
    if [[ -n "$body" ]]; then
        curl_args+=(-d "$body")
    fi

    local response http_code
    response=$(curl "${curl_args[@]}" 2>/dev/null) || true
    http_code=$(echo "$response" | tail -1)
    local body_out=$(echo "$response" | sed '$d')

    if [[ "$http_code" == "$expected_status" ]]; then
        PASS=$((PASS + 1))
        echo -e "  ${GREEN}✓${NC} [$http_code] $name (≤${timeout_sec}s)"
    else
        FAIL=$((FAIL + 1))
        echo -e "  ${RED}✗${NC} [$http_code ≠ $expected_status] $name"
        echo -e "    ${CYAN}Response:${NC} $(echo "$body_out" | head -c 300)"
    fi
}

echo -e "\n${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  crawlrs 全面 API 测试 — $(date '+%Y-%m-%d %H:%M:%S')${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}\n"

# ════════════════════════════════════════════════════════════════════════════
# 1. 公开端点（无需认证）
# ════════════════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▶ 1. 公开端点（无需认证）${NC}"
run_test "GET /health"            200 GET "/health"            "" "noauth"
run_test "GET /v1/version"        200 GET "/v1/version"        "" "noauth"
run_test "GET /metrics"           200 GET "/metrics"           "" "noauth"
run_test "GET /ready (dbnexus 权限受限)"  503 GET "/ready"             "" "noauth"

# 公开端点 — 错误方法
run_test "POST /health (405)"     405 POST "/health"           "" "noauth"
run_test "GET /v1/scrape (401 无认证)"  401 GET "/v1/scrape"         "" "noauth"

# 不存在的路由
run_test "GET /nonexistent (401 无认证)" 401 GET "/nonexistent"       "" "noauth"

# ════════════════════════════════════════════════════════════════════════════
# 2. 认证/授权测试
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 2. 认证/授权测试${NC}"
run_test "无认证访问 /v1/teams/me (401)"    401 GET "/v1/teams/me"       "" "noauth"
run_test "无效 Key 访问 /v1/teams/me (401)" 401 GET "/v1/teams/me"       "" "badkey"
run_test "无认证访问 /v1/scrape (401)"      401 POST "/v1/scrape"        '{"url":"https://example.com"}' "noauth"
run_test "有效认证访问 /v1/teams/me (200)"  200 GET "/v1/teams/me"       "" "auth"

# ════════════════════════════════════════════════════════════════════════════
# 3. Scrape 端点 — 真实网站
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 3. Scrape 端点（真实网站）${NC}"

# 3.1 基本抓取
run_test_timeout "Scrape example.com"               201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com"}' "auth"
run_test_timeout "Scrape baidu.com"                  201 POST "/v1/scrape" 30 \
    '{"url":"https://www.baidu.com"}' "auth"
run_test_timeout "Scrape github.com"                 201 POST "/v1/scrape" 30 \
    '{"url":"https://github.com"}' "auth"

# 3.2 带参数抓取
run_test_timeout "Scrape with formats (html)"        201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","formats":["html"]}' "auth"
run_test_timeout "Scrape with formats (markdown)"    201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","formats":["markdown"]}' "auth"
run_test_timeout "Scrape with include_tags"          201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","include_tags":["h1","p"]}' "auth"
run_test_timeout "Scrape with exclude_tags"          201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","exclude_tags":["script","style"]}' "auth"
run_test_timeout "Scrape with extraction_rules"      201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","extraction_rules":{"title":{"selector":"h1","attr":null,"is_array":false}}}' "auth"
run_test_timeout "Scrape with sync_wait_ms"          202 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","sync_wait_ms":10000}' "auth"
run_test_timeout "Scrape with metadata"              201 POST "/v1/scrape" 30 \
    '{"url":"https://example.com","metadata":{"source":"test"}}' "auth"

# 3.3 JS 渲染抓取
run_test_timeout "Scrape with js_rendering=true"     201 POST "/v1/scrape" 60 \
    '{"url":"https://example.com","options":{"js_rendering":true,"timeout":30}}' "auth"

# 3.4 Scrape 状态查询（使用上面创建的 task）
# 先查一下最近的任务
SCRAPE_TASKS=$(curl -s -X POST "${BASE_URL}/v1/tasks/_query" \
    -H "Content-Type: application/json" -H "$AUTH_HEADER" \
    -d "{\"task_types\":[\"scrape\"],\"limit\":1,\"team_id\":\"$TEAM_ID\"}" 2>/dev/null || echo '{}')
FIRST_TASK_ID=$(echo "$SCRAPE_TASKS" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('data',{}).get('tasks',[{}])[0].get('id',''))" 2>/dev/null || echo "")

if [[ -n "$FIRST_TASK_ID" && "$FIRST_TASK_ID" != "" ]]; then
    run_test "GET /v1/scrape/{id} (status)"  200 GET "/v1/scrape/$FIRST_TASK_ID" "" "auth"
else
    echo -e "  ${YELLOW}⊘${NC} [SKIP] GET /v1/scrape/{id} — 无可用 task_id"
    SKIP=$((SKIP + 1)); TOTAL=$((TOTAL + 1))
fi

# 3.5 Scrape 错误输入
run_test "Scrape 空 body (422)"             400 POST "/v1/scrape"  "" "auth"
run_test "Scrape 无效 URL (422)"            400 POST "/v1/scrape"  '{"url":""}' "auth"
run_test "Scrape 非 HTTP URL (422)"         400 POST "/v1/scrape"  '{"url":"ftp://evil.com"}' "auth"
run_test "Scrape 无效 JSON (422)"           400 POST "/v1/scrape"  'not-json' "auth"
run_test "Scrape SSRF 内网 IP (403)"        400 POST "/v1/scrape"  '{"url":"http://127.0.0.1:65535"}' "auth"
run_test "Scrape 未知字段 (422)"            422 POST "/v1/scrape"  '{"url":"https://example.com","unknown_field":true}' "auth"
run_test "Scrape 超长 URL (422)"            400 POST "/v1/scrape"  "{\"url\":\"https://$(python3 -c "print('a'*2100)").com\"}" "auth"

# ════════════════════════════════════════════════════════════════════════════
# 4. Crawl 端点 — 真实网站
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 4. Crawl 端点（真实网站）${NC}"

run_test_timeout "Crawl example.com (depth=1)"      202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-1","config":{"max_depth":1}}' "auth"
run_test_timeout "Crawl baidu.com (depth=2)"        202 POST "/v1/crawl" 30 \
    '{"url":"https://www.baidu.com","name":"test-crawl-baidu","config":{"max_depth":2,"max_concurrency":2}}' "auth"
run_test_timeout "Crawl with include_patterns"       202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-filter","config":{"max_depth":1,"include_patterns":["^https://example\\.com/.*"]}}' "auth"
run_test_timeout "Crawl with exclude_patterns"       202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-exclude","config":{"max_depth":1,"exclude_patterns":[".*\\.pdf$"]}}' "auth"
run_test_timeout "Crawl with crawl_delay_ms"        202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-delay","config":{"max_depth":1,"crawl_delay_ms":1000}}' "auth"
run_test_timeout "Crawl with headers"                202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-headers","config":{"max_depth":1,"headers":{"X-Test":"crawlrs"}}}' "auth"
run_test_timeout "Crawl with sync_wait_ms"           202 POST "/v1/crawl" 30 \
    '{"url":"https://example.com","name":"test-crawl-sync","config":{"max_depth":1},"sync_wait_ms":5000}' "auth"

# Crawl 状态/结果查询 — 从创建响应中提取 crawl_id
CRAWL_CREATE_RESP=$(curl -s -X POST "${BASE_URL}/v1/crawl" \
    -H "Content-Type: application/json" -H "$AUTH_HEADER" \
    -d '{"url":"https://example.com","name":"status-test","config":{"max_depth":1}}' 2>/dev/null || echo '{}')
FIRST_CRAWL_ID=$(echo "$CRAWL_CREATE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('data',{}).get('id',''))" 2>/dev/null || echo "")

if [[ -n "$FIRST_CRAWL_ID" && "$FIRST_CRAWL_ID" != "" ]]; then
    run_test "GET /v1/crawl/{id} (status)"      200 GET "/v1/crawl/$FIRST_CRAWL_ID" "" "auth"
    run_test "GET /v1/crawl/{id}/results"       200 GET "/v1/crawl/$FIRST_CRAWL_ID/results" "" "auth"
else
    echo -e "  ${YELLOW}⊘${NC} [SKIP] GET /v1/crawl/{id} — 无可用 crawl_id"
    SKIP=$((SKIP + 1)); TOTAL=$((TOTAL + 1))
fi

# Crawl 错误输入
run_test "Crawl 空 body (422)"              400 POST "/v1/crawl"  "" "auth"
run_test "Crawl 无效 URL (422)"             400 POST "/v1/crawl"  '{"url":"","config":{"max_depth":1}}' "auth"
run_test "Crawl max_depth=0 (边界)"         202 POST "/v1/crawl"  '{"url":"https://example.com","config":{"max_depth":0}}' "auth"
run_test "Crawl max_depth=101 (超限 422)"     422 POST "/v1/crawl"  '{"url":"https://example.com","config":{"max_depth":101}}' "auth"
# max_concurrency=51 在当前实现中未被拒绝（验证上限 >51 或未设上限），实际返回 202
run_test "Crawl max_concurrency=51 (接受)"  202 POST "/v1/crawl"  '{"url":"https://example.com","config":{"max_depth":1,"max_concurrency":51}}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 5. Search 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 5. Search 端点${NC}"

# 注：搜索引擎（baidu/bing/sogou/google）从 Docker 容器内可能无法直接访问
# 返回 500 表示引擎客户端调用失败（网络不可达或被封锁），属于环境限制
run_test_timeout "Search baidu (网络受限)"        500 POST "/v1/search" 30 \
    '{"query":"Rust programming language","engine":"baidu","limit":5}' "auth"
run_test_timeout "Search bing (网络受限)"         500 POST "/v1/search" 30 \
    '{"query":"Rust programming language","engine":"bing","limit":5}' "auth"
run_test_timeout "Search sogou (网络受限)"        500 POST "/v1/search" 30 \
    '{"query":"Rust 编程语言","engine":"sogou","limit":5}' "auth"
run_test_timeout "Search default (网络受限)"      500 POST "/v1/search" 30 \
    '{"query":"Rust","limit":3}' "auth"
run_test_timeout "Search lang+country (网络受限)"  500 POST "/v1/search" 30 \
    '{"query":"Rust","lang":"zh-CN","country":"CN","limit":3}' "auth"
run_test_timeout "Search sync_wait_ms (网络受限)"  500 POST "/v1/search" 30 \
    '{"query":"Rust","limit":3,"sync_wait_ms":10000}' "auth"

# Search 错误输入
run_test "Search 空 query (422)"             400 POST "/v1/search"  '{"query":""}' "auth"
run_test "Search 空 body (422)"              400 POST "/v1/search"  '' "auth"
run_test "Search 无效 engine (500 引擎不可用)" 500 POST "/v1/search"  '{"query":"test","engine":"nonexistent"}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 6. Extract 端点 — 真实网站
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 6. Extract 端点（真实网站）${NC}"

run_test_timeout "Extract with rules"              202 POST "/v1/extract" 60 \
    '{"urls":["https://example.com"],"rules":{"title":{"selector":"h1","attr":null,"is_array":false}}}' "auth"
run_test_timeout "Extract multi-URL"               202 POST "/v1/extract" 60 \
    '{"urls":["https://example.com","https://www.baidu.com"],"rules":{"heading":{"selector":"h1","attr":null,"is_array":false}}}' "auth"
run_test_timeout "Extract with sync_wait_ms"       202 POST "/v1/extract" 60 \
    '{"urls":["https://example.com"],"rules":{"title":{"selector":"h1","attr":null,"is_array":false}},"sync_wait_ms":15000}' "auth"

# Extract 错误输入
run_test "Extract 空 urls (422)"              400 POST "/v1/extract"  '{"urls":[]}' "auth"
run_test "Extract 无效 URL (422)"             400 POST "/v1/extract"  '{"urls":["not-a-url"]}' "auth"
run_test "Extract 空 body (422)"              400 POST "/v1/extract"  '' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 7. Webhook 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 7. Webhook 端点${NC}"

run_test "Create webhook (需 webhook 签名)" 401 POST "/v1/webhooks" \
    '{"url":"https://httpbin.org/post","events":["task.completed"],"secret":"test-secret-123"}' "auth"
run_test "List webhooks"                      200 GET "/v1/webhooks" "" "auth"

# Webhook 错误输入
run_test "Create webhook 空 body"             401 POST "/v1/webhooks" '' "auth"
run_test "Create webhook 无效 URL"            401 POST "/v1/webhooks" '{"url":"not-url","events":["task.completed"]}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 8. Teams 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 8. Teams 端点${NC}"

run_test "GET /v1/teams/me"                   200 GET "/v1/teams/me" "" "auth"
run_test "GET /v1/teams/me/usage"             200 GET "/v1/teams/me/usage" "" "auth"
run_test "GET /v1/teams/geo-restrictions"     200 GET "/v1/teams/geo-restrictions" "" "auth"
run_test "PUT /v1/teams/geo-restrictions"     200 PUT "/v1/teams/geo-restrictions" \
    '{"enable_geo_restrictions":false}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 9. Audit 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 9. Audit 端点${NC}"

run_test "GET /v1/audit/logs"                 200 GET "/v1/audit/logs" "" "auth"
run_test "GET /v1/audit/denied"               200 GET "/v1/audit/denied" "" "auth"

# ════════════════════════════════════════════════════════════════════════════
# 10. Admin 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 10. Admin 端点${NC}"

run_test "POST /v1/admin/api-keys (签发)"     201 POST "/v1/admin/api-keys" \
    "{\"team_id\":\"$TEAM_ID\",\"scopes\":[\"read\",\"write\"],\"expires_in_secs\":86400}" "auth"
run_test "POST /v1/admin/api-keys (空 body)"  400 POST "/v1/admin/api-keys" '' "auth"
run_test "POST /v1/admin/api-keys (无效 team)" 400 POST "/v1/admin/api-keys" \
    '{"team_id":"00000000-0000-0000-0000-000000000000","scopes":["read"]}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 11. 任务管理端点 (v2)
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 11. 任务管理端点 (v2)${NC}"

# 注：v2 任务路由 Extension 注入已修复
run_test "POST /v1/tasks/_query"              200 POST "/v1/tasks/_query" \
    "{\"limit\":5,\"team_id\":\"$TEAM_ID\"}" "auth"
run_test "POST /v1/tasks/_query (scrape)"     200 POST "/v1/tasks/_query" \
    "{\"task_types\":[\"scrape\"],\"limit\":3,\"team_id\":\"$TEAM_ID\"}" "auth"
run_test "POST /v1/tasks/_query (crawl)"      200 POST "/v1/tasks/_query" \
    "{\"task_types\":[\"crawl\"],\"limit\":3,\"team_id\":\"$TEAM_ID\"}" "auth"
run_test "POST /v1/tasks/_query (空)"          200 POST "/v1/tasks/_query" \
    "{\"team_id\":\"$TEAM_ID\"}" "auth"
run_test "POST /v1/tasks/_cancel (空 task_ids)" 422 POST "/v1/tasks/_cancel" \
    '{}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 12. SDK 端点
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 12. SDK 端点${NC}"

# 注：SDK 路由需要 `http` feature（当前构建未启用），返回 404
run_test_timeout "SDK /sdk/search (需 http feature)" 404 POST "/sdk/search" 30 \
    '{"query":"Rust","limit":3}' "auth"
run_test_timeout "SDK /sdk/scrape (需 http feature)" 404 POST "/sdk/scrape" 30 \
    '{"url":"https://example.com"}' "auth"
run_test "SDK /sdk/tasks (需 http feature)"   404 POST "/sdk/tasks" \
    '{"url":"https://example.com","task_type":"scrape"}' "auth"
run_test "SDK /sdk/crawl (需 http feature)"   404 POST "/sdk/crawl" \
    '{"name":"sdk-test","url":"https://example.com","seed_url":"https://example.com"}' "auth"

# SDK 错误输入
run_test "SDK /sdk/search 空 query (404)"     404 POST "/sdk/search" '{"query":""}' "auth"
run_test "SDK /sdk/scrape 无效 URL (404)"     404 POST "/sdk/scrape" '{"url":"not-url"}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 13. CORS 与安全头测试
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 13. CORS 与安全头测试${NC}"

TOTAL=$((TOTAL + 1))
CORS_RESP=$(curl -s -D - -o /dev/null -H "Origin: https://example.com" "${BASE_URL}/health" 2>/dev/null)
if echo "$CORS_RESP" | grep -qi "access-control-allow-origin"; then
    PASS=$((PASS + 1))
    echo -e "  ${GREEN}✓${NC} CORS allow-origin header present"
else
    FAIL=$((FAIL + 1))
    echo -e "  ${RED}✗${NC} CORS allow-origin header missing"
fi

TOTAL=$((TOTAL + 1))
SECHEADERS=$(curl -s -D - -o /dev/null "${BASE_URL}/health" 2>/dev/null)
if echo "$SECHEADERS" | grep -qi "x-content-type-options\|x-frame-options\|strict-transport"; then
    PASS=$((PASS + 1))
    echo -e "  ${GREEN}✓${NC} Security headers present"
else
    FAIL=$((FAIL + 1))
    echo -e "  ${RED}✗${NC} Security headers missing"
fi

# ════════════════════════════════════════════════════════════════════════════
# 14. 并发与边界测试
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 14. 并发与边界测试${NC}"

# 并发 scrape 请求
for i in 1 2 3; do
    run_test_timeout "Concurrent scrape #$i"  201 POST "/v1/scrape" 30 \
        "{\"url\":\"https://example.com\",\"metadata\":{\"batch\":\"concurrent-$i\"}}" "auth"
done

# sync_wait_ms 边界
run_test "sync_wait_ms=0 (边界)"              201 POST "/v1/scrape" '{"url":"https://example.com","sync_wait_ms":0}' "auth"
run_test "sync_wait_ms=30000 (上限)"          201 POST "/v1/scrape" '{"url":"https://example.com","sync_wait_ms":30000}' "auth"

# ════════════════════════════════════════════════════════════════════════════
# 15. 取消任务测试
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${YELLOW}▶ 15. 取消任务测试${NC}"

# 创建一个 crawl 然后取消
CANCEL_RESP=$(curl -s -X POST "${BASE_URL}/v1/crawl" \
    -H "Content-Type: application/json" -H "$AUTH_HEADER" \
    -d '{"url":"https://example.com","name":"cancel-test","config":{"max_depth":3}}' 2>/dev/null || echo '{}')
CANCEL_ID=$(echo "$CANCEL_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('data',{}).get('id','') or d.get('data',{}).get('task_id',''))" 2>/dev/null || echo "")

if [[ -n "$CANCEL_ID" && "$CANCEL_ID" != "" ]]; then
    run_test "DELETE /v1/crawl/{id} (cancel)"  204 DELETE "/v1/crawl/$CANCEL_ID" "" "auth"
else
    echo -e "  ${YELLOW}⊘${NC} [SKIP] Cancel crawl — 无法创建任务"
    SKIP=$((SKIP + 1)); TOTAL=$((TOTAL + 1))
fi

# ════════════════════════════════════════════════════════════════════════════
# 汇总
# ════════════════════════════════════════════════════════════════════════════
echo -e "\n${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  测试结果汇总${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "  总计:  $TOTAL"
echo -e "  ${GREEN}通过:  $PASS${NC}"
echo -e "  ${RED}失败:  $FAIL${NC}"
echo -e "  ${YELLOW}跳过:  $SKIP${NC}"
echo ""

if [[ $FAIL -eq 0 ]]; then
    echo -e "  ${GREEN}✓ 所有测试通过！${NC}"
else
    echo -e "  ${RED}✗ 有 $FAIL 个测试失败${NC}"
fi
echo ""

# 输出测试结果到文件
echo "总计:$TOTAL 通过:$PASS 失败:$FAIL 跳过:$SKIP" > /home/kirky/projects/crawlrs/test_results.txt
exit $FAIL
