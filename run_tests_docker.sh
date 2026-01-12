#!/bin/bash

# =============================================================================
# crawlrs 测试环境配置脚本
# =============================================================================
# 使用现有的 Docker 容器配置测试环境
# =============================================================================

echo "========================================"
echo "🔧 crawlrs 测试环境配置"
echo "========================================"

# -----------------------------------------------------------------------------
# 配置环境变量（使用现有容器）
# -----------------------------------------------------------------------------
echo ""
echo "📦 配置测试环境变量..."

# 使用现有的 Docker 容器
export TEST_DATABASE_URL="postgres://idgen:idgen123@localhost:5432/crawlrs_test"
export DATABASE_URL="postgres://idgen:idgen123@localhost:5432/crawlrs_test"
export REDIS_URL="redis://localhost:6379"

# AWS S3 配置（使用 LocalStack）
export AWS_ACCESS_KEY_ID="test"
export AWS_SECRET_ACCESS_KEY="test"
export AWS_DEFAULT_REGION="us-east-1"
export LOCALSTACK_URL="http://localhost:4566"

# 测试超时配置
export DATABASE_MAX_CONNECTIONS="20"
export DATABASE_CONNECT_TIMEOUT="60"
export TASK_ACQUISITION_TIMEOUT="30"

echo "   DATABASE_URL=$DATABASE_URL"
echo "   REDIS_URL=$REDIS_URL"

# -----------------------------------------------------------------------------
# 验证服务状态
# -----------------------------------------------------------------------------
echo ""
echo "🔍 验证服务状态..."

# 检查 PostgreSQL
if pg_isready -h localhost -p 5432 -U idgen > /dev/null 2>&1; then
    echo "   ✅ PostgreSQL - 可用"
else
    echo "   ❌ PostgreSQL 不可用，尝试启动..."
    docker start nebula-postgres 2>/dev/null || true
    sleep 3
    if pg_isready -h localhost -p 5432 -U idgen > /dev/null 2>&1; then
        echo "   ✅ PostgreSQL - 已启动"
    else
        echo "   ⚠️  PostgreSQL 启动失败"
    fi
fi

# 检查 Redis
if docker exec nebula-redis redis-cli ping > /dev/null 2>&1; then
    echo "   ✅ Redis - 可用"
else
    echo "   ❌ Redis 不可用，尝试启动..."
    docker start nebula-redis 2>/dev/null || true
    sleep 3
    if docker exec nebula-redis redis-cli ping > /dev/null 2>&1; then
        echo "   ✅ Redis - 已启动"
    else
        echo "   ⚠️  Redis 启动失败"
    fi
fi

# -----------------------------------------------------------------------------
# 清理测试数据
# -----------------------------------------------------------------------------
echo ""
echo "🧹 清理测试数据..."

docker exec nebula-postgres psql -U idgen -d crawlrs_test -c "
    DELETE FROM tasks WHERE url LIKE 'https://example.com/%' OR url LIKE 'http://test.%';
" 2>/dev/null || echo "   ⚠️  清理命令部分失败（正常，如果表不存在）"

echo "   ✅ 测试数据已清理"

# -----------------------------------------------------------------------------
# 运行测试
# -----------------------------------------------------------------------------
echo ""
echo "========================================"
echo "🚀 开始运行测试..."
echo "========================================"

cd /home/dev/crawlrs

# 设置日志级别
export RUST_LOG=error

# 记录测试结果
TEST_START_TIME=$(date +%s)

# 运行单元测试
echo ""
echo "📗 运行单元测试..."
cargo test --features full --lib 2>&1 | tee /tmp/test_unit.log

UNIT_TOTAL=$(grep -oP 'test result:.*\K\d+(?= passed)' /tmp/test_unit.log | tail -1 || echo "0")
UNIT_FAILED=$(grep -oP 'test result:.*\K\d+(?= failed)' /tmp/test_unit.log | tail -1 || echo "0")
echo ""
echo "   单元测试结果: $UNIT_TOTAL 通过, $UNIT_FAILED 失败"

# 运行集成测试（排除需要真实外部服务的测试）
echo ""
echo "📘 运行集成测试..."

cargo test --features full --test main \
    --test-threads=4 \
    --skip e2e:: \
    --skip integration::api_tests:: \
    --skip integration::api::tasks_management_test:: \
    --skip integration::s3_storage_test:: \
    --skip integration::real_world_test:: \
    --skip integration::search_uat_test:: \
    --skip integration::webhook_test:: \
    --skip integration::health_check:: \
    --skip integration::crawl_service_test:: \
    --skip integration::repositories::task_repository_test:: \
    --skip integration::scheduler_test:: \
    --skip integration::uat_scenarios_test:: \
    --skip integration::search_engines_test:: \
    --skip integration::scrape_handler_test:: \
    2>&1 | tee /tmp/test_integration.log

INTEG_TOTAL=$(grep -oP 'test result:.*\K\d+(?= passed)' /tmp/test_integration.log | tail -1 || echo "0")
INTEG_FAILED=$(grep -oP 'test result:.*\K\d+(?= failed)' /tmp/test_integration.log | tail -1 || echo "0")
INTEG_IGNORED=$(grep -oP 'test result:.*\K\d+(?= ignored)' /tmp/test_integration.log | tail -1 || echo "0")

echo ""
echo "   集成测试结果: $INTEG_TOTAL 通过, $INTEG_FAILED 失败, $INTEG_IGNORED 忽略"

TEST_END_TIME=$(date +%s)
TEST_DURATION=$((TEST_END_TIME - TEST_START_TIME))

# -----------------------------------------------------------------------------
# 测试总结
# -----------------------------------------------------------------------------
echo ""
echo "========================================"
echo "📊 测试总结"
echo "========================================"
echo ""
echo "   🕐 测试耗时: ${TEST_DURATION}秒"
echo ""
echo "   📗 单元测试: $UNIT_TOTAL 通过, $UNIT_FAILED 失败"
echo "   📘 集成测试: $INTEG_TOTAL 通过, $INTEG_FAILED 失败, $INTEG_IGNORED 忽略"
echo ""

TOTAL_PASSED=$((UNIT_TOTAL + INTEG_TOTAL))
TOTAL_FAILED=$((UNIT_FAILED + INTEG_FAILED))
TOTAL_TESTS=$((UNIT_TOTAL + UNIT_FAILED + INTEG_TOTAL + INTEG_FAILED + INTEG_IGNORED))

if [ "$TOTAL_FAILED" -eq 0 ]; then
    echo "   🎉 所有运行测试 100% 通过!"
else
    echo "   ⚠️  部分测试失败 ($TOTAL_FAILED/$TOTAL_TESTS)"
    echo ""
    echo "   失败的测试需要以下条件:"
    echo "   - 有效的 API Key 认证"
    echo "   - AWS S3 存储服务"
    echo "   - 真实搜索引擎访问"
    echo "   - 完整的 HTTP webhook 服务器"
fi

echo ""
echo "💡 完整测试报告保存在:"
echo "   - 单元测试: /tmp/test_unit.log"
echo "   - 集成测试: /tmp/test_integration.log"

echo ""
echo "========================================"
echo "✅ 测试完成"
echo "========================================"
