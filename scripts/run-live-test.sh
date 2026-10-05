#!/usr/bin/env bash
# 活体栈测试运行器：确保测试用 PostgreSQL 容器在跑，注入 TEST_DATABASE_URL 后运行指定 cargo 测试目标。
# 用法：scripts/run-live-test.sh <cargo 测试目标名> [额外 cargo test 参数...]
# 说明：crawlrs 集成/压力测试目标（Cargo.toml required-features = ["full", "test-mocks"]）
#       通过 TEST_DATABASE_URL 连接测试库。dbnexus DbConfig 默认 auto_migrate=false
#       （dbnexus foundation/config/types.rs DbConfig::default），迁移由本脚本在库为空时按序应用。
set -euo pipefail

target="${1:?用法: run-live-test.sh <cargo 测试目标名> [额外 cargo test 参数...]}"
shift

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

container="crawlrs-it-pg"
port="5499"
db="crawlrs_test"
export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:${port}/${db}" # pragma: allowlist secret

if ! docker ps --format '{{.Names}}' | grep -qx "${container}"; then
  docker rm -f "${container}" 2>/dev/null || true
  docker run -d --name "${container}" -p "${port}:5432" \
    -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB="${db}" \
    postgres:16 >/dev/null
fi

# 等待 PG 就绪（最多 60s），超时显性失败
ready=0
for _ in $(seq 1 30); do
  if docker exec "${container}" pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "${ready}" -ne 1 ]; then
  echo "error: ${container} PostgreSQL 未在 60s 内就绪" >&2
  exit 1
fi

# 库为空（无 teams 表）时按序应用迁移；已迁移则跳过，重复运行幂等
if [ "$(docker exec "${container}" psql -tA -U postgres -d "${db}" -c "SELECT to_regclass('public.teams') IS NOT NULL")" != "t" ]; then
  for f in "${root}"/migrations/*.sql; do
    docker exec -i "${container}" psql -v ON_ERROR_STOP=1 -U postgres -d "${db}" -q -f - < "${f}" >/dev/null
  done
fi

# lib 不是测试目标名（隐式库目标，cargo test 只认 --lib 标志），单独分派
if [ "${target}" = "lib" ]; then
  exec cargo test --quiet --features full,test-mocks --lib -- --include-ignored "$@"
fi
exec cargo test --quiet --features full,test-mocks --test "${target}" -- --include-ignored "$@"
