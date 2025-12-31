#!/usr/bin/env bash
# 搜索引擎功能测试脚本

set -e

echo "=== 搜索引擎功能测试 ==="
echo ""

# 测试配置
API_KEY="test-api-key-for-search"
SEARCH_URL="http://localhost:8899/v1/search"

echo "1. 测试基本搜索功能 (Bing)..."
echo "发送请求: query='crawlrs test', engines=['bing']"

RESPONSE=$(curl -s -X POST "$SEARCH_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{"query": "crawlrs test", "engines": ["bing"]}' \
  -w "\nHTTP_STATUS:%{http_code}" \
  --max-time 60)

HTTP_STATUS=$(echo "$RESPONSE" | grep "HTTP_STATUS:" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | grep -v "HTTP_STATUS:")

echo "HTTP 状态码: $HTTP_STATUS"
echo ""

if [ "$HTTP_STATUS" = "200" ]; then
    echo "✅ 搜索成功！"
    echo "响应内容预览:"
    echo "$BODY" | head -c 1000
    echo ""
    echo "..."

    # 检查是否有搜索结果
    if echo "$BODY" | grep -q "results"; then
        RESULTS_COUNT=$(echo "$BODY" | grep -o '"results"[^}]*' | grep -o '[0-9]\+' | head -1 || echo "0")
        echo ""
        echo "搜索结果数量: $RESULTS_COUNT"
    fi
elif [ "$HTTP_STATUS" = "429" ]; then
    echo "⚠️  请求被速率限制 (429)"
    echo "这是预期的行为，因为当前没有有效的 API key"
elif [ "$HTTP_STATUS" = "502" ]; then
    echo "❌  服务暂时不可用 (502)"
    echo "可能是搜索引擎服务出现问题"
else
    echo "❌  请求失败，状态码: $HTTP_STATUS"
    echo "响应: $BODY"
fi

echo ""
echo "=== 测试完成 ==="
