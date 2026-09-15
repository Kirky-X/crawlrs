#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Feature Combination Compile Matrix
# =============================================================================
# 编译级特性组合验证（E2E 套件 Stage 1）。逐组合执行 `cargo check`，任一失败
# 即记录并以非零码退出。组合定义与 tests/docs/feature-test-matrix.md 对齐。
#
# 使用方法:
#   ./scripts/feature-matrix.sh              # 全量组合
#   ./scripts/feature-matrix.sh --quick      # 仅关键组合（CI 门控等价集）
#   ./scripts/feature-matrix.sh --combo <n>  # 仅跑第 n 个组合（调试用）
#
# 输出: test-results/feature-matrix.log + 终端摘要
# =============================================================================

set -u

cd "$(dirname "$0")/../.."

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info()  { echo -e "${CYAN}[INFO]${NC} $1"; }
log_pass()  { echo -e "${GREEN}[PASS]${NC} $1"; }
log_fail()  { echo -e "${RED}[FAIL]${NC} $1"; }
log_section(){ echo ""; echo -e "${YELLOW}== $1 ==${NC}"; }

mkdir -p test-results
LOG="test-results/feature-matrix.log"
: > "$LOG"

QUICK=0
ONLY_COMBO=""
case "${1:-}" in
  --quick) QUICK=1 ;;
  --combo) ONLY_COMBO="${2:-}"; [ -z "$ONLY_COMBO" ] && { echo "usage: $0 --combo <n>"; exit 2; } ;;
esac

# -----------------------------------------------------------------------------
# 组合定义: name|cargo args|mode
#   mode=check        -> cargo check（lib+bins，bins 自动按 required-features 排除）
#   mode=all-targets  -> cargo check --all-targets（含 tests/benches/examples）
#
# Lib 面（--no-default-features，无服务端依赖）:
#   bare / agent-lib 及其引擎扩展 / 单提取器 / content-only
# 平台面（服务端能力）:
#   CI 7 组合（no-default/teams-only/auth-only/rate-limit-only/webhook-only/
#   default/full）+ platform 最小面 + standard + 引擎组合 + test-mocks 变体
# -----------------------------------------------------------------------------
COMBOS=(
  "bare|--no-default-features|check"
  "agent-lib|--no-default-features --features agent-lib|check"
  "agent-lib+playwright|--no-default-features --features agent-lib,engine-playwright|check"
  "agent-lib+tls-fp|--no-default-features --features agent-lib,engine-tls-fingerprint|check"
  "agent-lib+mllm|--no-default-features --features agent-lib,engine-mllm|check"
  "agent-lib+llm|--no-default-features --features agent-lib,llm|check"
  "agent-lib+metrics|--no-default-features --features agent-lib,metrics|check"
  "content-only|--no-default-features --features content|check"
  "trafilatura-only|--no-default-features --features trafilatura|check"
  "dom-smoothie-only|--no-default-features --features dom-smoothie|check"
  "teams-only|--no-default-features --features teams|check"
  "auth-only|--no-default-features --features auth|check"
  "rate-limit-only|--no-default-features --features rate-limit|check"
  "webhook-only|--no-default-features --features webhook|check"
  # platform 无驱动是非法面（lib.rs compile_error 守卫），不入矩阵
  "platform+db-sqlite|--no-default-features --features platform,db-sqlite|check"
  "platform+db-mysql|--no-default-features --features platform,db-mysql|check"
  "default|--features default|check"
  "standard|--features standard|check"
  "full|--features full|check"
  "full+tls-fp+mllm|--features full,engine-tls-fingerprint,engine-mllm|check"
  "default+test-mocks|--features default,test-mocks|all-targets"
  "full+test-mocks|--features full,test-mocks|all-targets"
)

if [ "$QUICK" = "1" ]; then
  # CI 门控等价集 + 关键新增面
  QUICK_NAMES="bare agent-lib default full platform-min full+test-mocks default+test-mocks"
  FILTERED=()
  for c in "${COMBOS[@]}"; do
    name="${c%%|*}"
    if echo " $QUICK_NAMES " | grep -q " $name "; then FILTERED+=("$c"); fi
  done
  COMBOS=("${FILTERED[@]}")
fi

log_section "Feature Combination Compile Matrix ($(date '+%F %T'))"
log_info "${#COMBOS[@]} combos; log: $LOG"

PASS=0
FAIL=0
FAILED_NAMES=()
i=0
for c in "${COMBOS[@]}"; do
  i=$((i+1))
  name="${c%%|*}"
  rest="${c#*|}"
  args="${rest%|*}"
  mode="${rest##*|}"
  if [ -n "$ONLY_COMBO" ] && [ "$ONLY_COMBO" != "$i" ] && [ "$ONLY_COMBO" != "$name" ]; then continue; fi

  extra=""
  [ "$mode" = "all-targets" ] && extra="--all-targets"

  log_info "[$i/${#COMBOS[@]}] cargo check $args $extra"
  echo "=== [$i] $name: cargo check $args $extra ===" >> "$LOG"
  if cargo check $args $extra >> "$LOG" 2>&1; then
    log_pass "$name"
    PASS=$((PASS+1))
  else
    log_fail "$name (see $LOG)"
    FAIL=$((FAIL+1))
    FAILED_NAMES+=("$name")
  fi
done

# workspace（examples 是 workspace member，单独门控）
if [ "$QUICK" != "1" ] && [ -z "$ONLY_COMBO" ]; then
  log_info "workspace examples: cargo check --workspace"
  echo "=== workspace: cargo check --workspace ===" >> "$LOG"
  if cargo check --workspace >> "$LOG" 2>&1; then
    log_pass "workspace"
    PASS=$((PASS+1))
  else
    log_fail "workspace (see $LOG)"
    FAIL=$((FAIL+1))
    FAILED_NAMES+=("workspace")
  fi
fi

log_section "Matrix Summary: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
  for n in "${FAILED_NAMES[@]}"; do log_fail "  - $n"; done
  exit 1
fi
log_pass "All feature combinations compile cleanly."
