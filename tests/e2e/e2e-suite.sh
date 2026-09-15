#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Crawlrs E2E 质量保障套件
# =============================================================================
# 特性组合测试与全量验证的单一入口。阶段划分与 tests/docs/feature-test-matrix.md
# 对齐：
#
#   Stage 1  matrix       特性组合编译矩阵（tests/e2e/feature-matrix.sh）
#   Stage 2  static       fmt / clippy(default|full|agent-lib) / cargo deny
#   Stage 3  unit         lib 测试(default|full) + mock 测试(main/sdk/route_diag)
#   Stage 4  integration  集成测试（PostgreSQL；testcontainers 自动拉起或外部
#                         TEST_DATABASE_URL，自动应用 migrations/*.sql）
#   Stage 5  bench        基准测试（首次建立基线，之后对比 e2e-baseline）
#   Stage 6  report       汇总 test-results/e2e-report.txt
#
# 使用方法:
#   ./scripts/e2e-suite.sh                  # 全量
#   ./scripts/e2e-suite.sh static unit      # 只跑指定阶段
#   ./scripts/e2e-suite.sh --skip matrix    # 跳过指定阶段
#   ./scripts/e2e-suite.sh --quick          # 快速矩阵（关键组合）
#
# 中间件：
#   - 集成测试仅需 PostgreSQL 16。缺省由 testcontainers 动态拉起并自动清理；
#     设 TEST_DATABASE_URL 时改用外部实例（可用 docker/docker-compose.test.yml），
#     套件自动应用 migrations/*.sql 并在结束时 down -v 清理。
#   - Redis / MySQL / Keycloak OIDC：代码库无集成，无服务轴（见矩阵文档 §1.3）。
#
# 退出码：任一阶段失败即非零。日志：test-results/*.log
# =============================================================================

set -u

cd "$(dirname "$0")/../.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info()    { echo -e "${CYAN}[INFO]${NC} $1"; }
log_pass()    { echo -e "${GREEN}[✓]${NC} $1"; }
log_fail()    { echo -e "${RED}[✗]${NC} $1"; }
log_section() { echo ""; echo -e "${YELLOW}======== $1 ========"; }

mkdir -p test-results

# --- 参数解析 ---------------------------------------------------------------
STAGES_ALL="matrix static unit integration bench report"
RUN_STAGES=""
SKIP_STAGES=""
QUICK=0
while [ $# -gt 0 ]; do
  case "$1" in
    --quick) QUICK=1 ;;
    --skip) shift; SKIP_STAGES="$1" ;;
    -h|--help) grep '^#   ' "$0" | sed 's/^#   //'; exit 0 ;;
    *) RUN_STAGES="$RUN_STAGES $1" ;;
  esac
  shift
done
[ -z "$RUN_STAGES" ] && RUN_STAGES="$STAGES_ALL"

want_stage() {
  local s="$1"
  echo " $RUN_STAGES " | grep -q " $s " || return 1
  if [ -n "$SKIP_STAGES" ] && echo " $SKIP_STAGES " | grep -q " $s "; then return 1; fi
  return 0
}

# --- 结果记账 ---------------------------------------------------------------
declare -A STAGE_STATUS
FAILED_STAGES=()

record() { # record <stage> <exit_code>
  if [ "$2" -eq 0 ]; then
    STAGE_STATUS["$1"]="PASS"; log_pass "Stage [$1] 通过"
  else
    STAGE_STATUS["$1"]="FAIL"; FAILED_STAGES+=("$1"); log_fail "Stage [$1] 失败"
  fi
}

# --- 外部 PostgreSQL 生命周期 ------------------------------------------------
COMPOSE_FILE="docker/docker-compose.test.yml"
COMPOSE_STARTED=0
EXTERNAL_DB=0
DB_MIGRATED=0
if [ -n "${TEST_DATABASE_URL:-}" ]; then EXTERNAL_DB=1; fi

