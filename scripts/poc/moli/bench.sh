#!/usr/bin/env bash
# moli vs Chrome 站点对照测量（moli-engine-adoption T003）。
# 每站点跑 {moli, Chrome} 两组（chromiumoxide 同一驱动器经 CDP 建连），
# 采集 成功率 / RSS 中位（进程树合计，采样） / P95 耗时（成功请求）。
# 原始数据落 scripts/poc/moli/bench-results.csv，聚合落 bench-summary.txt。
# 用法：bash scripts/poc/moli/bench.sh   （环境变量 RUNS 控制每站点次数，默认 2）
# 站点清单：仓内 tests/docs 出现的真实目标 11 个 + 运行者补充的稳定公共站 9 个
# （任务库测试容器 tasks 表为空，生产库不可达——回退路径，偏置见 poc-report）。
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${root}"

RUNS="${RUNS:-2}"
PAGE_TIMEOUT_MS="${PAGE_TIMEOUT_MS:-15000}"
BENCH_DIR="scripts/poc/moli"
CSV="${BENCH_DIR}/bench-results.csv"
SUMMARY="${BENCH_DIR}/bench-summary.txt"
MOLI_BASE="http://127.0.0.1:9222"
CHROME_BASE="http://127.0.0.1:9333"
DRIVER="${BENCH_DIR}/connector/target/release/bench_site"

SITES=(
  https://example.com
  https://text.npr.org
  https://github.com
  https://www.rust-lang.org
  https://news.sina.com.cn
  https://www.baidu.com
  https://doc.rust-lang.org
  https://x.com
  https://httpbin.org/html
  https://tls.peet.ws/api/all
  https://keepachangelog.com
  "https://en.wikipedia.org/wiki/Rust_(programming_language)"
  https://news.ycombinator.com
  https://www.python.org
  https://go.dev
  https://www.bbc.com/news
  https://quotes.toscrape.com
  https://books.toscrape.com
  https://www.mozilla.org
  https://react.dev
)

log() { echo "[bench] $*"; }

