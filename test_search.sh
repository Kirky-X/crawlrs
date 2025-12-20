#!/bin/bash
# 鸿蒙星光大赏搜索测试脚本

echo "=== 开始测试Google搜索：鸿蒙星光大赏 ==="
echo

# 搜索API端点
API_URL="http://localhost:8899/v1/search"

# 搜索请求数据
SEARCH_DATA='{
  "query": "鸿蒙星光大赏",
  "limit": 10,
  "lang": "zh-CN",
  "country": "CN"
}'

echo "请求数据: $SEARCH_DATA"
echo

# 发送搜索请求
echo "正在发送搜索请求..."
curl -X POST "$API_URL" \
  -H "Content-Type: application/json" \
  -d "$SEARCH_DATA" \
  -s | python3 -m json.tool || echo "直接输出原始响应:"

echo
echo "=== 搜索测试完成 ==="