cleanup() {
  if [ "$COMPOSE_STARTED" = "1" ]; then
    log_info "清理 Docker Compose 测试环境（含卷）..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  # 清扫泄漏的 testcontainers 容器（测试被强杀时 ContainerAsync Drop 不执行会泄漏）
  local leaked
  leaked=$(docker ps -q --filter label=org.testcontainers.managed-by=testcontainers 2>/dev/null)
  if [ -n "$leaked" ]; then
    log_info "清扫泄漏的 testcontainers 容器..."
    echo "$leaked" | xargs docker rm -f >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# 单阶段超时护栏（秒）。偶发的并行测试挂起不应阻塞整条流水线。
E2E_STAGE_TIMEOUT="${E2E_STAGE_TIMEOUT:-1800}"

ensure_db() {
  # 幂等：lib 测试（含 DB 仓库用例）与集成测试共用一个已迁移的 PostgreSQL。
  # 优先外部 TEST_DATABASE_URL；否则 compose 拉起（Docker 不可用时回落 testcontainers）。
  if [ "$DB_MIGRATED" = "1" ]; then return 0; fi
  if [ "$EXTERNAL_DB" != "1" ]; then
    if docker info >/dev/null 2>&1; then
      start_external_db || return 1
    else
      log_info "Docker 不可用，交由 testcontainers 动态拉起（仅覆盖 tc_ 用例，DB 仓库用例将失败）"
      return 1
    fi
  fi
  apply_migrations || return 1
  DB_MIGRATED=1
}

start_external_db() {
  log_info "通过 Docker Compose 拉起测试 PostgreSQL（5443 端口）..."
  docker compose -f "$COMPOSE_FILE" up -d test-db || return 1
  COMPOSE_STARTED=1
  log_info "等待 test-db 健康..."
  for i in $(seq 1 30); do
    if [ "$(docker compose -f "$COMPOSE_FILE" ps --format '{{.Health}}' test-db 2>/dev/null)" = "healthy" ]; then
      # 凭据与 docker/.env 对齐（compose 以 ${DB_PASSWORD:-password} 初始化，
      # .env 覆盖默认值时必须使用覆盖值，否则 TCP 密码认证 28P01）
      local db_user db_password db_name
      db_user=$(grep -E '^DB_USER=' docker/.env 2>/dev/null | cut -d= -f2)
      db_password=$(grep -E '^DB_PASSWORD=' docker/.env 2>/dev/null | cut -d= -f2)
      db_name=$(grep -E '^DB_NAME=' docker/.env 2>/dev/null | cut -d= -f2)
      export TEST_DATABASE_URL="postgres://${db_user:-crawlrs}:${db_password:-password}@127.0.0.1:5443/${db_name:-crawlrs}_test"
      log_pass "test-db 健康: $TEST_DATABASE_URL"
      return 0
    fi
    sleep 2
  done
  log_fail "test-db 30 次探测后仍未健康"
  return 1
}

apply_migrations() {
  # 外部库需手动应用迁移（testcontainers 路径由 src/common/test_helpers.rs 自动应用）
  log_info "应用 migrations/*.sql 到外部测试库..."
  for f in "$ROOT"/migrations/*.sql; do
    [ -e "$f" ] || { log_fail "未找到迁移文件: $f"; return 1; }
    if ! docker compose -f "$COMPOSE_FILE" exec -T test-db \
        psql -U "${DB_USER:-crawlrs}" -d "${DB_NAME:-crawlrs}_test" -v ON_ERROR_STOP=1 -q < "$f"; then
      log_fail "迁移应用失败: $(basename "$f")"
      return 1
    fi
  done
  log_pass "全部迁移已应用"
}

# =============================================================================
# Stage 1: 特性组合编译矩阵
# =============================================================================
stage_matrix() {
  log_section "Stage 1: Feature Compile Matrix"
  local args=""
  [ "$QUICK" = "1" ] && args="--quick"
  ./tests/e2e/feature-matrix.sh $args 2>&1 | tee test-results/stage1-matrix.log | grep -E '\[(PASS|FAIL)\]'
  record matrix "${PIPESTATUS[0]}"
}

# =============================================================================
# Stage 2: 静态检查
# =============================================================================
stage_static() {
  log_section "Stage 2: Static Analysis"
  local rc=0

  log_info "cargo fmt -p crawlrs -p crawlrs-examples -- --check"
  # 显式限定本项目包：cargo fmt --all 在 cargo 1.97 会波及 path 依赖
  # （如兄弟项目 garrison 的实时编辑），非本仓库门禁范围
  if cargo fmt -p crawlrs -p crawlrs-examples -- --check > test-results/stage2-fmt.log 2>&1; then
    log_pass "fmt"
  else
    log_fail "fmt（见 test-results/stage2-fmt.log）"
    head -20 test-results/stage2-fmt.log; rc=1;
  fi

  for f in default full agent-lib; do
    log_info "cargo clippy --features $f --all-targets -- -D warnings"
    if cargo clippy --features "$f" --all-targets -- -D warnings \
        > "test-results/stage2-clippy-$f.log" 2>&1; then log_pass "clippy:$f"; else
      log_fail "clippy:$f（见 test-results/stage2-clippy-$f.log）"; rc=1; fi
  done

  log_info "cargo deny check"
  if cargo deny check > test-results/stage2-deny.log 2>&1; then log_pass "deny"; else
    log_fail "deny（见 test-results/stage2-deny.log）"; tail -30 test-results/stage2-deny.log; rc=1; fi

  record static "$rc"
}

# =============================================================================
# Stage 3: 单元 + mock 测试
# =============================================================================
run_cargo_test() { # run_cargo_test <logname> <args...>
  local logname="$1"; shift
  log_info "cargo test $*"
  if timeout "$E2E_STAGE_TIMEOUT" cargo test "$@" > "test-results/stage3-$logname.log" 2>&1; then
    log_pass "$logname ($(grep -oE '[0-9]+ passed' "test-results/stage3-$logname.log" | tail -1))"
  else
    local rc=$?
    if [ "$rc" = "124" ]; then
      log_fail "$logname 超时（${E2E_STAGE_TIMEOUT}s，疑似并行挂起）"
    else
      log_fail "$logname（见 test-results/stage3-$logname.log）"
    fi
    grep -E "FAILED|panicked|error\[" "test-results/stage3-$logname.log" | head -10
    return 1
  fi
}

stage_unit() {
  log_section "Stage 3: Unit & Mock Tests"
  local rc=0
  # lib 测试包含真实 DB 的仓库用例，需先就绪数据库（失败不阻断 mock 用例）
  if ! ensure_db; then
    log_fail "数据库就绪失败——lib 中的 DB 仓库用例将失败（TEST_DATABASE_URL 未设置且 compose 不可用）"
    rc=1
  fi
  run_cargo_test lib-default --features default --lib || rc=1
  run_cargo_test lib-full --features full --lib || rc=1
  run_cargo_test mock-main --features full,test-mocks --test main || rc=1
  run_cargo_test sdk-api --features test-mocks --test sdk_api_test || rc=1
  run_cargo_test route-diag --features platform --test route_diag_test || rc=1
  record unit "$rc"
}

# =============================================================================
# Stage 4: 集成测试（真实 PostgreSQL）
# =============================================================================
stage_integration() {
  log_section "Stage 4: Integration Tests (PostgreSQL)"
  local rc=0

  if ! ensure_db; then
    log_fail "无法准备 PostgreSQL（compose 失败且无外部 TEST_DATABASE_URL）"
    record integration 1; return
  fi

  # 仓库集成测试（非 ignore）+ 认证端到端（ignore 标记，--include-ignored 激活；
  # wreq 指纹用例在 full 特性下不编译，不受影响）
  run_cargo_test integration --features full,test-mocks --test integration_tests -- --include-ignored --test-threads 4 || rc=1
  record integration "$rc"
}

# =============================================================================
# Stage 5: 基准测试
# =============================================================================
stage_bench() {
  log_section "Stage 5: Benchmarks"
  mkdir -p test-results/bench
  # 缩短采样保证套件时效；--save-baseline 建立基线后，后续运行 --baseline 对比回退
  local bench_args=(--warm-up-time 1 --measurement-time 3 --sample-size 10)
  # --bench benchmark：只跑 criterion 目标（lib unittests 的 libtest bench 不接受 criterion 参数）
  if ls target/criterion/e2e-baseline >/dev/null 2>&1; then
    log_info "已有基线，运行并对比 e2e-baseline"
    timeout "$E2E_STAGE_TIMEOUT" cargo bench --bench benchmark -- \
      "${bench_args[@]}" --baseline e2e-baseline 2>&1 | tee test-results/stage5-bench.log | tail -30
  else
    log_info "首次运行，建立 e2e-baseline 基线"
    timeout "$E2E_STAGE_TIMEOUT" cargo bench --bench benchmark -- \
      "${bench_args[@]}" --save-baseline e2e-baseline 2>&1 | tee test-results/stage5-bench.log | tail -30
  fi
  record bench "${PIPESTATUS[0]}"
}

# =============================================================================
# Stage 6: 汇总报告
# =============================================================================
stage_report() {
  log_section "Stage 6: Summary Report"
  local report="test-results/e2e-report.txt"
  {
    echo "Crawlrs E2E Suite Report — $(date '+%F %T')"
    echo "工作区: $ROOT (branch: $(git branch --show-current 2>/dev/null || echo n/a))"
    echo ""
    for s in $STAGES_ALL; do
      if want_stage "$s"; then
        printf '  %-12s %s\n' "$s" "${STAGE_STATUS[$s]:-SKIPPED}"
      else
        printf '  %-12s %s\n' "$s" "not-selected"
      fi
    done
    echo ""
    echo "日志: test-results/（stage*.log, feature-matrix.log, bench/）"
  } | tee "$report"

  if [ "${#FAILED_STAGES[@]}" -gt 0 ]; then
    log_fail "失败阶段: ${FAILED_STAGES[*]}"
    exit 1
  fi
  log_pass "E2E 套件全部通过"
}

# =============================================================================
# 主流程
# =============================================================================
for s in $STAGES_ALL; do
  if want_stage "$s"; then
    "stage_$s"
  fi
done

# report 始终生成（即使未选择，也输出最终状态）
[ -f test-results/e2e-report.txt ] || stage_report
if [ "${#FAILED_STAGES[@]}" -gt 0 ]; then exit 1; fi
exit 0
