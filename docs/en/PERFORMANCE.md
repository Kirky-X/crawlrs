# ⚡ Crawlrs Performance Guide

crawlrs's performance engineering is built into the architecture (AIMD adaptive concurrency, memory-aware scheduling, TabPool, request coalescing, WaitFor, Hedge, etc.). This guide covers the reproducible benchmark suite, performance observability, and tuning advice. For benchmark design details and module implementations see [🏗️ Architecture](ARCHITECTURE.md).

## 📋 Table of Contents

- [🎯 Overview](#-overview)
- [📏 Benchmark Suite](#-benchmark-suite)
- [📊 About the Historical Comparison Numbers](#-about-the-historical-comparison-numbers)
- [⚡ Runtime Performance Design](#-runtime-performance-design)
- [💾 Caching and Resource Configuration](#-caching-and-resource-configuration)
- [📈 Observability and Measurement](#-observability-and-measurement)

---

## 🎯 Overview

- **Reproducible benchmarks**: `benches/benchmark.rs` provides 9 Criterion benchmark groups, runnable in one shot with `cargo bench`; E2E suite Stage 5 establishes `e2e-baseline` on first run, and every subsequent full run automatically compares against it to detect regressions.
- **Observe first**: Prometheus metrics (`/metrics`) expose queue depth, engine success rates, engine duration distribution, cache hit rates, and webhook delivery results; use the data to locate bottlenecks before tuning.
- **Trim by feature**: engines/business capabilities that are not enabled do not participate in compilation or runtime; `--no-default-features` yields the deployment surface with the smallest resource footprint.

### Performance Goals

crawlrs targets high throughput, low tail latency (P99), and controlled memory. The repo currently **has no authoritative benchmark numbers published** — the legacy Node.js comparison figures lacked a measurement methodology; see [About the Historical Comparison Numbers](#-about-the-historical-comparison-numbers); for how to reproduce see the [Benchmark Suite](#-benchmark-suite).

---

## 📏 Benchmark Suite

`benches/benchmark.rs` (`[[bench]]` name=`benchmark`, harness=false, Criterion 0.8) contains 9 benchmark groups:

| Benchmark group | What it measures |
|--------|----------|
| `benchmark_task_creation` | task creation path overhead |
| `benchmark_task_status_transitions` | task state-machine transition overhead |
| `benchmark_json_serialization` | JSON serialization/deserialization (serde_json) |
| `benchmark_url_parsing` | URL parsing |
| `benchmark_url_validation` | URL validation |
| `benchmark_uuid_generation` | UUID generation |
| `benchmark_ssrf_detection` | SSRF detection hot path |
| `benchmark_regex_cache` | RegexCache cache hit/build |
| `benchmark_engine_routing` | engine routing (strategy selection with multiple mock engines) |

```bash
# Run all benchmarks (release profile, inherits lto=thin / codegen-units=1 / opt-level=3)
cargo bench

# View the report
target/criterion/report/index.html
```

### Baseline Capture and Regression-Comparison Methodology

Criterion writes measurement results to `target/criterion/`; baselines are archived by name via `--save-baseline` and compared via `--baseline`. **Criterion only reports relative changes and sets no over-threshold exit code**; the regression gate is `scripts/bench_gate.py` (exit code `0` = pass / `1` = over threshold / `2` = baseline missing).

```bash
# 1) Capture a baseline (must be rebuilt after any machine or sampling-methodology change)
cargo bench --bench benchmark -- --warm-up-time 1 --measurement-time 3 --sample-size 10 \
  --save-baseline ci-baseline

# 2) Compare against the archived baseline (same sampling parameters)
cargo bench --bench benchmark -- --warm-up-time 1 --measurement-time 3 --sample-size 10 \
  --baseline ci-baseline

# 3) Threshold judgment: any benchmark whose median rises above the threshold yields exit code 1
python3 scripts/bench_gate.py --criterion-dir target/criterion \
  --baseline ci-baseline --threshold-pct 20
```

**Artifact paths and naming rules**

- Baseline snapshots: `target/criterion/<group>/<benchmark>/<baseline_name>/estimates.json` (the same directory also holds `sample.json` / `benchmark.json`; `new/` is the most recent run). The gate reads `median.point_estimate`.
- HTML report: `target/criterion/report/index.html`.
- The Criterion output root follows cargo's target directory: `bench_gate.py` resolves it via `CARGO_TARGET_DIR` by default (`target/` if unset); when a user-level cargo config sets `build.target-dir` elsewhere, pass `--criterion-dir <that path>/criterion` explicitly.
- Baseline naming convention: `e2e-baseline` (E2E suite Stage 5), `ci-baseline` (the bench gate). **One name is fixed to one snapshot on one machine**; switching machines requires a new name (e.g. `ci-baseline-runner-<spec>`) — never reuse a baseline across machines.

**CI gate (`.github/workflows/bench.yml`, `workflow_dispatch` only)**

- `mode=save`: captures and uploads the artifact `criterion-baseline-<baseline_name>` (retained 90 days).
- `mode=compare`: downloads the most recent successful capture of that artifact, runs the `--baseline` comparison, and `bench_gate.py` judges the result — over threshold fails the job. First use requires one `save` run.
- bench is not in the default CI (not triggered on push/PR) to avoid competing with the functional gates for runners.

**Machine notes and number discipline**

- Baselines are strongly machine-dependent (CPU model/frequency, memory bandwidth, background load); numbers across machines are not comparable. GitHub-hosted runner specs drift, and the default threshold of `20%` absorbs noise.
- The sampling parameters (`--warm-up-time` / `--measurement-time` / `--sample-size`) must exactly match those used to capture the baseline, otherwise `bench_gate.py` treats them as unpairable (exit code 2).
- **This repo publishes no unreproduced absolute performance numbers**; any externally referenced number must be accompanied by machine specs, sampling parameters, and the capture commands.

### Integration with the E2E Baseline

Stage 5 of `./tests/e2e/e2e-suite.sh` compiles and briefly runs the benchmarks with shortened sampling parameters:

- the first run establishes the `e2e-baseline` baseline;
- every later run automatically compares against it and fails on a significant regression — performance regressions are guarded at the same level as functional regressions.

---

## 📊 About the Historical Comparison Numbers

The project's early promotional material cited the following comparisons (relative to the Node.js implementation):

| Metric | Early promotional claim |
|--------|-------------|
| Throughput | 3-5x higher (roughly 1,200 → 4,500 requests/sec) |
| P99 latency | about 50-60% lower (450ms → 180ms) |
| Memory usage | about 75% lower (512 MB → 128 MB) |
| CPU usage | about 59% lower (85% → 35%) |

> ⚠️ **This set of numbers came with no measurement script, environment, or load model, is not reproducible within the repo, and is kept for historical reference only.** Authoritative benchmark data: the plan is to re-measure in a fixed environment with the Criterion suite and k6 load testing (`tests/stress/k6_script.js`) and then update this section.

---

## ⚡ Runtime Performance Design

All of the following mechanisms are implemented with corresponding tests/benchmarks (code locations and design details in [🏗️ Architecture · Crawl Capability Enhancement Modules](ARCHITECTURE.md)):

| Mechanism | Benefit |
|------|------|
| AIMD adaptive concurrency (`adaptive_concurrency.rs`) | lock-free `AtomicUsize` + `AdaptiveSemaphore`; halves on consecutive failures, increments on success, automatically approaching optimal concurrency |
| Memory-aware scheduling (`workers/scheduler/`) | `MemoryState{Normal,Pressure,Critical}` three-state admission + priority-queue aging, preventing OOM and starvation under pressure |
| TabPool (Chrome CDP tab pool) | DashMap + AtomicUsize LIFO stack reuses tabs, eliminating per-request tab-creation overhead |
| Request coalescing (`coalesce.rs`) | DashMap single-flight + broadcast notification; concurrent requests to the same URL trigger only 1 actual scrape |
| WaitFor conditional waiting | NetworkIdle / Selector / DomStable replace fixed sleeps, reducing pointless waiting |
| Hedge request-replica controller | EMA + variance estimate the P84 threshold; above it, replica requests race, cutting tail latency |
| Smart retry + full-jitter backoff | independent caps per RetryReason, avoiding retry storms |
| RegexCache / Bloom⊕Interner dedup | regex compilation cache + layered URL dedup, cutting CPU and DB queries |
| Waterfall/MRT timeouts | per-engine MRT configuration; on timeout, switch to the next engine, preventing one engine from dragging down tail latency |

---

## 💾 Caching and Resource Configuration

### Multi-Level Cache (oxcache / moka)

The `[cache]` config section controls the L1 in-memory cache (`enabled` / `[cache.memory] capacity` / `ttl_seconds`) and supports three per-type TTLs for search / dns / regex (`[cache.types.*]`); 4 cache modes (Enabled / Disabled / ReadOnly / Bypass) gate per request (`CacheContext`; the old WriteOnly semantics were merged into Bypass).

### Key Tuning Parameters

| Setting | Default | Advice |
|------|------|------|
| `[cache.memory] capacity` / `ttl_seconds` | 10000 / 300 | raising it toward the working-set size improves hit rate (observe `crawlrs_cache_hit_total`) |
| `[concurrency] default_team_limit` | 10 | adjust per team quota in multi-tenant scenarios |
| `[workers] count` | `auto` | for CPU-bound workloads set to physical cores; higher for IO-bound |
| `[timeouts.engines]` MRTs | see `config/default.toml` | calibrate against the target sites' response distribution; too small amplifies the engine-switch rate |
| `[rate_limiting] default_rpm` / `burst_size` | 60 / 20 (toml values; the code `#[config(default)]` is 100 — toml wins) | match downstream sites' tolerance to avoid pointless 429 retries |
| `[proxy]` rotation and sticky sessions | - | enable category routing and health checks when many domains get blocked |

---

## 📈 Observability and Measurement

### Prometheus Metrics (the `metrics` feature, `GET /metrics`)

> ⚠️ **`/metrics` is currently a placeholder implementation**: `metrics_handler` returns a JSON envelope whose `metrics` field is **hard-coded Prometheus-style text** (`version` fixed at `1.0.0`, uptime fixed at `3600`), neither real runtime metrics nor a Prometheus-scrapeable aggregation endpoint. The authoritative `/metrics` scrape format is **yet to be implemented** — until then, the table below remains the authoritative list of metric instrumentation (capture points and label definitions), and Prometheus/Alertmanager scraping must not rely on the current endpoint. This matches the `/metrics` description in [API_REFERENCE.md](API_REFERENCE.md).

| Metric | Type | Description |
|------|------|------|
| `crawlrs_queue_depth` | Gauge | task queue depth |
| `crawlrs_engine_success_total` | Counter | engine success/failure counts |
| `crawlrs_engine_duration_seconds` | Histogram | engine request duration distribution |
| `crawlrs_cache_hit_total` | Counter | cache hits/misses |
| `crawlrs_webhook_delivery_total` | Counter | webhook delivery success/failure |

`docker/prometheus/` provides sample Prometheus and Alertmanager configurations; `docker/docker-compose.yml` starts the full observability stack in one shot.

### Measurement Method Advice

1. **Microbenchmarks**: `cargo bench` (the benchmark suite on this page) for in-library hot-path regression.
2. **End-to-end load testing**: `k6 run tests/stress/k6_script.js` to stress the running service and observe throughput and percentile latencies.
3. **Python performance probes**: `python -m pytest tests/python/test_performance.py` (or `./scripts/run-tests.sh local`) for basic response-time patrols.
4. **Production observation**: in Grafana dashboards, watch for sustained `queue_depth` growth (insufficient workers), sudden percentile spikes in `engine_duration_seconds` (engine/site degradation), and a falling `cache_hit_total` hit rate (insufficient capacity).
