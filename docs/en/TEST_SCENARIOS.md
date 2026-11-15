# 🧪 Crawlrs Test Scenario Matrix

> Applicable version: crawlrs **0.2.0** (workspace, `Cargo.toml` `rust-version = "1.97.1"`, edition 2021)
> Data sources: `Cargo.toml`, `.github/workflows/ci.yml`, `tests/`, `tests/docs/feature-test-matrix.md` (calibrated 2026-09-14), and the actual code of `benches/benchmark.rs`.

## 📋 Table of Contents

- [🎯 Test System Overview](#-test-system-overview)
- [🗂️ Suite Inventory](#️-suite-inventory)
- [✅ Unit Test Scenarios](#-unit-test-scenarios)
- [🔌 Integration and End-to-End Scenarios](#-integration-and-end-to-end-scenarios)
- [🧩 Feature Combination Matrix](#-feature-combination-matrix)
- [▶️ Runbook](#️-runbook)
- [📊 Statistics Summary](#-statistics-summary)

---

## 🎯 Test System Overview

| Layer | Location | Middleware requirement | Notes |
|---|---|---|---|
| L1 inline unit tests | `src/` (`#[cfg(test)]` modules) | none | covers domain / application / engines / infrastructure and all other modules |
| L2 module unit tests | `tests/unit/` (70 files) | none (wiremock fake services) | organized by presentation / application / domain / engines / search / workers / queue / config / di / utils domains |
| L3 table-driven harness | `tests/main.rs` (`[[test]]` name=`main`) | none (trait fakes) | gated by `test-mocks`; shared mocks in `tests/common/mocks/` |
| L4 SDK API tests | `tests/sdk_api_test.rs` | none | gated by `test-mocks` |
| L5 route diagnostics | `tests/route_diag_test.rs` | none | gated by `platform`; verifies route registration matches gating |
| L6 integration tests | `tests/integration/mod.rs` (`[[test]]` name=`integration_tests`) | PostgreSQL 16 (auto-started by testcontainers, or external `TEST_DATABASE_URL`) | gated by `full,test-mocks`; includes the repository layer and garrison auth end-to-end |
| L7 quality assurance suite | `tests/e2e/`, `tests/python/`, `tests/stress/`, `benches/` | varies by stage | E2E orchestration, Python API/performance tests, k6 load scripts, Criterion benchmarks |

**Hard environment constraints** (consistent with `tests/docs/feature-test-matrix.md` §1.3):

- Middleware needs only **PostgreSQL 16**; Chrome / FlareSolverr are optional browser profiles.
- The Redis shared cache is an optional backend (`cache-redis` feature + `[cache] backend=redis`, default `memory`); the test-service axis still does not depend on a Redis instance.
- Migration DDLs are provisioned per dialect: `migrations/{postgres,sqlite,mysql}/` three DDL sets (consistency guarded by `tests/migration_dialect.rs`); `db-sqlite` / `db-mysql` cover the runtime schema provisioning surface (`[database] auto_migrate`).

---

## 🗂️ Suite Inventory

| Suite | Entry point | Feature requirement | Trigger environment |
|---|---|---|---|
| lib unit tests (standard) | `cargo test --features "standard" --lib` | default (=platform+db-postgres) | CI test job / local |
| lib unit tests (full) | `cargo test --features "full" --lib` | full | CI test job / local |
| table-driven harness | `cargo test --test main` | test-mocks | CI integration-test job (`--tests`) / e2e Stage 3 |
| SDK API | `cargo test --test sdk_api_test` | test-mocks | same as above |
| route diagnostics | `cargo test --test route_diag_test` | platform | same as above |
| integration tests | `cargo test --test integration_tests` | full,test-mocks | CI integration-test job / e2e Stage 4 (`--include-ignored`) |
| auth end-to-end | `cargo test --test integration_tests -- --ignored auth_garrison_test` | full | needs real PostgreSQL |
| TLS fingerprint engine | `cargo test --test integration_tests -- --ignored wreq_fingerprint_test` | full,engine-tls-fingerprint | needs internet egress |
| Python API/performance | `./scripts/run-tests.sh local` | a running service instance | local / manual |
| E2E API smoke | `tests/e2e/api_test.sh` | a running service instance | manual / automatic in the liveapi stage |
| API semantic verification | `tests/e2e/api_semantics_test.py` | a running service instance | automatic in the liveapi stage / manual (45 `scenarios.append` scenarios) |
| live API E2E orchestration | `tests/e2e/live-api-e2e.sh` | Docker (compose PG) | manual / liveapi stage |
| E2E quality suite | `./tests/e2e/e2e-suite.sh` | all | local / manual, 7 stages |
| feature compile matrix | `tests/e2e/feature-matrix.sh` | all | e2e Stage 1 |
| manual real-crawl verification | `tests/manual/test_real_crawl.py` | a running service instance | manual |
| load test | `tests/stress/k6_script.js` | a running service instance | `k6 run tests/stress/k6_script.js` |
| benchmarks | `cargo bench` | default | no dedicated CI job; short run in e2e Stage 5 |

Shared test infrastructure lives in `tests/common/`: `factories/` (data factories), `fixtures/`, `helpers/`, `macros/`, `mocks/` (MockTaskRepository / MockScraperEngine / MockWebhookService etc.), `assertions/`, `constants/`. The project has no mock library — mocks are real trait implementations + `Arc<AtomicU32>` call counters.

---

## ✅ Unit Test Scenarios

### 📌 Normal paths

| Domain | Coverage location | Representative scenarios |
|---|---|---|
| config | `tests/unit/config/settings_test.rs` | confers config loading, defaults, `CRAWLRS__` environment-variable overrides |
| search | `tests/unit/search/` (baidu / bing / sogou / aggregator / client / router) | engine parsing, result aggregation, dedup, routing |
| presentation | `tests/unit/presentation/` (handlers / middleware / helpers / sdk / state) | per-handler request validation and responses, rate-limit and security-header middleware, SSRF pre-validation (`ssrf_mod_test` / `ssrf_redirect_test`), synchronous waiting (`sync_wait_*_test`) |
| workers and queue | `tests/unit/workers/`, `tests/unit/queue/` | task claim/complete/failure paths, worker manager lifecycle, priority queue |
| engines | `tests/unit/engines/` (engine_client / reqwest / playwright / browser_downloader) | engine selection, support_score routing, error propagation |
| domain and application | `tests/unit/domain/`, `tests/unit/application/` | entity rules, DTO validation |
| infrastructure | `tests/unit/infrastructure/` | database connection (dbnexus), repository-layer logic |
| DI / bootstrap | `tests/unit/di/module_test.rs`, `tests/unit/bootstrap/routes_test.rs` | module assembly, route registration |
| utils | `tests/unit/utils/` (telemetry / encoding / processor) | telemetry, encoding detection, content processing |
| i18n | `tests/unit/i18n/` | Fluent message loading |

### ⚠️ Errors and boundaries (built into the test targets above)

| Dimension | Coverage location |
|---|---|
| network timeouts/failures | `tests/unit` (wiremock fake services, engine error paths), `tests/common/mocks` |
| DB connection failures | `tests/unit/infrastructure/database/dbnexus_connection_test.rs` (invalid URL/exhausted retries), `repositories/*_test.rs` error paths |
| token forgery/auth bypass | `tests/integration/auth_garrison_test.rs`, `tests/unit/presentation` (middleware) |
| concurrency races | `tests/unit/queue`, `tests/unit/workers` (AtomicU32 call counters, tokio test-util) |
| configuration errors | `tests/unit/config`, confers validation |
| SSRF protection | `tests/unit` (security), handler pre-validation + engine-routing dual validation |

---

## 🔌 Integration and End-to-End Scenarios

### 🏬 Repository-layer integration (real PostgreSQL)

`tests/integration/repositories/`: `tasks_backlog_repo_test`, `scrape_result_repo_test`, `webhook_repo_test`, `credits_repo_test` — covering CRUD, batch writes, and error paths. The test app is built by `tests/integration/helpers/test_app.rs` (real routes + the middleware stack).

### 🔐 Authentication end-to-end (`auth_garrison_test`, ignored)

The full garrison authentication chain: API Key issuance and verification, RBAC scope mapping, brute-force lockout (401/429 semantics), multi-tenant isolation. The garrison global singleton is reset via the `test-mocks`-gated `reset_garrison_global_state_for_integration_test()` and serialized with `GARRISON_TEST_LOCK`.

### 🧬 TLS fingerprint engine (`wreq_fingerprint_test`, ignored)

WreqEngine verifies JA3/JA4 fingerprint spoofing against the network (requires the `engine-tls-fingerprint` feature).

### 🌐 Python end-to-end (`tests/python/`)

| Suite | Scenarios |
|---|---|
| `test_api_endpoints.py` | smoke and contract tests for all REST endpoints |
| `test_error_handling.py` | error codes and error responses |
| `test_performance.py` | response-time and throughput baseline |
| `test_search_engines.py` / `test_search_comprehensive.py` | four-engine search behavior |
| `conftest.py` + `api_test_framework.py` | shared fixtures and the HTTP client wrapper |

### 🏋️ Load tests and manual

- `tests/stress/k6_script.js`: k6 load scenarios.
- `tests/manual/test_real_crawl.py`: real-site scraping regression (needs internet egress).
- `tests/e2e/api_test.sh`: service-level API smoke script.

---

## 🧩 Feature Combination Matrix

### 🤖 CI feature-matrix job (7 combinations, required on every PR)

| Combination | cargo arguments | Verification goal | garrison expectation |
|------|------|----------|----------|
| no-default | `cargo check --no-default-features --lib` | all gates in place, no business capabilities + no engines | absent |
| teams-only | `--no-default-features --features teams` | teams→auth→garrison closure | present |
| auth-only | `--no-default-features --features auth` | auth only | present |
| rate-limit-only | `--no-default-features --features rate-limit` | rate limiting only | absent |
| webhook-only | `--no-default-features --features webhook` | webhook only | absent |
| default | `--features default` | the default release surface | present |
| full | `--features full` | full functionality | present |

Each combination additionally runs a `cargo tree` assertion that garrison enters the dependency tree only when auth is on (R-auth-engine-005); auth-on combinations also assert that `src/` contains no RSA code paths (RS256/RS384/RS512/`use rsa::`), keeping the precondition for `deny.toml`'s RUSTSEC-2023-0071 ignore valid.

### 🧪 E2E compile matrix (Stage 1, 22 combinations)

`tests/e2e/feature-matrix.sh` executes per `tests/docs/feature-test-matrix.md` §2; key combinations:

| # | Combination | cargo arguments | Coverage intent |
|---|---|---|---|
| 1 | bare | `--no-default-features` | empty-feature compilation sanity |
| 2-7 | agent-lib ± engines/llm/metrics | `--no-default-features --features agent-lib,...` | the minimal embedded library surface and engine gating |
| 8-10 | content / trafilatura / dom-smoothie individually | same as above | content processing and extractor gating |
| 11-14 | teams / auth / rate-limit / webhook individually | same as above | business capability closures |
| 15-16 | platform+db-sqlite / platform+db-mysql | same as above | the other two database driver surfaces |
| 17-19 | default / standard / full(+tls-fp+mllm) | `--features ...` | the release surface and the full engine closure |
| 20-21 | default+test-mocks / full+test-mocks (`--all-targets`) | same as above | all-test-target compilation |

### 🔒 Hard constraints

- The `crawlrs` binary requires `platform`; without a platform-surface feature the bin is automatically excluded.
- `platform` must pair with exactly one `db-*`; embedded (sqlite) and server-side (postgres/mysql) cannot mix, and postgres/mysql are mutually exclusive (enforced at compile time by dbnexus + the `src/lib.rs` compile_error guard).
- `teams` implies `auth`, and `auth` implies `dep:garrison`.
- `integration_tests` requires `full,test-mocks`; `main` / `sdk_api_test` require `test-mocks`; `route_diag_test` requires `platform`.

---

## ▶️ Runbook

### 🔁 Commands identical to CI

```bash
# Unit tests (CI test job; needs PostgreSQL 16 + migrations/postgres/*.sql, started automatically by testcontainers locally)
cargo test --features "standard" --lib
cargo test --features "full" --lib

# Integration + all test targets (CI integration-test job)
cargo test --features "full,test-mocks" --tests --no-fail-fast

# Lint / format / docs / supply chain
cargo fmt -p crawlrs -p crawlrs-examples -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings
cargo doc --features "full" --no-deps          # CI doc job (RUSTDOCFLAGS=-D warnings)
cargo deny check

# Coverage gate: line coverage ≥ 90%
cargo llvm-cov --features "full" --lib --fail-under-lines 90
```

CI's test / integration-test / coverage jobs all assume a `postgres:16` service container: first run `for f in migrations/postgres/*.sql; do psql ... -f "$f"; done` to apply migrations, then run the tests; `DATABASE_URL` / `TEST_DATABASE_URL` point to `postgres://postgres:postgres@localhost:5432/crawlrs_test`. The test job's toolchain matrix is Rust 1.97.1 / stable (see `ci.yml`).

### 🎼 E2E suite (7 stages)

```bash
./tests/e2e/e2e-suite.sh                    # full run (matrix+static+tests+integration+bench+live API+report)
./tests/e2e/e2e-suite.sh static unit        # only run specified stages
./tests/e2e/e2e-suite.sh --skip matrix      # skip slow stages
./tests/e2e/e2e-suite.sh --quick            # quick matrix (liveapi skipped automatically)
TEST_DATABASE_URL=postgres://... ./tests/e2e/e2e-suite.sh   # reuse an external PG (migrations applied automatically)
```

| Stage | Content |
|---|---|
| Stage 1 matrix | feature compile matrix (22 combinations) |
| Stage 2 static | `cargo fmt --check` → clippy (default/full/agent-lib, `--all-targets -D warnings`) → `cargo deny check` |
| Stage 3 unit | `--lib` (default/full), `--test main`, `--test sdk_api_test`, `--test route_diag_test` |
| Stage 4 integration | `--test integration_tests --include-ignored` (including the garrison auth end-to-end) |
| Stage 5 bench | `cargo bench` with shortened sampling; the first run establishes `e2e-baseline`, and later runs compare automatically to detect regressions |
| Stage 6 liveapi | `tests/e2e/live-api-e2e.sh`: compose starts PG → migrate → build → start `crawlrs api` via a `script` pseudo-TTY (env `CRAWLRS_BOOTSTRAP_ADMIN=true` + the `CRAWLRS__WEBHOOK__SECRET` test secret; the API process itself calls `run_bootstrap` at startup to issue the admin key; after building, the `crawlrs migrate` subcommand's idempotency is verified first; the garrison DAO is in-process oxcache, so keys issued by another process are invisible; the plaintext key is masked immediately after parsing) → wait for `/health` readiness (port 8901) → status-code smoke (`api_test.sh`, including `/api/v1/sdk/*` real-path assertions) + semantic verification (`api_semantics_test.py`, 45 `scenarios.append` scenarios: response-envelope consistency/security redaction/state transitions/data filtering/concurrency/SDK semantics/the cancel family/apikey expiry/webhook signature creation) → the `crawlrs migrate` subcommand idempotent double-run verification → the 402 paywall gate (credits zeroed → `POST /v1/scrape` asserts 402 `QUOTA_EXCEEDED`) → the 429 rate-limit gate (80×SSRF-rejected request burst → assert 429 + rate-limit error signal; worker 64 + team semaphore 64 are the suite's concurrency prerequisites) → artifacts `test-results/{api-smoke.log, api-semantics.log, api-audit.jsonl, api-interaction-digest.md, api-semantics-matrix.md, api-quota-402.json, liveapi-server.log}` |
| Stage 7 report | aggregates `test-results/e2e-report.txt`; the EXIT trap cleans up compose containers/volumes and leaked testcontainers containers |

Any stage failure exits non-zero; logs concentrate in `test-results/`; a per-stage timeout guard `E2E_STAGE_TIMEOUT` (default 1800s).

Running the live API E2E on its own:

```bash
./tests/e2e/live-api-e2e.sh                 # full flow (DB→bootstrap→start→smoke+semantics→cleanup)
./tests/e2e/live-api-e2e.sh --skip-build    # skip cargo build
KEEP_ENV=1 ./tests/e2e/live-api-e2e.sh      # keep DB/service after the run for troubleshooting
# With a service already running, run the semantic verification directly with credentials:
python3 tests/e2e/api_semantics_test.py --base-url http://localhost:8899 \
    --api-key "$KEY" --team-id "$TEAM_ID" --audit-log test-results/api-audit.jsonl
```

### 🐳 Environment and cleanup

- Prefer testcontainers (automatic, zero residue; when Docker is unreachable the relevant cases auto-skip; CI additionally has the `skip_if_no_test_db` guard).
- For external environments use `docker/docker-compose.test.yml`: `docker compose -f docker/docker-compose.test.yml up -d test-db` → tests → `down -v` (including volume cleanup).
- Python tests: `./scripts/run-tests.sh local` (auto `pip install -r tests/python/requirements.txt`, results written to `test-results/`).

---

## 📊 Statistics Summary

As of 0.2.0 (counted via `grep -rE '^\s*#\[(tokio::)?test\]'`):

| Metric | Count |
|---|---|
| `src/` inline tests (`#[test]` / `#[tokio::test]`) | 6,548 (476 `.rs` files, 229,992 lines) |
| `tests/` directory tests | 994 test functions (124 `.rs` files, of which `tests/unit/` has 71 files) |
| Explicit `[[test]]` targets | 6 (integration_tests / main / sdk_api_test / route_diag_test / ws_task_status / acceptance) |
| Criterion benchmark groups | 9 (`benches/benchmark.rs`) |
| Python test suites | 5 (`tests/python/`) |
| BDD acceptance scenarios | 61 scenarios / 10 feature files (`tests/acceptance/features/`; 2 `@external` ones do not run locally) |
| feature compile matrix | CI 7 combinations + E2E 22 combinations |
| Coverage gate | line coverage ≥ 90% (CI coverage job, reported to Codecov; measured 90.14%) |

> The `acceptance` target (`tests/acceptance/main.rs`, `harness = false`, `required-features = ["platform"]`) is driven by the CI `acceptance-test` job (`cargo test --test acceptance --features platform`, excluding `@external`).

> Statistics fluctuate as the code evolves; the current HEAD's measured values are authoritative.
