#!/usr/bin/env bash
# moli CDP PoC 环境脚本（moli-engine-adoption T001）：
# 检测/安装 moli 二进制（优先 PATH 已装，其次 GitHub release 按平台下载），
# 启动 `moli serve`，健康检查 CDP 端点 /json/version。全程幂等，可重复执行。
# 用法：scripts/poc/moli/setup.sh [--port PORT] [--version TAG]
# 环境变量：MOLI_POC_HOME（默认 ~/.cache/crawlrs-moli-poc，二进制/日志/pid 存放处）
set -euo pipefail

port="${MOLI_POC_PORT:-9222}"
version="${MOLI_POC_VERSION:-v1.1.21}"
home_dir="${MOLI_POC_HOME:-$HOME/.cache/crawlrs-moli-poc}"
# T002 实测：Page.captureScreenshot 需要 moli serve 带 --layout（否则显式报错 -32000）。
layout_flag=()
if [[ "${MOLI_POC_LAYOUT:-0}" == "1" ]]; then
  layout_flag=(--layout)
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) port="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    *) echo "未知参数: $1（支持 --port/--version）" >&2; exit 1 ;;
  esac
done

health_url="http://127.0.0.1:${port}/json/version"

log() { echo "[moli-poc] $*"; }

platform_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}-${arch}" in
    Darwin-arm64) echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    *) echo "不支持的构建平台: ${os}-${arch}" >&2; return 1 ;;
  esac
}

health_check() {
  curl -sf --max-time 3 "${health_url}" >/dev/null 2>&1
}

install_moli() {
  local target tar_url tmp_dir
  target="$(platform_target)"
  mkdir -p "${home_dir}/bin"
  local bin="${home_dir}/bin/moli"
  if [[ -x "${bin}" && "$("${bin}" --version 2>/dev/null || true)" == "moli ${version#v}"* ]]; then
    log "已装本地 moli: ${bin} ($("${bin}" --version))"
    return 0
  fi
  if command -v moli >/dev/null 2>&1; then
    log "PATH 已有 moli: $(command -v moli) ($(moli --version))"
    return 0
  fi
  tar_url="https://github.com/lexmount/moli/releases/download/${version}/moli-${target}.tar.gz"
  # 下载完整性校验：pin 值为 v1.1.21 官方 release 资产的 sha256（首次下载时本地校验所得）。
  # 未 pin 平台显式报错，须以 MOLI_POC_SKIP_CHECKSUM=1 显式承担风险后继续（诚实失败，不静默跳过）。
  local -A pinned_sha256=(
    # 下一行是 moli 官方 release 资产的 sha256 校验值，非密钥
    [aarch64-apple-darwin]="ecf15b7e56e576bf69e5fcfc1069e025a0734d58dd660cd4c0830c1e6e8177c5" # pragma: allowlist secret
  )
  local expected="${pinned_sha256[${target}]:-}"
  if [[ -z "${expected}" && "${MOLI_POC_SKIP_CHECKSUM:-0}" != "1" ]]; then
    echo "平台 ${target} 无 pin 校验值：请人工核对 sha256 后设 MOLI_POC_SKIP_CHECKSUM=1 重跑" >&2
    exit 1
  fi
  log "下载 ${tar_url}"
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "${tmp_dir}"' RETURN
  curl -sSL --fail --max-time 300 -o "${tmp_dir}/moli.tar.gz" "${tar_url}"
  if [[ -n "${expected}" ]]; then
    local actual
    actual="$(shasum -a 256 "${tmp_dir}/moli.tar.gz" | awk '{print $1}')"
    if [[ "${actual}" != "${expected}" ]]; then
      echo "sha256 校验失败：期望 ${expected}，实际 ${actual}" >&2
      exit 1
    fi
    log "sha256 校验通过 (${actual:0:12}…)"
  else
    log "警告：MOLI_POC_SKIP_CHECKSUM=1，跳过完整性校验"
  fi
  tar xzf "${tmp_dir}/moli.tar.gz" -C "${tmp_dir}"
  local extracted
  extracted="$(find "${tmp_dir}" -type f -name moli -perm +111 | head -1)"
  [[ -n "${extracted}" ]] || { echo "压缩包内未找到可执行 moli" >&2; exit 1; }
  cp "${extracted}" "${bin}"
  chmod +x "${bin}"
  log "安装完成: ${bin} ($("${bin}" --version))"
}

moli_bin() {
  if [[ -x "${home_dir}/bin/moli" ]]; then echo "${home_dir}/bin/moli"
  elif command -v moli >/dev/null 2>&1; then command -v moli
  else echo "" ; fi
}

install_moli

if health_check; then
  log "CDP 端点已就绪: ${health_url}（幂等跳过启动）"
  exit 0
fi

bin="$(moli_bin)"
[[ -n "${bin}" ]] || { echo "moli 二进制不可用（安装步骤失败）" >&2; exit 1; }

pid_file="${home_dir}/moli-serve-${port}.pid"
if [[ -f "${pid_file}" ]]; then
  old_pid="$(cat "${pid_file}" 2>/dev/null || true)"
  if [[ -n "${old_pid}" ]] && kill -0 "${old_pid}" 2>/dev/null; then
    log "已有 serve 进程 ${old_pid} 但健康检查未过，终止后重启"
    kill "${old_pid}" 2>/dev/null || true
    sleep 1
  fi
  rm -f "${pid_file}"
fi

mkdir -p "${home_dir}"
log_file="${home_dir}/moli-serve-${port}.log"
log "启动 moli serve（端口 ${port}，日志 ${log_file}）"
nohup "${bin}" serve --port "${port}" "${layout_flag[@]}" >"${log_file}" 2>&1 &
echo $! >"${pid_file}"

deadline=$((SECONDS + 30))
until health_check; do
  if (( SECONDS >= deadline )); then
    echo "健康检查超时（30s）：${health_url} 不可达。serve 日志尾部：" >&2
    tail -20 "${log_file}" >&2
    exit 1
  fi
  if ! kill -0 "$(cat "${pid_file}")" 2>/dev/null; then
    echo "moli serve 进程提前退出。serve 日志尾部：" >&2
    tail -20 "${log_file}" >&2
    exit 1
  fi
  sleep 0.5
done

log "CDP 端点就绪: ${health_url}"
curl -sf --max-time 3 "${health_url}" | head -c 400; echo
