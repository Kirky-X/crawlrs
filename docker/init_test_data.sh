#!/bin/bash
# Crawlrs 测试数据初始化脚本

set -e

# 数据库连接配置
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_NAME="${DB_NAME:-crawlrs}"
DB_USER="${DB_USER:-crawlrs}"
DB_PASSWORD="${DB_PASSWORD:-password}"

# 测试API密钥
TEST_API_KEY="test_api_key_$(date +%s)"
TEAM_ID="a1b2c3d4-e5f6-7890-abcd-ef1234567890"

echo "============================================"
echo "Crawlrs 测试数据初始化"
echo "============================================"
echo "API Key: $TEST_API_KEY"
echo "Team ID: $TEAM_ID"
echo "============================================"

# 执行SQL初始化
PGPASSWORD="$DB_PASSWORD" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" << EOF
-- 插入测试团队
INSERT INTO teams (id, name, created_at, updated_at)
VALUES ('$TEAM_ID'::uuid, 'Test Team', NOW(), NOW())
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = NOW();

-- 插入测试API密钥
INSERT INTO api_keys (id, key, team_id, created_at, updated_at)
VALUES (gen_random_uuid(), '$TEST_API_KEY', '$TEAM_ID'::uuid, NOW(), NOW())
ON CONFLICT (key) DO UPDATE SET team_id = EXCLUDED.team_id, updated_at = NOW();

-- 插入测试积分余额 (credits)
INSERT INTO credits (team_id, total_credits, used_credits, created_at, updated_at)
VALUES ('$TEAM_ID'::uuid, 10000, 0, NOW(), NOW())
ON CONFLICT (team_id) DO UPDATE SET total_credits = EXCLUDED.total_credits, used_credits = EXCLUDED.used_credits, updated_at = NOW();

-- 插入积分交易记录
INSERT INTO credits_transactions (id, team_id, api_key_id, amount, transaction_type, description, created_at)
VALUES (gen_random_uuid(), '$TEAM_ID'::uuid, (SELECT id FROM api_keys WHERE key = '$TEST_API_KEY'), 10000, 'credit', 'Initial test credits', NOW())
ON CONFLICT DO NOTHING;

SELECT '初始化完成!' AS status, '$TEST_API_KEY' AS api_key, '$TEAM_ID' AS team_id;
EOF

echo "============================================"
echo "测试数据初始化完成!"
echo "API Key: $TEST_API_KEY"
echo "============================================"

echo "$TEST_API_KEY" > /home/project/crawlrs/.test_api_key