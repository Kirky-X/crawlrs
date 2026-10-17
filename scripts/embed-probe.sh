#!/usr/bin/env bash
# Copyright (c) 2025-2026 Kirky.X🌠
# SPDX-License-Identifier: Apache-2.0

# =============================================================================
# crawlrs 嵌入编译探针门禁 (embed-probe)
# =============================================================================
# 目的：把「crawlrs 能否作为库被嵌入」的可行性从一次性探针记录固化为可重复
#       执行的仓库门禁。fluxgate 等下游以 path 依赖嵌入 crawlrs 时，cargo 会把
#       nexus 家族依赖统一到最高版本；本探针在临时目录复现该路径，确保嵌入编译
#       面（feature 组合 + 家族依赖版本）在 crawlrs 仓内即可回归，避免「单仓绿、
#       嵌入红」在上游升级时复发。
#
# 机制：在 mktemp 临时目录 `cargo new` 一个探针 crate，`cargo add --path` 以路径
#       依赖引入本仓并按受支持的嵌入 feature 组合声明，随后 `cargo check`。探针
#       构建产物落在临时 target，不触碰仓库工作区；EXIT 清理保证幂等。
#
# 嵌入 feature 组合（与 README「作为库依赖」节声明严格一致）：
#       db-postgres + extractors + llm，引擎默认集随 crawlrs default features
#       （platform + db-postgres），不显式关闭 default。
#
# 用法：
#   scripts/embed-probe.sh              # 自动定位仓库根（由脚本自身位置推导）
#   scripts/embed-probe.sh <repo-root>   # 显式指定仓库根（跨机器/CI 可执行）
#
# 退出码：
#   0  嵌入编译面完好
#   1  失败；stderr 打印失败步骤名（root / new / add / check）
# =============================================================================

set -euo pipefail

# -----------------------------------------------------------------------------
# 仓库根定位：优先取参数 $1，否则由脚本自身位置回推（不硬编码绝对路径）
# -----------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${1:-$(cd "$SCRIPT_DIR/.." && pwd)}"

fail() {
    # $1 = 失败步骤名，$2 = 详细说明
    echo "FAIL: step=$1 — $2" >&2
    exit 1
}

# 步骤 0：校验仓库根为有效 cargo 包（含 Cargo.toml 且声明 crawlrs）
[ -f "$REPO_ROOT/Cargo.toml" ] || fail "root" "仓库根缺 Cargo.toml: $REPO_ROOT"

# 嵌入 feature 组合：与 README 声明一致，逗号分隔单值（cargo add 确定支持）
readonly PROBE_FEATURES="db-postgres,extractors,llm"

# -----------------------------------------------------------------------------
# 临时探针目录：mktemp 独立于仓库，EXIT 陷阱清理，保证幂等且不污染工作区
# -----------------------------------------------------------------------------
PROBE_DIR="$(mktemp -d)"
cleanup() { rm -rf "$PROBE_DIR"; }
trap cleanup EXIT

# 步骤 1：新建探针 crate（bin 目标；作为依赖声明即触发 crawlrs lib 编译检查）
( cd "$PROBE_DIR" && cargo new probe --vcs none ) \
    || fail "new" "cargo new 探针 crate 失败"

# 步骤 2：以 path 依赖引入本仓，按嵌入 feature 组合声明
( cd "$PROBE_DIR/probe" && cargo add crawlrs --path "$REPO_ROOT" --features "$PROBE_FEATURES" ) \
    || fail "add" "cargo add path 依赖失败（feature 组合: $PROBE_FEATURES）"

# 步骤 3：cargo check —— 连带编译校验 crawlrs 嵌入编译面（探针自身代码不使用 crawlrs，
# 但 cargo 对声明的依赖恒执行 check，feature 组合按步骤 2 生效）
( cd "$PROBE_DIR/probe" && cargo check ) \
    || fail "check" "cargo check 嵌入编译失败（feature 组合: $PROBE_FEATURES + default 引擎集）"

echo "PASS: embed-probe — crawlrs 嵌入编译面完好（仓库根: $REPO_ROOT；feature 组合: $PROBE_FEATURES + default 引擎集）"