find_chrome() {
  local candidate
  for candidate in \
    "$HOME/Library/Caches/ms-playwright"/chromium-*/chrome-mac-arm64/"Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing" \
    "$HOME/Library/Caches/ms-playwright"/chromium-*/chrome-linux/chrome \
    /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

rss_tree_kb() {
  local pid total child
  pid="$1"
  total="$(ps -o rss= -p "${pid}" 2>/dev/null | tr -d ' ' || true)"
  total="${total:-0}"
  for child in $(pgrep -P "${pid}" 2>/dev/null || true); do
    total=$(( total + $(rss_tree_kb "${child}") ))
  done
  echo "${total}"
}

start_sampler() { # $1=pid $2=outfile $3=flagfile
  (
    while [[ -f "$3" ]]; do
      rss_tree_kb "$1" >>"$2" 2>/dev/null || true
      sleep 0.5
    done
  ) &
}

[[ -x "${DRIVER}" ]] || { echo "驱动器未构建：${DRIVER}" >&2; exit 1; }

MOLI_POC_LAYOUT=1 bash "${BENCH_DIR}/setup.sh" >/dev/null
MOLI_PID="$(cat "$HOME/.cache/crawlrs-moli-poc/moli-serve-9222.pid")"
log "moli serve pid=${MOLI_PID}"

CHROME_BIN="$(find_chrome)" || { echo "未找到 Chrome 基线二进制（playwright 缓存/系统 Chrome）" >&2; exit 1; }
CHROME_PROFILE="$(mktemp -d)"
log "Chrome 基线: ${CHROME_BIN}"
nohup "${CHROME_BIN}" --headless=new --remote-debugging-port=9333 --no-first-run \
  --no-default-browser-check --user-data-dir="${CHROME_PROFILE}" about:blank \
  >/tmp/moli-bench-chrome.log 2>&1 &
CHROME_PID=$!
cleanup() {
  kill "${CHROME_PID}" 2>/dev/null || true
  # best-effort：Chrome 退出途中仍在写 profile，目录可能瞬时非空，失败不致命。
  rm -rf "${CHROME_PROFILE}" 2>/dev/null || true
  [[ -f /tmp/moli-bench-sampler-chrome.flag ]] && rm -f /tmp/moli-bench-sampler-chrome.flag
  [[ -f /tmp/moli-bench-sampler-moli.flag ]] && rm -f /tmp/moli-bench-sampler-moli.flag
}
trap cleanup EXIT

for _ in $(seq 1 40); do
  curl -sf --max-time 2 "${CHROME_BASE}/json/version" >/dev/null && break
  sleep 0.5
done
curl -sf --max-time 3 "${CHROME_BASE}/json/version" >/dev/null || { echo "Chrome CDP 未就绪" >&2; exit 1; }
log "Chrome CDP pid=${CHROME_PID}"

run_one() { # $1=base $2=url -> "ok|ms|html_len"
  local out
  out="$("${DRIVER}" "$1" "$2" "${PAGE_TIMEOUT_MS}")" || out='{"ok":false,"ms":'"${PAGE_TIMEOUT_MS}"',"html_len":0,"error":"driver-exit"}'
  python3 -c "import json,sys; d=json.loads(sys.argv[1]); print(f\"{str(d['ok']).lower()}|{d['ms']}|{d['html_len']}\")" "${out}"
}

log "warmup（各引擎 example.com 一次，丢弃）"
run_one "${MOLI_BASE}" https://example.com >/dev/null
run_one "${CHROME_BASE}" https://example.com >/dev/null

echo "kind,engine,site,run,ok,ms,html_len" >"${CSV}"
rm -f /tmp/moli-bench-rss-moli.txt /tmp/moli-bench-rss-chrome.txt
touch /tmp/moli-bench-rss-moli.txt /tmp/moli-bench-rss-chrome.txt
touch /tmp/moli-bench-sampler-moli.flag /tmp/moli-bench-sampler-chrome.flag
start_sampler "${MOLI_PID}" /tmp/moli-bench-rss-moli.txt /tmp/moli-bench-sampler-moli.flag
start_sampler "${CHROME_PID}" /tmp/moli-bench-rss-chrome.txt /tmp/moli-bench-sampler-chrome.flag

for engine in moli chrome; do
  base="${MOLI_BASE}"
  [[ "${engine}" == "chrome" ]] && base="${CHROME_BASE}"
  for site in "${SITES[@]}"; do
    for run in $(seq 1 "${RUNS}"); do
      IFS='|' read -r ok ms html_len <<<"$(run_one "${base}" "${site}")"
      echo "row,${engine},${site},${run},${ok},${ms},${html_len}" >>"${CSV}"
      log "${engine} ${site} run${run}: ok=${ok} ms=${ms} len=${html_len}"
    done
  done
done

rm -f /tmp/moli-bench-sampler-moli.flag /tmp/moli-bench-sampler-chrome.flag
sleep 1

{
  echo "# moli vs Chrome 站点对照测量聚合（runs/site=${RUNS}, sites=${#SITES[@]}, timeout=${PAGE_TIMEOUT_MS}ms）"
  echo "# 指标定义：success_rate=ok 行数/总行数；rss_median_kb=引擎进程树 RSS 采样中位（KB，warmup 后采样期）；p95_ms=成功请求耗时的 P95（nearest-rank）"
  for engine in moli chrome; do
    python3 - "${CSV}" "${engine}" /tmp/moli-bench-rss-"${engine}".txt <<'PYEOF'
import sys
csv_path, engine, rss_path = sys.argv[1], sys.argv[2], sys.argv[3]
rows = [l.strip().split(',') for l in open(csv_path) if l.startswith('row,') and l.split(',')[1] == engine]
oks = [r for r in rows if r[4] == 'true']
lat = sorted(int(r[5]) for r in oks)
def p95(xs):
    if not xs: return 'n/a'
    return str(xs[max(0, -(-len(xs)*95//100)-1)])
rss = sorted(int(x) for x in open(rss_path).read().split())
rss_med = rss[len(rss)//2] if rss else 'n/a'
rate = f"{len(oks)/len(rows):.3f}" if rows else 'n/a'
print(f"{engine}: success_rate={rate} ({len(oks)}/{len(rows)}) rss_median_kb={rss_med} p95_ms={p95(lat)}")
PYEOF
  done
} | tee "${SUMMARY}"
log "完成：${CSV} 与 ${SUMMARY}"
