#!/usr/bin/env bash
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

# =============================================================================
# crawlrs 活体 API E2E 编排
# =============================================================================
# 单命令完成：拉起测试 PostgreSQL → 应用迁移 → 构建并引导服务（含自动签发
# admin key）→ 运行状态码冒烟（api_test.sh）+ 语义级验证（api_semantics_test.py）
# → 收集审计日志 → 清理环境。
#
# 用法:
#   ./tests/e2e/live-api-e2e.sh                    # 全流程
#   ./tests/e2e/live-api-e2e.sh --skip-build       # 跳过 cargo build（已有二进制）
#   KEEP_ENV=1 ./tests/e2e/live-api-e2e.sh         # 结束后保留 DB/服务以便排查
#
# 产物（test-results/）:
#   liveapi-server.log        服务进程日志
#   liveapi-bootstrap.log     bootstrap 输出（key 已掩码）
#   api-smoke.log             api_test.sh 状态码冒烟输出
#   api-audit.jsonl           语义测试全部请求/响应审计（凭证已掩码）
#   api-semantics-matrix.md   语义覆盖矩阵
#
# 前置: Docker（或外部 TEST_DATABASE_URL 指向的 PostgreSQL 16）
# 退出码: 任一步骤失败即非零
# =============================================================================
set -euo pipefail

cd "$(dirname "$0")/../.."
ROOT="$(pwd)"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_pass() { echo -e "${GREEN}[✓]${NC} $1"; }
log_fail() { echo -e "${RED}[✗]${NC} $1"; }

SKIP_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=1 ;;
    -h|--help) grep '^#   ' "$0" | sed 's/^#   //'; exit 0 ;;
    *) log_fail "未知参数: $arg"; exit 2 ;;
  esac
done

COMPOSE_FILE="docker/docker-compose.test.yml"
SERVER_PORT="${CRAWLRS_TEST_SERVER_PORT:-8901}"
BASE_URL="http://127.0.0.1:${SERVER_PORT}"
JWT_SECRET="${CRAWLRS_TEST_JWT_SECRET:-e2e-liveapi-jwt-secret-0123456789abcdef!}"
SERVER_PID=""
WORKER_PID=""
OWN_COMPOSE=0

mkdir -p test-results
# 清理上一轮活体产物，避免审计日志混入历史结果
rm -f test-results/api-audit.jsonl test-results/api-semantics-matrix.md \
      test-results/api-smoke.log test-results/api-semantics.log \
      test-results/liveapi-server.log

cleanup() {
  for pid_name in SERVER_PID WORKER_PID; do
    pid="${!pid_name:-}"
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      log_info "停止 ${pid_name} (pid=$pid)..."
      pkill -TERM -P "$pid" 2>/dev/null || true
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
      kill -9 "$pid" 2>/dev/null || true
    fi
  done
  if command -v fuser >/dev/null 2>&1; then
    fuser -k "${SERVER_PORT}/tcp" 2>/dev/null || true
  fi
  if [ "$OWN_COMPOSE" = "1" ] && [ "${KEEP_ENV:-0}" != "1" ]; then
    log_info "清理 Docker Compose 测试环境（含卷）..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  # 清扫泄漏的 testcontainers 容器（进程被强杀时 ContainerAsync Drop 不执行会泄漏）
  local leaked
  leaked=$(docker ps -q --filter label=org.testcontainers.managed-by=testcontainers 2>/dev/null)
  if [ -n "$leaked" ]; then
    log_info "清扫泄漏的 testcontainers 容器..."
    echo "$leaked" | xargs docker rm -f >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# ── 1. 测试数据库 ────────────────────────────────────────────────────────────
if [ -z "${TEST_DATABASE_URL:-}" ]; then
  if ! docker info >/dev/null 2>&1; then
    log_fail "Docker 不可用且未设置 TEST_DATABASE_URL"; exit 1
  fi
  log_info "拉起测试 PostgreSQL (compose, 5443 端口)..."
  docker compose -f "$COMPOSE_FILE" up -d test-db >/dev/null
  OWN_COMPOSE=1
  for i in $(seq 1 30); do
    if [ "$(docker compose -f "$COMPOSE_FILE" ps --format '{{.Health}}' test-db 2>/dev/null)" = "healthy" ]; then
      break
    fi
    [ "$i" = "30" ] && { log_fail "test-db 30 次探测后仍未健康"; exit 1; }
    sleep 2
  done
  db_user=$(grep -E '^DB_USER=' docker/.env 2>/dev/null | cut -d= -f2)
  db_password=$(grep -E '^DB_PASSWORD=' docker/.env 2>/dev/null | cut -d= -f2)
  db_name=$(grep -E '^DB_NAME=' docker/.env 2>/dev/null | cut -d= -f2)
  export TEST_DATABASE_URL="postgres://${db_user:-crawlrs}:${db_password:-password}@127.0.0.1:5443/${db_name:-crawlrs}_test"
fi
log_pass "测试数据库就绪: $TEST_DATABASE_URL"

# 迁移（幂等，与 e2e-suite 同口径）
log_info "应用 migrations/*.sql..."
for f in "$ROOT"/migrations/*.sql; do
  [ -e "$f" ] || { log_fail "未找到迁移文件"; exit 1; }
  docker compose -f "$COMPOSE_FILE" exec -T test-db \
    psql -U crawlrs -d crawlrs_test -v ON_ERROR_STOP=1 -q < "$f" \
    || { log_fail "迁移应用失败: $(basename "$f")"; exit 1; }
done
log_pass "迁移已应用"

# ── 2. 构建 ──────────────────────────────────────────────────────────────────
BIN="$ROOT/target/debug/crawlrs"
if [ "$SKIP_BUILD" != "1" ]; then
  log_info "构建 crawlrs (default features, debug)..."
  cargo build --bin crawlrs || { log_fail "构建失败"; exit 1; }
fi
[ -x "$BIN" ] || { log_fail "二进制不存在: $BIN（先构建）"; exit 1; }

# ── 3. 启动服务（进程内自举 admin key）────────────────────────────────────────
# garrison DAO 为进程内 oxcache 存储（跨进程不可见），admin key 必须由 API
# 进程自身经 CRAWLRS_BOOTSTRAP_ADMIN=true 签发。run_bootstrap 仅在交互终端
# 输出完整明文 key（管道下掩码，CWE-532 设计），故经 `script` 伪 TTY 启动；
# 日志中的明文 key 在解析后立即掩码，防止审计产物泄漏凭证。
log_info "启动 crawlrs API 服务 (port=$SERVER_PORT, 进程内自举 admin key)..."
SERVER_LOG="test-results/liveapi-server.log"
: > "$SERVER_LOG"
script -qefc "env CRAWLRS__DATABASE__URL='$TEST_DATABASE_URL' \
    CRAWLRS__AUTH__JWT_SECRET='$JWT_SECRET' \
    CRAWLRS__SERVER__PORT='$SERVER_PORT' \
    CRAWLRS_BOOTSTRAP_ADMIN=true \
    RUST_LOG='${RUST_LOG:-info}' \
    '$BIN' api" /dev/null > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!

log_info "等待 /health 就绪..."
for i in $(seq 1 60); do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    log_fail "服务进程提前退出（见 $SERVER_LOG）"
    tail -30 "$SERVER_LOG"
    exit 1
  fi
  if curl -sf -o /dev/null "${BASE_URL}/health"; then
    break
  fi
  [ "$i" = "60" ] && { log_fail "服务 120s 未就绪（见 $SERVER_LOG）"; tail -30 "$SERVER_LOG"; exit 1; }
  sleep 2
done
log_pass "服务就绪: $BASE_URL"

# 抓取/爬取任务的执行由 worker 进程完成（API 进程仅入队），与 API 共享同一
# 数据库与 garrison 环境；缺失会导致任务停留在 queued。
log_info "启动 crawlrs worker 进程..."
WORKER_LOG="test-results/liveapi-worker.log"
env CRAWLRS__DATABASE__URL="$TEST_DATABASE_URL" \
    CRAWLRS__AUTH__JWT_SECRET="$JWT_SECRET" \
    RUST_LOG="${RUST_LOG:-info}" \
    "$BIN" worker > "$WORKER_LOG" 2>&1 &
WORKER_PID=$!
log_pass "worker 已启动 (pid=$WORKER_PID)"

# 从启动日志解析自举凭证（伪 TTY 为 \r\n 行尾，必须剥离 \r）
API_KEY=$(grep -oE 'api_key:[[:space:]]+[^ ]+' "$SERVER_LOG" | head -1 \
  | sed 's/^api_key:[[:space:]]*//' | tr -d '\r')
TEAM_ID=$(grep -oE 'team_id:[[:space:]]+[0-9a-f-]+' "$SERVER_LOG" | head -1 \
  | sed 's/^team_id:[[:space:]]*//' | tr -d '\r')
if [ -z "$API_KEY" ] || [ -z "$TEAM_ID" ]; then
  log_fail "未能从服务启动日志解析自举 api_key/team_id（见 $SERVER_LOG）"
  exit 1
fi
log_pass "自举完成 (team=$TEAM_ID, key=${API_KEY:0:6}…<masked>)"
# 掩码化留存服务日志（日志文件不得含明文凭证）
sed -i "s|${API_KEY}|${API_KEY:0:6}…<masked>|g" "$SERVER_LOG"

# 测试夹具：为自举团队充值积分（抓取/搜索按次扣费，种子余额会耗尽导致 402）
log_info "为自举团队充值测试积分..."
docker compose -f "$COMPOSE_FILE" exec -T test-db psql -U crawlrs -d crawlrs_test -v ON_ERROR_STOP=1 -q <<SQL
INSERT INTO credits (team_id, balance)
VALUES ('${TEAM_ID}', 100000000)
ON CONFLICT (team_id) DO UPDATE SET balance = 100000000, updated_at = NOW();
SQL
log_pass "测试积分已充值"

# ── 5. 状态码冒烟（api_test.sh）──────────────────────────────────────────────
log_info "运行状态码冒烟 api_test.sh ..."
SMOKE_RC=0
CRAWLRS_TEST_BASE_URL="$BASE_URL" \
CRAWLRS_TEST_API_KEY="$API_KEY" \
CRAWLRS_TEST_TEAM_ID="$TEAM_ID" \
  ./tests/e2e/api_test.sh 2>&1 | tee test-results/api-smoke.log || SMOKE_RC=${PIPESTATUS[0]}
if [ "$SMOKE_RC" = "0" ]; then log_pass "状态码冒烟通过"; else log_fail "状态码冒烟有失败（见 test-results/api-smoke.log）"; fi

# ── 6. 语义级验证（api_semantics_test.py）───────────────────────────────────
log_info "运行语义级验证 api_semantics_test.py ..."
SEMANTICS_RC=0
python3 tests/e2e/api_semantics_test.py \
  --base-url "$BASE_URL" \
  --api-key "$API_KEY" \
  --team-id "$TEAM_ID" \
  --audit-log test-results/api-audit.jsonl 2>&1 | tee test-results/api-semantics.log \
  || SEMANTICS_RC=${PIPESTATUS[0]}
if [ "$SEMANTICS_RC" = "0" ]; then log_pass "语义级验证全部通过"; else log_fail "语义级验证存在失败（见 test-results/api-semantics.log）"; fi

# 人工审查摘要（安全脱敏/状态转换/数据过滤取证点 + 状态机观测 + 全量结果矩阵）
python3 tests/e2e/api_audit_digest.py \
  --audit test-results/api-audit.jsonl \
  --out test-results/api-interaction-digest.md \
  || log_info "审计摘要生成失败（不影响测试结论）"

# ── 7. 汇总 ──────────────────────────────────────────────────────────────────
echo ""
if [ "$SMOKE_RC" = "0" ] && [ "$SEMANTICS_RC" = "0" ]; then
  log_pass "活体 API E2E 全部通过"
  echo "产物: test-results/{api-smoke.log, api-semantics.log, api-audit.jsonl, api-interaction-digest.md, api-semantics-matrix.md, liveapi-server.log}"
  exit 0
fi
log_fail "活体 API E2E 存在失败（smoke=$SMOKE_RC semantics=$SEMANTICS_RC）"
exit 1
