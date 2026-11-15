# 📋 Crawlrs Changelog

Current version: 0.2.0 (matches `Cargo.toml` `version`). The format follows [Keep a Changelog](https://keepachangelog.com/), and versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 📋 Table of Contents

- [0.2.0 - 2026-11-15](#020---2026-11-15)
- [0.2.0-rc.4 - 2026-11-10](#020-rc4---2026-11-10)
- [0.2.0-rc.3 - 2026-11-10](#020-rc3---2026-11-10)
- [0.2.0-rc.2 - 2026-09-22](#020-rc2---2026-09-22)
- [0.2.0-rc.1 - 2026-08-26](#020-rc1---2026-08-26)
- [0.2.0-legacy - 2026-07-29](#020-legacy---2026-07-29)
- [0.1.0 - 2026-07-22](#010---2026-07-22)

---

## [0.2.0] - 2026-11-15

The first stable crates.io release. All in-house dependencies normalized to stable versions (minor precision), `alchemize` switched to the crates.io registry, the repo has no path/git dependencies at all, and the `cargo publish` channel is unblocked. This section merged the former `[Unreleased]` section.

### Added

- **i18n system rewrite**: the message layer switched to the `sdforge::i18n` host registry, with `locales/*.ftl` embedded at compile time (`src/i18n/catalog.rs`, `include_str!` + startup-time line-parsed `register_translation`); deployed artifacts no longer carry an on-disk `locales/` directory; the key guard was upgraded to bidirectional consistency between the embedded manifest ↔ the on-disk directory (`tests/unit/i18n`); `t_with_args`/`tr_log_args` arguments simplified to `&[(&str, String)]` (`{ $name }` replaced by name)
- **crates.io publishing channel**: with all dependencies registry-ized, `cargo publish` is unblocked; `release.yml` migrated to the family's final state (OIDC trusted publishing preferred + `CARGO_REGISTRY_TOKEN` fallback, idempotent `gh release` script step)

### Changed

- **Default Markdown converter switched to alchemize**: the `[markdown].converter` default changed `moli` → `alchemize` (the in-house html engine: htmd base chain + in-house deep chain, with built-in empty-output guarding / honest degradation / math `$TeX$` fidelity routing); the `content` feature absorbed alchemize, and `htmd` remains as the pass-through diagnostic arm. Equivalence measured on 13 cases (5 real pages + 8 synthetic): 10/13 byte-equal (trailing newline only), math pages improved from leaked MathML to `$TeX$`, 2 heuristic differences in navigation widgets (no content loss); throughput on a 1.06MB wiki ×20: moli 105.6 vs alchemize 96.8 MB/s (-8%); the assembly-chain output is byte-identical to native alchemize on the 5 real pages
- **MSRV 1.97 → 1.97.1**: aligned with alchemize's `rust-version`; CI toolchain matrix, README/FAQ/CONTRIBUTING wording synced
- **Coverage job narrowed to `--lib`**: after alchemize entered the tree, all-target cold builds exceeded 75m repeatedly (cancelled runs do not write back the cache); lib-only measured 90.12% (integration-test gain <0.01pp), gate unchanged at 80%
- **CI gate methodology**: fmt changed to `-p crawlrs -p crawlrs-examples` targeting the two packages; coverage/integration/BDD job timeouts raised based on measurements (30→45→75/25→40/30 minutes)

### Dependencies

- **All in-house dependencies normalized to stable versions (minor precision)**: confers `0.6`, dbnexus `0.6.1`, garrison `0.9`, inklog `0.3`, limiteron `0.3`, nebulaid `0.3`, oxcache `0.5`, sdforge `0.5`, trait-kit `0.5`, vecboost `0.3`, fluxweir `0.1` (resolving 0.1.2), alchemize `0.1`; direct-dependency rc pins reduced to zero
- **alchemize switched to the crates.io registry**: removed `path = "../alchemize"`, so `cargo publish`/`cargo package` are no longer rejected by path dependencies; the CI `.github/actions/alchemize-sibling` checkout chain was deleted (a clean checkout resolves directly)
- **fluxweir back on the registry**: starting 0.1.1 it provides `with_pool`/`build_app_with_pool`/`TrustedIdentity`; the rev-pinned git dependency was dropped and deny's `allow-git` cleared

### Removed

- **moli-html2md dependency and converter chain retired**: deleted `moli_adapter.rs` / `moli_converter.rs` / `converter_chain.rs` (moli main + htmd fallback chain); the `content` feature dropped `dep:moli-html2md` and `dep:ego-tree`; the `crawlrs_md_converter_fallback_total` metric was removed with the chain (alchemize's built-in fallback is no longer counted separately)
- **fluent-bundle dependency retired**: after the message surface unified onto the `sdforge::i18n` host registry there are zero reference points; the `I18nSettings.locales_dir` field was removed in sync (the `CRAWLRS__I18N__LOCALES_DIR` environment variable is no longer read)
- **Migration note**: `CRAWLRS__MARKDOWN__CONVERTER=moli` (the old default) is rejected during config validation (startup failure); change it to `alchemize` (the default) or `htmd`

### Fixed

- **rag-local runtime DOA**: vecboost with `default-features=false` stripped the `onnx` main engine while the provider pinned `EngineType::Candle` — the vecboost factory fail-fasts on uncompiled engines, so `rag.enabled=true` + `embedding_provider="local"` failed at startup. Fix: the dependency explicitly sets `features=["onnx"]`, and the provider pins `EngineType::Onnx`
- **Two nightly clippy `needless_bool` occurrences**: the sitemap seed guard now returns the feed result directly; the cache_mode POST decision uses a non-matching expression
- **SDK integration test E0061**: `sdk_api_test`'s `I18nBundle::load` switched to the two-argument call (a stale test missed after the sdforge host-registration switch)
- **acceptance teardown `single_match`**: the logger shutdown in `tests/acceptance/teardown.rs` changed to `if let` (the release workflow's `clippy --tests` would block it)
- **BDD infrastructure**: two companion defects closed out — swallowed warn logs and a Ryuk container leak

### Security

- **security workflow permissions**: added `checks: write` — rustsec/audit-check needs it to write back check-runs (R-17: Resource not accessible by integration)
- **CodeQL alert-adjudication workflow**: adjudication idempotency (skips already-adjudicated alerts), adjudication reason enum aligned with API literals, switched to PATCH (the current REST endpoint method)

## [0.2.0-rc.4] - 2026-11-10

### Fixed (release pipeline full re-run: CI re-green and hygiene cleanup)

- **CI fixes**: 6 no-default warnings across the five Feature Matrix combinations (`RetryReason` cfg gating, race/sequential unused parameters, three url_redaction allows reassigned per feature); the engine-moli memory-gate test's cross-submodule access changed to `state_for_test()` (also fixing the `_available_mib` warning); the BDD job gained the PostgreSQL service and migrations steps (previously TEST_DATABASE_URL pointed at a nonexistent service and bootstrap went all red on "Connection closed")
- **Queue metric fix**: the `PostgresTaskQueue` depth counter clamps at 0 — crawl subtasks enqueue directly via `repository.create` while the worker decrements via `queue.dequeue`; negative drift wrapped the u64: in debug builds a later enqueue overflowed and panicked (the root cause of intermittent BDD acceptance "connection closed before message completed"), and release silently emitted garbage `crawlrs_queue_depth`; a regression unit test is attached
- **Security channel**: added `.cargo/audit.toml` sharing exemptions with `deny.toml` (including RUSTSEC-2026-0459 — sqlx 0.9's `<0.38` constraint blocked the fixed 0.39); CodeQL's 18 open alerts adjudicated to zero one by one (9 identifier/masked-log FPs, 2 high-entropy key digest FPs, 2 per-request TLS option won't-fix, 5 test-fixture used-in-tests)
- **Comments and docs**: cleaned task-number/review-stamp comments (CI and worker/engine/handler comments); fixed 3 doctest errors (`record_block()`/`intercepted_count()`/the `HtmdMarkdownService` path); API_REFERENCE/FAQ/USER_GUIDE versions and health examples aligned; README stats re-measured (230,418 lines)
- **Examples**: the 18 examples previously registered zero engines via `EngineClient::new()` and always failed; added `crawlrs_examples::default_engine_client()` (ReqwestEngine) and wired 17 examples (`mllm_scrape` needs the full LLM DI and deliberately keeps the explicit zero-engine client to avoid ordinary HTTP success masquerading as MLLM success); `basic_scrape` verified against example.com with HTTP 200; all examples compile with `-D warnings`
- **Dependencies**: html-escape 0.3.1, toml 1.2.1, uuid 1.29.2 (cargo-outdated leaves only testcontainers-chain-constrained items; pins maintained)
- **Verification**: lib 6379/0, integration 170/170, main 790 passed/16 ignored, sdk 8/8, doctest 10/10, line coverage 90.13% (pipeline Step 9 target ≥90 met; the CI threshold was still 80% at the time), E2E matrix/static/unit/integration/bench/liveapi six stages PASS (report aggregation SKIPPED by design), BDD acceptance 5 consecutive runs of 59/59, clippy(default/full/agent-lib)+doc+deny+audit all green

## [0.2.0-rc.3] - 2026-11-10

### Fixed (pre-release closure: terminal-state reconciliation and security hardening)

- **Crawl terminal state and crash recovery**: terminal-state determination consolidated into a single SQL constructor (`terminal_status`; `status IN (queued,processing)` uses the index); enqueue-interruption compensation writes `failed = max(total-completed, failed)` preserving `completed+failed==total`; the task completion count moved after sub-link enqueue (eliminating the premature-terminal-state window); the expiry sweep gained crash-recovery recounting (a non-terminal crawl silent for >10 minutes with no queued/active tasks is reconciled from actual task counts, with a recent-task-activity guard against mistaken termination)
- **Team domain blacklist end-to-end**: the sole carrier is `crawl.config` (no longer copied into task payloads); normalization (host extraction/IDN/trailing dots/userinfo) + caps of 1000 entries / 253B each + rejection of non-normalizable/wildcard entries; at execution time seeds/redirect final_urls/links/sitemap top-level and sub-sitemaps/proxy are validated with the same standard, fail-closed when the snapshot is unavailable; 300s worker cache (Arc-shared); rolling upgrades fall back to the legacy top-level payload copy
- **SDK crawl creation and search-derived crawls**: `POST /api/v1/sdk/crawl` (name/url/seed_url, SSRF pre-check, rate limiting, compensation writes); search-derived crawl results get a 200 hard cap, depth ≤5 dual-layer validation, and blacklist normalization once outside the loop
- **Security hardening**: scrape/crawl requests uniformly drop custom `Host` headers (vhost SSRF bypass); rejection reasons contain only the normalized host (no path/query/userinfo)
- **Build surface**: `web-axum` no longer hard-codes the inklog/postgres driver; the db-postgres/sqlite/mysql three-combination compile matrix is all green; the missing-driver guard covers platform/web-axum
- **Verification**: three-dimensional review (security/architecture/performance) three rounds PASS; fmt/clippy(default,full)/`-D warnings` test compilation/pre-commit 13 items/cargo deny all green; real PG: lib 6402 passed (without local Chrome the 5 Playwright XHR cases fail only under --include-ignored), integration 170/170, main 806/806, sdk 8/8

- **Publishing channel**: release.yml's Version Consistency CHANGELOG path aligned to `docs/` (fixing a migration leftover); when git dependencies are detected (moli-html2md / fluxweir not yet registry-ized), crates.io publishing is explicitly skipped while GitHub Release proceeds — the dual channel resumes automatically once upstream publishes a registry version; ci.yml's leftover `checkout@v4` pinned SHA

### Changed (full dependency upgrade, deps-refresh)

- **Upgraded to latest minors across the board**: tokio 1.53→1.54, uuid 1.27→1.29, sysinfo 0.39→0.40, scraper 0.27→0.29, dom_smoothie 0.18→0.19, lru 0.18→0.19, html-escape 0.2→0.3, parking_lot 0.12→0.13, toml 1.1→1.2, utoipa 5.5→6.0 (same version as sdforge 0.5.0-rc.10's `pub use utoipa` re-export); the other direct dependencies' latest minors matched existing constraints — no changes needed
- **In-house dependencies raised to latest rc**: confers 0.6.0-rc.13, dbnexus 0.6.0-rc.11, sdforge 0.5.0-rc.10, garrison 0.9.0-rc.5, inklog 0.3.0-rc.12, limiteron 0.3.0-rc.10, trait-kit 0.5.0-rc.12, oxcache 0.5.0-rc.12, vecboost 0.3.0-rc.3, nebulaid 0.3.0-rc.4; semver requirements within the chain are mutually compatible, no dual-version coexistence
- **Removed local-dependency forms**: nebulaid dropped `path = "../NebulaId"` for a pure registry rc.4 (the symbols needed by SDK/ID already exist in the registry version); deleted `[patch.crates-io] vecboost = { path = "../vecboost" }` (registry 0.3.0-rc.3 already depends on confers ^0.6.0-rc.12, so the local patch was no longer needed). **fluxweir changed to a rev-pinned git dependency (external path dependencies zeroed)**: 4 local commits (3fe9e68/c872012/16b7cc5/2d559ad) were pushed to `github.com/Kirky-X/fluxweir` and the dependency changed to `git = ..., rev = "2d559ad"` (including `build_app_with_pool`/`TrustedIdentity`); deny allow-git whitelisted that source. Registry 0.1.0 still lacked the needed APIs; it switches to pure registry once a new version is published — from this point a clean checkout resolves dependencies directly
- **sysinfo 0.40 API adaptation**: `System::new()/new_with_specifics()` now return `Result`; the three monitoring components degrade **fail-closed** with `Option<System>` — when sampling is impossible, CPU/memory reads return the 100% sentinel (driving threshold alerts, never a fake healthy 0%), the poisoned-lock path shares the semantics, `MutableSystemMonitor` (the production consumer) reports `is_metrics_stale = true` when degraded, and a new `crawlrs_system_monitor_degraded` gauge distinguishes "cannot sample" from "genuinely saturated"; when the memory-watermark sampler fails to initialize it immediately calls `mark_sampler_unavailable()` (watermark Critical, engine scheduling rejects new tasks) and logs an error
- **Pins maintained (with explicit upstream constraints)**: wreq 6.0.0-rc.31 (all of 5.x yanked; rc.31 is the latest usable), testcontainers 0.27 (testcontainers-modules 0.15.0 hard-depends on ^0.27; moving to 0.28 conflicts), axum-test 21.1 (22.0 is rc-only), dashmap 6.2 (7.0 is rc-only); the moli-html2md git dependency pinned rev `2db52b45` = tag v1.1.21 (latest tag)
- **Verification**: pre-commit-check all 13/13, default lib 6174/0, full lib 6357/0, live integration 161/0, acceptance 49/49, markdown_fidelity 8/8, e2e --quick all stages PASS (liveapi skipped per quick), cargo deny four checks ok, clippy three legs `-D warnings` 0 errors; `Cargo.lock` fully re-resolved and committed

### Added (fluxweir gateway integration, fluxweir-llm-gateway)

- **Unified LLM outlet**: new `gateway` feature (fluxweir pulled in with `default-features=false, features=["full","db-postgres"]`, avoiding the dbnexus embedded×server-side mutual-exclusion guard); `[fluxweir]` config section (`enabled`/`mode = embedded | external`/`listen_addr`/`external_base_url`/`api_key_env`/`trusted_proxy`/`rag_fallback`; all off by default, api key injected via env, Debug-masked, fail-fast at startup). `FluxweirLlmCarrier` implements `LLMServiceTrait` (prompt template → SDK chat → JSON parsing/usage mapping; chunker/provenance orchestration reused); `init_llm_service` assembles per `[fluxweir].enabled`, keeping `ExplicitErrorCarrier` when disabled
- **Embedded single-pool sharing**: `bootstrap/fluxweir_gateway` starts the gateway via `build_app_with_pool` by cloning crawlrs's dbnexus connection pool (fluxweir gained incremental `Database::with_pool`/`assemble_with_database` APIs); request_logs/token_usage/quota ledger land in the same PostgreSQL as crawlrs; readyz probes with exponential backoff, fail-fast; the shutdown order is pinned — call `handle.shutdown()` and wait for the flush before releasing the pool close. The fluxweir DI kit pool (the inklog storage surface) connects independently per host URL (empty URL fail-fasts)
- **Migration history table separation**: crawlrs migrations moved to a dedicated history table `crawlrs_migrations` (an explicit `run_crawlrs_migrations` executor; the pool's `auto_migrate` is always off and handed over); the first run backfills a one-time baseline from `dbnexus_migrations` by `file_path LIKE '%migrations/%'` — eliminating the version-space collision between crawlrs (1..8) and fluxweir (1..=9) on the same default history table; the `migrate` subcommand switched in sync
- **Embedding/rerank fallback chain**: `FluxweirEmbeddingProvider`/`FluxweirRerankProvider` (dimensions/out-of-range/missing slots/base64 all explicitly reported) as secondary; `FallbackEmbeddingProvider`/`FallbackRerankProvider` composite wrappers (single switch on primary failure; on double failure embedding errors / rerank fails open preserving order); assembly matrix — vecboost primary + fluxweir fallback, fluxweir-only when rag-local is absent, pass-through when both absent; the shared pool's `retry_policy` explicitly re-enables fluxweir's idempotent-read retry semantics
- **rig-core/genai retirement**: the `rag-remote` feature and `rig_embedding_provider` were removed (remote embedding fallback now carried by fluxweir), with `rag = ["rag-local", "gateway"]`; `vision_adapter` switched to fluxweir chat multimodal (`engine-mllm` explicitly depends on `gateway`, no separate compile state), and the genai dependency was removed entirely; `http_rerank_provider` keeps direct Cohere/TEI connections
- **Auth and tenant surface**: the gateway `[auth]` digest authentication (the SDK loopback passes through the client's original key; tenant/quota/cost attribution per real client per tenant; scoping decision: no Router::merge — it would collide with existing /health paths and panic at startup); fluxweir gained a `TrustedIdentity { team_id, is_admin }` trusted-proxy mechanism (the `[auth].trusted_proxy` hot-read switch, default false with byte-identical behavior; is_admin=false rejects /admin/*, pinned by 4-branch unit tests) — retained for a future HTTP direct-connection identity bridge
- **Verification**: live integration tests (`tests/integration/fluxweir_gateway_test.rs`, `#[ignore]` + real PostgreSQL) cover startup/probing/persistence/history-table separation/401/graceful shutdown; fluxweir-side lib 1124 tests + dual-backend clippy all green; crawlrs four feature combinations (default/gateway/rag-local+gateway/full) clippy `-D warnings` all green; the in-house dependency chain moved up in sync with fluxweir (sdforge rc.10, trait-kit/confers/oxcache/inklog rc.12, limiteron rc.10), with the fast-masking surface regressed without panics

### Added (architecture evolution closure, arch-evolution-closure)

- **Multi-dialect schema provisioning**: migration DDLs are split per dialect (`migrations/{postgres,sqlite,mysql}/`); `dbnexus_connection` picks `migrations_dir` by compiled features and wires `[database] auto_migrate` (default false); an unmatched dialect errors explicitly; `bootstrap/services.rs` removed the "PG DDL failure → fall back to memory" path; a new `tests/migration_dialect.rs` asserts all three dialects cover the same tables and columns
- **WebSocket task status**: new `GET /v1/tasks/{id}/ws` (axum ws); after upgrade it subscribes to the existing `event_bus` (tokio broadcast) and pushes status frames filtered by task id, reusing the existing auth/tenant middleware; new connection-cap and idle-timeout configuration (`[websocket]`) with rejection asserted when over the cap
- **Redis shared cache layer**: new `[cache] backend = "memory" | "redis"` and `redis_url`; the `cache-redis` feature forwards to `oxcache/redis`; `backend=redis` fail-closed rejects at startup when the feature is not compiled or the URL is missing; `cache_service` constructs per backend (`RedisBackend` injected via `Cache::builder().backend_arc`; `rediss://` forced by default, non-TLS needs an explicit `OXCACHE_ALLOW_INSECURE_REDIS`), with get/set/ttl semantics identical across both backends (per-key TTL); Redis unreachability is always a rejection (no degradation)
- **Performance-baseline regression gate**: new `.github/workflows/bench.yml` (`workflow_dispatch` only, not in default CI); `mode=save` captures and archives the Criterion baseline as an artifact, `mode=compare` compares against the archived baseline; new `scripts/bench_gate.py` handles over-threshold judgment (Criterion only reports changes and sets no exit code); `benches/benchmark.rs` and `docs/PERFORMANCE.md` document the capture/comparison methodology, artifact path naming, and machine notes
- **Status alignment**: README / README_EN roadmap "architecture evolution" 📋→✅ (event bus annotated with tokio broadcast), "multi-dialect databases" 🚧→✅ (runtime schema provisioning landed); added the `forge_api` (`forge-full`) in-flight migration entry; `docs/TEST_SCENARIOS.md` migration wording updated to the three-dialect directory reality, the Redis backend explanation, and the `migrations/postgres/` path

### Added (RAG semantic rerank, rag-rerank)

- **Search semantic rerank**: the search pipeline previously ended at statistical ranking (RRF fusion + heuristic relevance overlay) with no query-doc semantic relevance. The new `SearchReranker` (`domain/services/rerank.rs`) reorders the top-N after result mapping: the reranked subset is pinned on top by provider score, the rest keep their original order, and each item gets a `score`; with no provider or fewer than 2 results it passes through, and provider failures fail open preserving the original order
- **Two provider chains**: `rag-local` (vecboost 0.2.1, Candle CPU — one bi-encoder model covers both embedding and rerank) and `rag-remote` (rig-core 0.42 embedding via an OpenAI-compatible endpoint); `vecboost`/`rig-core` are optional, outside `default`/`full`, so the default build gains zero new dependencies
- **Generic HTTP rerank adapter** (no feature gate, uses existing reqwest): `http_rerank_provider` supports the `cohere` (Cohere v2 / Jina / most self-hosted: `{model,query,documents,top_n}` → `results[{index,relevance_score}]`) and `tei` (`{query,texts}` → `[{index,score}]`) wire formats, with configurable Bearer auth
- **RAG-augmented extraction wiring**: `ExtractionService::with_rag` + `extract_with_rag_auto` — when rag is not injected, the query is empty, or the context is empty, it is equivalent to the existing `extract_with_schema` (fail-open, falling back to the full page); the worker's three schema-extraction branches uniformly switched to this entry; prompt-only extraction keeps the full-page path
- **Configuration and gating**: new `[rag]` section (all off by default; `enabled`/`embedding_provider`/`rerank_provider`/local and remote model parameters/`rerank_format`/`search_rerank_top_n`/`rag_top_k`), api keys injected via `CRAWLRS__RAG__*` env and Debug-masked; startup `validate_providers` fail-fasts on "provider configured but its feature not compiled"
- **API exposure**: `SearchResultDto.score` (`skip_serializing_if = "Option::is_none"`); when reranking is active each result carries a 0..1 relevance score; when disabled the field is omitted entirely

### Fixed (registry zero-out, registry-zero-out)

- **429 responses use the standard envelope uniformly**: `check_rate_limit`'s `Denied`/`RetryAfter`/backend errors (fail-closed 503) now go through the `response_builder::errors` standard envelope (`error.code = "RATE_LIMITED"`, 503 as `SERVICE_UNAVAILABLE`; `RetryAfter` keeps `retry_after_seconds`); unit tests assert the new shape; the liveapi 429 gate assertion upgraded to `error.code = "RATE_LIMITED"` (the old string envelope is no longer accepted)
- **Failed-task errors visible end-to-end**: `RetryHandler::handle_failure` gained an `error` parameter; the terminal branch writes `task.payload["error"]` (empty strings are not written, preserving the "only fall back when there is truly no error string" semantics); scrape/crawl call sites pass the real error; the failed task's API `error` field no longer falls back to generic copy
- **Crawl result-row invariant**: `handle_crawl_success` calls `ensure_result_row` before `increment_completed_tasks` — missing result rows (every path including soft-404 skips) get a minimal row (`status_code=0` + `meta_data.note="no content captured"`); rows that exist get zero extra writes; unit tests cover both the backfill and skip paths
- **Live-suite credentials off argv (CWE-214)**: server/worker inject via a 0600 `test-results/.liveapi-env` + inner `set -a; . file; exec`, so `ps` sees only the file path; cleanup deletes the file both at startup and at exit
- **Doc sync**: the rate-limit response samples in `docs/API_REFERENCE.md` updated to the new envelope (`error.{code,message}` + `retry_after_seconds`)

### Fixed (registry final cleanup — single error-response shape / full fingerprint coverage / result invariants / backend evidence)

- **Single error-response shape**: `CrawlRsError::IntoResponse` unified onto the standard ApiResponse envelope (data=null + timestamp + error.{code,message}; dev/prod information tiering retained), removing the redundant `error.status` field — error responses are now a single shape repo-wide (except RateLimitErrorResponse's `retry_after_seconds` extension, shown with the sample)
- **Full browser-engine fingerprint coverage**: UA/client-hints/locale/viewport/stealth setting extracted into the shared `apply_browser_fingerprint`; MllmEngine wired in — previously mllm did a bare goto after fetching the page, the only engine with no fingerprint setting (runtime verification rides with engine enablement; the compile level is already covered)
- **Crawl result-row invariant**: sub-tasks reaching terminal state without a result row get a placeholder row (a COUNT-projection existence check, not a full-row read) — covering every no-result path such as dedup/soft-404/coalescing misses, making "completed⇒/results is queryable" hold; the semantic suite's crawl-family scenarios gained a queue-drain barrier (timing luck → deterministic ordering)
- **Database rate-limit backend evidenced**: the storage backend is parameterized (`CRAWLRS_TEST_STORAGE_BACKEND`); the database-mode 429 gate passes end-to-end (limiteron postgres storage availability was previously in doubt, now proven); the UA-pool version-freshness maintenance standard landed on the UaPool doc anchor

### Fixed (leftover convergence — UaPool consumption surface / locale / arch + test gates)

- **CriOS Client Hints source of truth**: the two iOS Chrome profiles' `sec_ch_ua` templates corrected to the empty string (WKWebView sends no Client Hints; the original Chrome brand list was fabricated capability) — the reqwest etc. UA consumption paths no longer emit the contradictory "UA=WKWebView + client hints=Chromium" combination (one data-layer fix benefits pick()/pick_seeded/router branches everywhere); the consistency unit test puts CriOS in the empty-string set
- **Playwright locale coverage**: page setup gained `Emulation.setLocaleOverride` (locale = the first tag of accept_language), closing the secondary contradiction of "request headers zh-CN while Intl defaults to en-US" for zh-CN profiles (a leftover gap where launch `--lang` is a process-level fixed value); CDP failures are best-effort and non-blocking
- **Sec-CH-UA-Arch host consistency**: architecture derives from the host's `std::env::consts::ARCH` (x86_64→x86, aarch64→arm), replacing the hard-coded x86; Android profiles keep the arm device narrative
- **Test gates added**: liveapi gained an idempotent double-run verification of the `crawlrs migrate` subcommand (manual psql migrations previously happened to bypass the binary's migration path) and a 429 rate-limit gate (80×SSRF-rejected request burst → 429 + rate-limit error signal, placed after 402; the rate-limit helper uses the legacy envelope — error as a string, with the product-internal envelope inconsistency on record); api_test.sh gained garrison probe smoke checks (`/health/live` 200, `/health/ready` 200|503)

- **On record (this change unified the helper's direct-dispatch path; a parallel entry remains)**: `check_rate_limit_as_app_error` → `CrawlRsError::RateLimit::into_response` (the search/webhook handler paths) renders 429 slightly differently from the helper (no timestamp, an extra error.status, RetryAfter losing retry_after_seconds) — unifying the two renderers belongs to the `CrawlRsError` IntoResponse refactor and is registered as a separate engineering item; the crawl placeholder result row (status_code=0 + note) semantics are in API_REFERENCE's Get Crawl Results section

### Changed (Playwright stealth fingerprint consistency upgrade)

- **Filling server-verifiable fingerprint contradictions (converged to four layers after three-way review)**: launch switched to New Headless (`new_headless_mode()`) + `--disable-blink-features=AutomationControlled` + `--lang=en-US,en` (the only launch point in `playwright_pool.rs`); page setup upgraded from bare `set_user_agent` to a full `SetUserAgentOverrideParams` (UA + acceptLanguage + navigator.platform + userAgentMetadata); brands take `UaProfile.sec_ch_ua`'s per-version authoritative templates (including Edge/Samsung correct brands and GREASE, same source as the headers wreq sends — re-parsing the UA string would parse Edge as Google Chrome and contradict its `Edg/` marker); **dual-vocabulary split**: `navigator.platform` (Win32/MacIntel/iPhone/Linux armv8l) and `Sec-CH-UA-Platform` (Windows/macOS/…) are mapped separately by `build_ua_fingerprint`, never double-written with different values (the first version's double-writing of the raw UA was exactly the contradiction class to eliminate — caught by review and fixed); Android high-entropy fields follow UA facts (architecture=arm, model parsed from the UA, the `wv` WebView marker emptied); CriOS (WKWebView, no Client Hints capability) is excluded from `pick_chromium` candidates and its metadata is absent entirely — empty brand client hints are never sent; full_version_list is always absent (sent only when Accept-CH requests it — forging is more dangerous than absence). No new configuration entries
- **Measured baseline (test-results/antibot/)**: header echo evidence shows `Sec-CH-UA` changed from missing/HeadlessChrome to template brands (`Google Chrome`+`Chromium`+GREASE, versions consistent with the UA), and `Sec-CH-UA-Platform` uses the client-hints vocabulary (`macOS`, not the UA's raw `Mac OS X`); the anti-bot matrix nowsecure.nl/g2.com × js_rendering pass rate 0%→0% (both js/plain are explicitly classified `cloudflare challenge structure matched`; fallback semantics unchanged; Turnstile penetration is not promised — see the change's Deferred). The npr-plain smoke spot suffered CPU contention from a parallel build on the same machine, tripping reqwest's 5s MRT circuit breaker (curl direct test 1.1s normal) — evidence-environment noise, not a product regression

### Changed (same-author dependency family rc-line alignment)

- **dbnexus dependency family aligned to the rc.9 line**: removed dbnexus's deleted dead feature `config-toml` (empty since rc.6 — `= []` pulls no dependencies and gates no code; crawlrs's TOML parsing comes from the direct confers dependency and is unrelated; upstream cleanup at `../base/dbnexus@c59ae46`). After vecboost's (local patch) manifest moved to dbnexus rc.9 the lockfile re-resolved, and each same-author library landed on the newest satisfying version per the existing "minimum rc" semver requirements: dbnexus rc.9 / confers rc.10 (rc.8/rc.9 were yanked) / trait-kit rc.9 / inklog rc.9 / oxcache rc.8 / limiteron rc.7 / sdforge rc.8; garrison stays at rc.2. `cargo check` passed in one run with zero API breakage; full regression: clippy(default|full) 0 warnings, lib tests 6014+6184 passed, integration 152 passed, liveapi full-build chain green (the build path recovered for the first time since the vecboost manifest change). `Cargo.lock` synced into the repo

### Fixed (E2E coverage gap closure)

- **Live E2E coverage for 8 zero-coverage surfaces**: `api_test.sh` SDK assertions now hit the real mounted paths `/api/v1/sdk/*` (fixing hollow 404 passes and the outdated `http feature` comment; `(422)`/`(403)` mislabels aligned to the actual 400); `api_semantics_test.py` 37→45 scenarios — SDK semantics (search success/engine-degradation 500, empty query 400, scrape creation 2xx), `POST /v1/scrape/{id}/_cancel` (200 + `message` envelope + terminal-state polling, unknown id 404), `POST /v1/tasks/_cancel` batch cancel (`cancelled∪failed` full coverage + per-item terminal states), `expires_in_secs=1` short-lived key expiry 401, webhook signature creation (201 + `data.secret` echo, extra-field rejection, list redaction; SKIP when `--webhook-secret` is absent); `live-api-e2e.sh` injects and forwards `CRAWLRS__WEBHOOK__SECRET` and adds a final 402 gate (credits zeroed → `POST /v1/scrape` asserts 402 `QUOTA_EXCEEDED`); deleted the orphan `tests/e2e/test_scenarios.py` referenced by no suite. Note: the webhook extra-field rejection is actually 400 `VALIDATION_ERROR` (the handler maps it manually via `from_slice`), not the drafted 422 — the server's reality wins

### Breaking (auth-off security semantics restored — doc-audit defect #6)

- **auth-off protected routes restored to deny-by-default (401)** (`ee135748`): restored the v0.2.0-rc.1 tightened semantics (accidentally lost during the rc.2 route-assembly refactor with no record). With the `auth` feature off, protected routes return 401 by default; embedders must explicitly call `crawlrs::presentation::middleware::auth_middleware::allow_unauthenticated_protected()` to allow them — injecting an anonymous restricted identity (`ApiKeyScope::denied()` scope, no token_hash) with a one-time `error!` log declaring the downgrade. The default build (platform implies auth) is unaffected
- **Restored the `web-axum` build surface**: platform implies auth, so after rc.2 the auth-off degraded path (`default_identity_middleware` / `allow_unauthenticated_protected`) could not compile in the feature matrix (presentation hangs off platform → necessarily includes auth) — a decayed surface since web-axum's rc.1 removal. Now restored per the rc.1 mechanism: `--no-default-features --features web-axum,db-postgres --lib` builds/tests the auth-off semantics (gating expanded to presentation/bootstrap/workers/di/queue/infrastructure etc., with dual coverage of `CrawlRsError::IntoResponse` and the `Database` variant), and the auth-off middleware tests (default 401 / opt-in injecting the denied template identity / credential-free pass-through / invalid-credential ignored) run on this axis for the first time (5/5 passed)

### Added (doc-audit defect clearance #1-#5)

- **The `crawlrs migrate` subcommand** (#1, `3b98167a`): a first-class production schema-initialization entry point applying `migrations/*.sql` in order (via dbnexus's `auto-migrate` feature `DbPool::run_migrations`); the directory defaults to `./migrations`, overridable with `CRAWLRS_MIGRATIONS_DIR`
- **`POST /v1/scrape/{id}/_cancel` production route registration** (#2): the handler was fully implemented but only registered on the test router, so production returned 404 and single scrapes could not be canceled (only via the `POST /v1/tasks/_cancel` batch cancel); the route group registration + registration-probe test were added. It coexists with 0.2.0's `DELETE /v1/crawl/{id}`
- **Search configuration made effective** (#3): `search.default_engine` (the single-engine fallback path's default engine, must be registered or it warns and falls back to bing) and the `[search.engines] bing/baidu/sogou_enabled` registration switches (unregistered engines are unavailable to both smart aggregation and the single-engine path) were previously dead config with no code consumption; now consumed by `SearchClient::with_configured_engines`; default.toml / .env.example synced (removing the nonexistent google key); 4 new i18n log keys (en/zh)
- **`ScrapeOptionsDto` exposes `block_ads` / `block_media` / `needs_mllm`** (#4): the engine-level capabilities existed but the DTO did not expose them; wired through both the use-case and worker paths; `needs_mllm` is explicitly rejected with 422 at submission in builds without the `llm` feature (a task-level explicit failure on the worker side), no more silent degradation; `session_id` remains an internal field
- **forge_api registration surface brought into the build** (#5, `6d295bd7`): `presentation/forge_api` (the in-flight sdforge single-registration-surface migration) was previously undeclared in the module tree — not compiled, silently rotting with handler evolution. A new `forge-full` feature parking lot: inventory collects globally at link time, and the active state is mutually exclusive with the hand-written protected/v2 route groups (coexistence panics at startup with `Overlapping method route`), with route-equivalence anchoring tests (inventory probe + production-path subset + known-gap pins); the full switch requires a design decision on unified ordering of the three middleware families (protected/v2/sdk), to be implemented in a later change

### Added (i18n / Fluent integration)

- **42 user-visible strings and logs wired to Fluent** (`9fdcafeb`): strings/logs added or changed since the last version tag uniformly resolve via `src/i18n` (engines, handlers, repositories, mappers, observability, ssrf, the limiteron, garrison listener, di/id modules, utils, workers surfaces); en-US default with zh-CN keys aligned (existing published strings untouched)
- **IdModule config validation and auth-off degradation logs wired to Fluent** (`72ddb07c`): errors.ftl / logs.ftl gained 2 keys each (IdModule startup algorithm validation, the auth-off degraded-mode ops log), with the I18nBundle key registry maintained (en/zh bilingual)
- **IdModule and auth_middleware switched to tr_log output** (`3e8997bb`): the message content of the two algorithm validations and the auth-off allow/deny degradation logs is carried by the bilingual locale resources; the code no longer holds user-visible strings

### Added (feature absorption F1–F12)

- **Politeness AutoThrottle** (F1/F5, `1cc6a3c6`): HostDiscipline `current_interval` dynamic delay convergence + 429 exponential backoff + Crawl-delay dual clocks + stall detection; route_sequential gained 429 pass-through observation and stalled→rotate_proxy; Retry-After parsing extracted into the single `utils::retry_after` implementation (shared with the antibot classifier); the `[politeness]` config surface of 10 keys injected via DI
- **Session identity trio** (F2, `4b88673a`): `SessionIdentity` (proxy + UA/TLS profile + Cookie cache) and `SessionManager` (a DashMap affinity pool with error_score +1.0/−0.5 scoring, lazy retirement by uses/age, temporary-identity degradation when the pool is full); `[session]` config of 6 keys with `session.enabled` defaulting to false (equivalent to the current blind rotation when off); challenge pages/401/403/429 call mark_bad, 2xx calls mark_good
- **Sitemap streaming as a first-class request source + soft-404 fingerprints + seen warm-up deltas** (F3/F11, `aaec5d8b`): `SitemapFetcher::fetch_streaming` (quick-xml incremental parsing + mpsc backpressure) and `MapUseCase::execute_streaming` (sitemapindex one-level recursion, concurrency cap, max_urls guard), the `POST /v1/map` endpoint (gated by the `sitemap` feature); per-crawl `sitemap_seed` explicit opt-in + main-switch 422 rejection + depth=0 execution-side hookup; `Soft404Detector` content fingerprint learning/matching — 200 shell pages do not land results, preventing retry storms; `list_distinct_urls` keyset-paginated delta warm-up of the dedup set; `utils::glob` unified wildcard matching in a single implementation
- **Failure-scene snapshots + AIMD memory-pressure linkage** (F4/F10, `2c726cce`): `FailureSnapshotter` archives response scenes (`.html`/`.meta.json`/screenshot) deduped by error signature key (sha256(url|status|reason)[:30] + sanitized reason); the `ObservabilitySettings` config surface (an empty dir string = disabled); the antibot detection branch persists via spawn_blocking without blocking the worker; `AIMDController::on_memory_pressure` (Pressure freezes additive-increase / Critical CAS-halves) + TeamSemaphore broadcast, eliminating the contradiction of "gating pauses dispatch while AIMD raises concurrency"; plus an AIMD concurrency-target team gauge and increase/decrease debug logs (`5f71dd92`)
- **Playwright XHR/Fetch background response capture** (F6, `23494fda`): CDP Network dual event streams (ResponseReceived + LoadingFinished) with in-line body retrieval (avoiding Network.disable discarding buffers), glob allowlist filtering + max_bytes truncation + a max_responses cap, and four discard counters warn-reported at collect; `[engines.playwright]` config of 4 keys with `capture_xhr` defaulting to false (zero Network listeners, zero behavior change)
- **Extraction degradation-ladder quality gate + LLM token-window chunked parallel merge** (F7/F8, `985a56ca`): `QualityGateConfig` (word_count short-result promotion + length-ratio adjudication + prose hard gates for sentence_avg/link_density) wired into `ContentExtractionFacade`'s adjudicating try_extract_chain, with the `disabled()` constant keeping unwired call sites unchanged; a zero-dependency deterministic chunker (budget = (window − overhead − output) × chars_per_token ÷ 2, three-level boundary greedy splits, terminology pre-check short-circuit, JoinSet+Semaphore parallelism, explicit fallback to heuristics on merge-prompt failure); the `[llm.chunking]` eight-key config surface
- **Character-level extraction provenance + MLLM hybrid AX-tree snapshots** (F9/F12, `aa58626b`): provenance with three-layer alignment (greedy earliest monotonic + anchored sliding-window tightest LCS + coverage/density dual gates), `extract_data_with_provenance` attaches `_provenance` (off by default via `provenance_enabled`); the AX tree is captured via CDP `Accessibility.getFullAXTree` with pruning that folds unnamed connective nodes, and decision Click gains `ax_id` (located via DOM.resolveNode + Runtime.callFunctionOn(user_gesture)), falling back to the existing selector path on failure
- **Vision calls support custom OpenAI-compatible endpoints** (F12, `2fbca6f7`): when the vision model has the `openai:` prefix and `CRAWLRS__MLLM__VISION_API_BASE_URL` / `CRAWLRS__MLLM__VISION_API_KEY` are set, the OpenAI adapter binds and `ServiceTargetResolver` takes over endpoint and auth (stripping the `openai:` prefix keeps the bare model name; without a bound adapter_kind, genai sniffs unrecognized model names as Ollama and POST /api/chat to the gateway inevitably 404s); unconfigured, the provider's native routing applies and gemini/ollama semantics are unchanged
- **Doc sync** (`ed7f834a`, `3338341c`, `10338e4d`): README feature tables and the API docs synced with the F1–F12 feature absorption, redundant F1–F12 sections cleaned, comments aligned with the implementation
- **Live/acceptance test adaptations** (`b3128b3c`, `4d6976e5`, `eba14e86`, `673bc6d1`, `7d847df3`, `bab5d770`, `57d5c6d8`, `2457bdf4`): F12 AX-tree + Ollama vision-navigation live verification (`#[ignore]` gated, vision model env override `CRAWLRS_TEST_VISION_MODEL`), F4 scrolling-guard acceptance tests (max_total oldest-first eviction); fixed the construction adaptations/stale assertions/clippy violations exposed by the live gates, filled the missing fields in WorkerManagerDeps test construction, and unified garrison global-singleton cross-test mutual exclusion; added the live-stack test runner `scripts/run-live-test.sh`; fixed the e2e stage-failure PIPESTATUS judgment

### Added (engines and intelligence enhancements)

- **TLS fingerprint spoofing engine** (`engine-tls-fingerprint`): WreqEngine implements real JA3/JA4 fingerprint spoofing on BoringSSL, supporting Chrome/Firefox/Safari/Edge emulation
  - `UaProfile` gained a `tls_emulation` field, binding UA↔TLS fingerprint consistency
  - The `needs_tls_fingerprint` request parameter and Scrape API options field
  - The `[engines.tls_fingerprint]` config section
- **MLLM autonomous-navigation engine** (`engine-mllm`): a vision-LLM agentic loop for autonomous page navigation
  - The `MllmDecision` enum (Click/Scroll/Input/Wait/Extract/Done) + JSON parser
  - The `VisionAdapter` vision adapter (screenshot → vision-model request)
  - The `ActionExecutor` action executor (MLLM decision → CDP operations)
  - The `MllmEngine` struct + `ScraperEngine` trait implementation
  - The `[engines.mllm]` config section (vision_model/max_iterations/screenshot_quality/max_token_budget/mrt_seconds)
- **RAG-augmented extraction**: DOM semantic chunking + vector embedding + cosine-similarity retrieval + precise LLM extraction
  - `SemanticChunker`: chunks along DOM boundaries (article/section/div/table), never splitting tables/lists
  - `EmbeddingProvider` trait + `VectorStore` in-memory vector store
  - `RagExtractionStrategy` as the third extraction mode (`extract_with_rag`)
- **Knowledge-graph coverage-aware crawling**: `KnowledgeGraphAccumulator` + Chao1 coverage estimation + structural-hole detection
  - `KgBoostScorer` implements the `UrlScorer` trait, giving URLs in structural-hole regions higher priority
  - `StopCondition` gained the `CoverageReached` stop reason + the `min_coverage` condition
- **DRL adaptive crawl policies**: `DrlPolicy` ONNX inference + `HeuristicPolicy` heuristic degradation
  - `CrawlState`/`CrawlAction` state-action definitions
  - `DrlConfig` config switches (off by default)
  - The Python training script `scripts/drl/train_policy.py` (gymnasium + stable-baselines3 + ONNX export)
- **Observability enhancements**: 5 new Prometheus metrics
  - `crawlrs_queue_depth` Gauge (task queue depth)
  - `crawlrs_engine_success_total` Counter (engine success/failure counts)
  - `crawlrs_engine_duration_seconds` Histogram (engine request duration distribution)
  - `crawlrs_cache_hit_total` Counter (cache hits/misses)
  - `crawlrs_webhook_delivery_total` Counter (webhook delivery success/failure)
- **Benchmark extensions**: URL validation / SSRF detection / RegexCache / engine-routing benchmarks (`benches/benchmark.rs`)

### Added (engine health monitoring, garrison-ized)

- **Engine health monitoring absorbs garrison's `HealthCheck`/`HealthRegistry`** (auth-gated, `6c154273`): the probe logic was rewritten as `EngineHealthCheck` registered into `HealthRegistry`, and the `/health/live` + `/health/ready` probe routes merged into the existing Router via `health_routes` (readiness aggregates Unhealthy → 503, with stream-removal semantics carried by the library; per-probe timeouts are aggregated as Unhealthy under the library's check_timeout guard); the aggregation scale matches the pre-migration `get_aggregate_status`; auth-off builds keep a lean fallback without pulling the whole garrison suite in for the health surface. See Changed → selfdev-lib-adoption-r1 S2

### Changed (selfdev-lib-adoption-r1: 5 in-house implementations sunk into the in-house library matrix, 1 judged non-absorbable)

After item-by-item review of the overlap between crawlrs's in-house implementations and the in-house libraries (limiteron/garrison/oxcache/sdforge), the following were absorbed; library-side gaps were registered per docs/ARCHITECTURE.md "In-House Library Reverse Requirements":

- **S3 error mechanism sunk into sdforge** (`src/common/error.rs`): `status_code`'s hand-written 17 arms changed to "variant → `ApiError` normalization + `unified::http_status_for` delegation" — 14 variants' HTTP status codes are maintained in a single place by the library mapping table, with mapping results equal item-by-item to the pre-migration table (pinned by tests); 502/504 (Network/Engine/Timeout) remain outside sdforge's coverage set, kept explicitly and registered as a gap; `sanitize_engine_error` regex redaction is retained as a domain-specific case, with the Internal/ServiceUnavailable full-masking semantics aligned to `ApiError::sanitized_message` (test-asserted); i18n keeps crawlrs's `I18nBundle` (sdforge's `TranslationStore` supports only JSON loading and the fallback chain does not match — the gap is registered)
- **S5 DNS single-flight folded into oxcache** (`src/infrastructure/dns/dns_cache.rs`): `lookup_host`'s miss/single-flight/write-back orchestration folded into `Cache::get_or_with_ttl`'s built-in leader/follower, deleting the self-managed `in_flight` DashMap + `FlightSlot` + `resolve_single_flight` (~120 lines); per-entry TTL takes the resolver-recorded remaining validity (`valid_until`, capped by `default_ttl`), falling back to `default_ttl` when no record exists; the hit/miss metric scale is unchanged
- **S4 Bloom filter replaced by oxcache's `BloomFilter`** (`src/utils/dedup/bloom.rs`): deleted the mmap(MAP_HUGETLB)→mmap→heap three-level degradation chain and `unsafe impl Send/Sync` (~384 lines) in exchange for `Arc<RwLock>` thread safety and standard FPR parameterization (`new(1_000_000, 0.01)`, the false-positive rate as a constant); **behavior change: the mmap huge-page residency strategy is removed — at large capacities memory residency moves from 2MiB huge-page mmap to heap allocation (1M @1% ≈ 1.2MB bitmap)**; the libc target dependency went away with the deletion
- **S2 engine health absorbed into garrison** (`src/engines/health_monitor.rs`, gated by the `auth` feature): the probe logic was rewritten as `EngineHealthCheck` (impl garrison `HealthCheck`) registered into `HealthRegistry`, with the `/health/live` + `/health/ready` probe routes merged into the existing Router via `axum_routes::health_routes` (readiness aggregates Unhealthy → 503, K8s stream-removal semantics carried by the library; per-probe timeouts aggregate as Unhealthy under the library's check_timeout guard); the aggregation scale is word-identical to the pre-migration `get_aggregate_status` (test-reconciled); auth-off builds keep a lean fallback without pulling in the whole garrison suite for the health surface
- **S6 circuit-breaker thin wrapper over limiteron's `ManualCircuitBreaker`** (`src/engines/circuit_breaker.rs`): the per-engine keyed pool is carried by crawlrs's `DashMap` (the library has no keyed breaker); `record_success`/`record_failure` became async library calls (five router call sites made async), `is_open` keeps a synchronous signature — after recording, the state is mirrored to an `AtomicU8` (+Open instant), read synchronously by the selector; `CircuitStats` and engine-labeled metrics remain crawlrs instrumentation; **behavior change: sliding-window absolute failure counting → the library's cumulative-ratio triggering (`total ≥ failure_threshold` and failure ratio ≥ 0.5) — N consecutive failures still always trip, with raised sensitivity under mixed traffic**; the config mapping `min_requests = failure_threshold` and `half_open_max_successes = 1` preserves the "one success closes it" semantics
- **S1 team semaphore pool: judged non-absorbable** (`src/domain/services/team_semaphore.rs`): review found the library's `ConcurrencyLimiter` has no public non-blocking entry (`try_acquire` exists only in the library's `#[cfg(test)]` private impl), keyed-pool permits cannot escape-borrow from `&self` (a concurrent map handing out `&'a V` must hold a local guard — a self-referential structure is unconstructible in safe Rust), and the manager has no injection entry — Fixed mode keeps the crawlrs implementation (zero changes to signatures/callers), registered as a library-side gap like S6 for a second convergence
- Verification: `cargo test --features default --lib` 6013 passed / 0 failed; `cargo test --features auth --lib` 6013 passed; `cargo check --features platform,auth,rate-limit,openapi` passed; `cargo clippy --features default|agent-lib --lib` zero warnings; the default (auth implied via platform) / auth-only / agent-lib (auth-off) three build surfaces all green

### Changed (in-house library dependency feature expansion)

- limiteron features unchanged (`manager` was originally to be added per the plan, but after S1 was judged non-absorbable it had no consumers and was withdrawn per feature-hygiene discipline); oxcache features gained `bloom` (a URL-dedup Bloom filter replacing the in-house MmapBloom); the libc target dependency was removed (the MmapBloom mmap surface went away with S4 — zero references repo-wide)

### Changed (dependency upgrades and feature-explicitness cleanup)

- **In-house rc-chain follow-up**: dbnexus 0.6.0-rc.5→rc.6, confers 0.6.0-rc.5→rc.6, inklog 0.3.0-rc.5→rc.6 (rc.7's fast-masking panics on fancy_regex BacktrackLimitExceeded in log lines — deliberately not upgraded, the lockfile pins rc.6, see the Cargo.toml inklog comment), trait-kit 0.5.0-rc.6→rc.7, oxcache 0.5.0-rc.5→rc.6, limiteron 0.3.0-rc.5→rc.6, sdforge 0.5.0-rc.5→rc.6; garrison 0.9.0-rc.2 is already the latest rc and stays; vecboost 0.3.0-rc.1 via `[patch.crates-io]` pointed at a local adaptation of confers rc.6 (`../vecboost`), removable once upstream ships rc.2 (see Fixed → rag feature compile fix)
- **Third-party dependencies raised to latest stable**: quick-xml 0.41→0.42 (`LocalName::as_ref` now returns `&str`, `xml10_content` returns `Cow<str>` — adapted in map_use_case sitemap parsing), rig-core 0.42→0.44 (0.43's new wire/driver architecture: rig_embedding_provider rewritten to `OpenAIConfig::new` + `client.embedding(model, Some(ndims)).call(texts)`, adapted to the explicit ndims response-width validation introduced in 0.43 and continued in 0.44 — width mismatches error explicitly rather than returning silently), uuid 1.26→1.27
- **Full feature explicitness**: `[workspace.dependencies]` + all direct dependencies + dev-dependencies + examples turned off default features and declared minimal feature sets; substantive trims: axum dropped form/matched-path/original-uri/tower-log/tracing (zero references repo-wide), sysinfo kept only system, genai kept only rustls-tls, futures dropped executor, criterion dropped rayon/plotters, chromiumoxide made bytes explicit, base64 stripped simd-unsafe, aho-corasick made std+perf-literal explicit, testcontainers/testcontainers-modules stripped default ring; chrono explicitly gained clock+std (previously `Utc::now` relied implicitly on the dependency graph's feature union); plotters, tracing-futures, as-any, and the sysinfo 0.38 dual version exited the dependency tree
- **CI actions upgraded**: codecov-action v7.0.0→v7.1.1, setup-rust-toolchain v1.17.0→v2.0.0, codeql-action v4.38.0→v4.38.2, action-gh-release v3.0.2→v3.0.3, taiki-e/install-action→v2.87.26, dtolnay/rust-toolchain master SHA refreshed; checkout v7.0.1 / rust-cache v2.9.2 / cargo-deny-action v2.1.1 were already latest and untouched
- **New feature axes and library adaptations**: `web-axum` (the auth-off equivalent build surface), `forge-full` (the forge registration-surface parking lot), dbnexus `auto-migrate` (a prerequisite of the migrate subcommand), `[patch.crates-io] vecboost` (the local fixed version); dbnexus rc.6 added `DbError` variants (RateLimited/Unsupported/VersionConflict) with matching arms added at the repository layer, and inklog rc.6's `rate_limit`/`sampling` fields completed the telemetry assembly
- **Pin evidence maintained**: wreq 6.0.0-rc.31 (all 5.x stable versions yanked; rc.31 remains the latest usable), testcontainers 0.27 (testcontainers-modules 0.15.0 hard-depends on ^0.27.0 — cargo outdated forcing 0.28 conflicts), utoipa 5.5 (the in-house sdforge 0.5.0-rc.6 optional dependency ^5.5 with `pub use utoipa` re-export — moving to 6.0 creates dual-utoipa coexistence triggering E0659 ambiguity and breaks forge macro-generated code)
- **Known items**: RUSTSEC-2023-0071 (rsa) / 2024-0436 (paste) / 2025-0119 (number_prefix) keep deny.toml's existing ignores and CI precondition checks (rsa converged further with the jsonwebtoken dead-dependency removal — see Removed)
- Verification: `cargo check` (no-default / default / full / platform / openapi / engine-tls-fingerprint) all green; `cargo clippy --features default|full -- -D warnings` all green; `cargo fmt --all -- --check` passed; `cargo check --features full,test-mocks --tests` compiled all test targets; `cargo test --lib` (standard 6112 / full 6163) 0 failures; rag-remote embedding unit tests (wiremock contract) 4/4 passed; `cargo deny check advisories` ok

### Changed (ID system fully migrated to NebulaId)

- **Unified ID carrier**: added `domain::id::Id` (internally `u128`, wrapping `nebulaid::core::Id` semantics), the project-wide replacement for `uuid::Uuid`; completed `Copy/Hash/Default/Ord/FromStr` and serde as UUID strings (bypassing the orphan rule, keeping the API JSON contract unchanged)
- **Generation engine switched**: all project ID generation goes through the local `../NebulaId` (`nebulaid` path dependency, `default-features=false, features=["sdk"]`). The default algorithm is `uuid_v8` (generates UUID-compatible values → zero changes to DB `UUID` columns and the API JSON contract, no schema migration); `snowflake` is a configurable alternative (`[id].algorithm`)
- **Generation entries**: `IdService::next_id().await` (async, injected via DI) + the synchronous `Id::generate()` (a sync facade over a process-level prefetch pool + background refill, covering contexts that cannot await such as `impl Default`/struct literals/sync mappers — a direct replacement for `Uuid::new_v4()`)
- **Assembly**: `IdModule` builds the `NebulaIdKit` at startup and injects it into `CrawlRsState` (both API and worker personalities share the same assembly point); configuration gained the `[id]` section (`CRAWLRS__ID__*`)
- **Persistence boundary**: the entity layer keeps `uuid::Uuid`/`UUID` columns unchanged; only the repository/mapper boundary converts `Id↔Uuid`; `impl From<Id> for Value` makes `Id` a first-class sea-orm query value
- **Algorithm-resolution assertion alignment**: after NebulaId rc.3 reversed its algorithm aliases, `IdModule::resolve_algorithm`'s assertions were aligned in sync (`106f7592`)
- **Test wrap-up**: external integration tests and multi-binary uniqueness adaptation (`edf68f7e`); the CWE-798 regression test is under Security (`e466b1b6`)
- Verification: `cargo check --lib/--bins` (default + full) all green; `cargo clippy -- -D warnings` (default + full) all green; `cargo fmt --all -- --check` passed; `cargo deny check` (advisories/bans/licenses/sources) all green; `cargo test --lib --features default` 5981 passed / 0 failed; external `tests/` and `benches/` compile all green

### Changed (code architecture)

- **Code architecture optimization**:
  - `router.rs` split into the `engine_selector` / `route_sequential` / `route_race` submodules
  - `scrape_worker.rs` split into `scrape_task` / `crawl_task` / `extract_task` + `builder` + `deps`
  - `ScrapeWorkerDeps` made a parameter object (eliminating the 16-parameter function signature)
  - `webhook_service.rs` split out `webhook/management.rs`
- **Test code extraction**: inline tests in 5 modules extracted to standalone files (scrape_worker/router/engine_client/webhook_service/task_repo)
- **Unified test mocks**: `tests/common/mocks/` shares MockTaskRepository/MockScraperEngine/MockWebhookService etc.

### Changed

- `/v1/search` without an `engine` parameter changed from "single bing engine" to smart aggregation (see the Search API in docs/API_REFERENCE.md)

### Removed

- **The jsonwebtoken dead dependency** (`bd44ea14`): the jsonwebtoken declared in Cargo.toml had zero references in src (audited 2026-10-03); its `rust_crypto` backend unconditionally carried rsa 0.9.10 (RUSTSEC-2023-0071, unpatched), pushing rsa into every consumer's dependency tree. JWT capability comes with garrison (the auth feature) and is unaffected; crawlrs's own dependency tree (default includes auth) converged its residual rsa path to the single garrison one, and agent-lib consumers (default-features=false builds) lost rsa entirely. The dependency security baseline in docs/SECURITY.md was synced

### Fixed (fix-audit-mediums-r3: deferred items landed + review LOW wrap-up)

- **HostDiscipline per-host discipline component** (`utils/host_discipline.rs`): per-host scheduled-time rate limiting (concurrent same-host adjacent requests spaced ≥ min_interval), 429 cooldown (parsing the response Retry-After header, default 60s), automatic escalation to the TLS fingerprint engine (`needs_tls_fingerprint`) when bare 403 streaks reach the threshold; the classifier's `Detection` gained `retry_after_secs`
- **Per-task BrowserContext isolation** (on by default, the `context_isolation` switch): one-shot resolution of cookie/localStorage/indexedDB cross-task crosstalk; the TabPool reuse path is retained as an escape hatch
- **DNS per-key single-flight**: concurrent misses on the same host allow only one actual resolution, and same-batch waiters share the result (failures are shared too, eliminating the K×-timeout failure-tail amplification)
- **WebGL/canvas stealth** (`webgl_overrider.js`): WebGL vendor/renderer fingerprint override (Proxy wrapper resists toString detection) + canvas read noise; toDataURL applies only to canvases already holding a 2d context, without materializing the context type
- **robots fail-closed switch** (`robots.fail_closed`, default false keeps fail-open) + request-level `respect_robots` override on `/v1/scrape`
- **exa/tavily conditional registration**: once `CRAWLRS__SEARCH__EXA__API_KEY` / `CRAWLRS__SEARCH__TAVILY__API_KEY` are configured, they auto-register into SearchClient's single-engine path (live joint debugging pending real keys)
- **baidu mu/data-url link unwrapping**: real URLs participate in cross-engine dedup; when attributes are missing, the relay link is kept (302 following was ruled out)
- **Incremental-scrape conditional requests** (scoped slices, `options.conditional_request`): re-runs of the same task automatically carry If-None-Match / If-Modified-Since; 304 reuses the historical result content (marked `x-crawlrs-not-modified: true`), with no schema change
- **HostDiscipline lazy sweep**: a 60s interval reclaims neutral entries across the whole table, fixing unbounded memory growth in the append-only state

### Fixed

- DNS single-flight shares results with same-batch waiters (take→clone: previously a second waiter reading an empty slot would re-resolve)

### Fixed (rag feature compile fix + migration prerequisite feature)

- **`--features rag` restored to compilable** (`6d295bd7`): vecboost 0.3.0-rc.1 and confers 0.6.0-rc.6 had mismatched crypto APIs (`derive_field_key` returning `Zeroizing<[u8; 32]>`, `decrypt` returning `Zeroizing<Vec<u8>>`), making this feature combination uncompilable; `vecboost_provider` adapted to ModelConfig's new fields after confers rc.6; the first-class local vecboost repo adapted to rc.6 (commit 6438c1d in `../vecboost` + clearing the hf-hub non-http build warning), with crawlrs pointing at the local fixed version via `[patch.crates-io]` (Cargo.toml `[patch.crates-io] vecboost = { path = "../vecboost" }`), removable once upstream ships rc.2
- The `dbnexus` dependency gained the `auto-migrate` feature (a prerequisite of the migrate subcommand, purely additive over `migration`)

### Fixed (LLM live extraction path wiring)

- **Chunking/provenance wired into the live extraction path** (`ae1a3bb0`): LLM chunking and character-level provenance connected to the live extraction path, with direct max_tokens capping and configurable timeouts

### Fixed (fix-audit-mediums-r2: 7 audit-medium improvements + 9 independent-review fixes)

- **smart fan-out per-engine timeouts**: a single hung engine degrades as a failure (previously route retries could stack far beyond 30s and drag down the whole aggregation)
- **/v1/search engine-layer locale/country support**: the bing engine layer supports mkt (lang+country both present) + Accept-Language linkage and `first=` page derivation (commit 8e8ca660). **The service-layer bridge is not yet wired**: `perform_search`'s `lang`/`country` parameters are currently ignored and API-supplied values have no effect (see the search section of docs/USER_GUIDE.md and the parameter table in docs/API_REFERENCE.md)
- **Cross-engine title SimHash dedup**: RRF merged only by URL; cross-engine duplicates with identical content but different URLs are now merged as a fallback
- **Outbound per-engine minimum-interval rate limiting** (bing 1s / baidu 2s / sogou 2s): prevents high-frequency aggregation from getting the search engines themselves banned
- **URL normalization strips known tracking parameters** (utm_* / fbclid / gclid / msclkid / dclid / twclid / mc_eid / _ga): marketing-variant URLs are no longer scraped repeatedly
- **robots matching carries query**: rules like `Disallow: /*?s=` work again
- **Host anti-bot memory**: hosts with detected challenge pages route directly as needs_js within a TTL (10 minutes), so HTTP engines stop hitting the wall every time; expired entries are re-probed automatically
- **Independent review fixes**: restored the `_abck` test and comments damaged by the typos hook; tab returns no longer call browser-level cookie clearing (preventing concurrent tabs from destroying each other's sessions); DomStable over-budget degrades to Ok with wait_for clamped to 10s (previously valid API input failed deterministically); the DFS key changed to `priority-(depth+1)` (no longer collapsing back to BFS at the default priority=0); when all race candidates are challenged, one re-dispatch round runs by needs_js while preserving the AntiBotDetected diagnostic; Bing challenge detection moved after parsing (preventing normal result pages from being misjudged as Captcha); trafilatura body HTML relativizes links against the page URL; dedup exempts baidu.com/link from query stripping (preventing all results from collapsing into one key); the PlainText degradation branch minimally escapes leading metacharacters

### Fixed (fix-audit-defects-r1: 16 confirmed defects from the 2026-09-24 interview-question audit)

Anti-bot / HTTP layer:
- **Proxies silently ineffective for HTTPS** (high severity): all proxy construction points `Proxy::http()` → `Proxy::all()` — the original intercepted only `http://`, so https targets bypassed the proxy and connected directly (real IP leak)
- **wreq TLS emulation landed**: `emulation_provider` changed from "ignore input, always default" to real Chromium-family emulation (browser default headers + Chrome H2 SETTINGS frame parameters); WreqEngine's UA candidate set filtered to Chromium-family profiles, eliminating the "Firefox UA + Chromium JA4" fingerprint contradiction
- **wreq sessions/compression**: enabled cookies/gzip/brotli/zstd/charset/stream features + `cookie_store(true)`; added a response-body size cap on reads (matching the reqwest-side anti-OOM double guard)
- **Race mode bypassed anti-bot detection**: the winning response must now pass antibot classification; challenge pages are voided and the race continues over remaining candidates (aligned with the sequential mode's protection semantics)
- **The `_back\b` regex falsely flagged Akamai**: replaced with the real markers `_abck`/`ak_bmsc`/`bm_sv` — normal pages containing `go_back()` etc. no longer get needlessly escalated to the browser

Dedup / Bloom filter:
- **Bloom false positives permanently skipped URLs** (high severity): URLs the DB confirmed as "absent" were previously misjudged and dropped by a second `check_and_insert` (the `to_db_enqueue` branch was always empty); now changed to unconditional insert + enqueue
- **Zero dedup-state persistence**: at worker startup, distinct URLs are paged from the tasks table (5000/page) to warm the Bloom filter — restarts no longer re-crawl the whole site
- **No Bloom capacity scaling**: at load factor ≥90% the Bloom filter is rebuilt at 2× capacity from the authoritative set (error rate returns to target, no false negatives)

Meta-search:
- **SmartSearchEngine wired to the production API**: `/v1/search` with no `engine` parameter (or `"smart"`) now takes the multi-engine fan-out + RRF aggregation (previously the aggregation layer was built but never consumed by the router); explicit engine names take the single-engine path; unknown engine names return 400 rather than silently substituting bing
- **RRF fusion scale imbalance**: RRF and relevance components are min-max normalized within the result set before weighting — multi-engine consensus signals are no longer drowned by a single engine's relevance
- **Bing CAPTCHA detection**: challenge markers (bnp.bing.com etc.) are detected before parsing, explicitly returning `Captcha` instead of a silent "0 results, success"
- **Bing tracking-link unwrapping wired**: `decode_bing_url` was previously dead code; it now unwraps `bing.com/ck/a` redirects uniformly at the parse exit — cross-engine dedup/aggregation now applies to Bing results

Content processing / browser engine:
- **`only_main_content` Markdown structural degradation** (high severity): body extraction gained an `html` output (trafilatura `include_links: true`), and Markdown conversion prefers the structured body HTML — link/heading structure is no longer lost; without structured HTML it degrades explicitly (warn log)
- **DFS crawl strategy semantics inverted**: sub-link priority changed from `+1` to `−1` (deeper links dequeue first under priority ASC) — the previous DFS was actually BFS
- **stealth injection lost after navigation** (high severity): switched to CDP `Page.addScriptToEvaluateOnNewDocument`, registering for all subsequent documents — previously `page.evaluate` applied only to about:blank, making the `navigator.webdriver` override useless
- **Fake NetworkIdle**: `wait_network_idle` changed to CDP network-event-driven in-flight counting + quiet window (previously a fixed 500ms sleep); CDP failures fail open to sleep
- **`wait_for` API wiring**: the worker path previously hard-coded `None`, discarding caller parameters; now mapped to the `DomStable` wait strategy (consistent with the use-case layer)
- **Pool path argv injection + tab session crosstalk**: pool startup proxy URL validation moved earlier (reusing `validate_proxy_url`, fail-fast); Playwright UA takes only Chromium-family profiles; cookies are cleared when tabs are returned

### Fixed (cleanup)

- Deleted the `TextEncodingProcessor` dead code (the unused struct marked `#[allow(dead_code)]`)
- Deleted the `make_queue()` test helper (clippy never-used warning)

### Security

- **SSRF bypass wired** (`1ee28a93`): the `CRAWLRS_DISABLE_SSRF_PROTECTION` switch constant and the config_validator startup warning already existed, but `SsrfValidator::validate` still intercepted by host blacklist + intranet rules, making the switch a no-op (live-probe joint debugging of a local LLM endpoint could not pass). This change wires the bypass before Step 5's host blacklist — DNS is still resolved to preserve the ValidatedUrl / engine DNS-pin shape, only the blacklist and intranet rules are skipped; the redirect policy degrades to ordinary following accordingly; the process warns once, and non-dev/test environments get a prominent config_validator startup warning
- **CWE-798 regression test** (`e466b1b6`): the historically hard-coded default webhook secret (`default-webhook-secret`) configured verbatim into production is treated as unset and must block startup (a config_validator.rs regression case pins this branch; previously only empty/non-empty strings were tested)

## [0.2.0-rc.2] - 2026-09-22

### Changed
- **In-house dependencies fully switched to the crates.io registry**: confers/inklog/dbnexus/sdforge/oxcache/limiteron → 0.x.0-rc.5, trait-kit → 0.5.0-rc.6, garrison → 0.9.0-rc.2, vecboost → 0.3.0-rc.1; removed all `../base`, `../garrison` cross-repo path dependencies and the `[patch.crates-io]` section, so CI no longer depends on sibling-repo checkouts
- Merged the diverged github/main dual-remote history (docs dual-remote alignment; the BDD acceptance-line material is preserved in history)

### Fixed
- `audit_service` error mapping: the `RepositoryError::DatabaseError` dual state (platform rebuilds `DbErr::Custom`, the lightweight surface uses `Database(String)`), preserving the `Database` variant semantics
- i18n locale test environment race (`CRAWLRS_LANG` process-level env serialized via ENV_MUTEX + explicit cleanup)
- 88 rustdoc 1.98 strict-mode issues (bare links to code font/full paths, lazy continuation-line indentation, bare-generic HTML escaping)
- clippy 1.98: explicit `result_large_err` exemption, `cmp_owned` elimination

## [0.2.0-rc.1] - 2026-08-26

### Added
- The `web-axum` feature: the auth-off web middleware build surface (platform implies auth, which made this path unbuildable in the original feature matrix; now it builds/tests independently)
- The `allow_unauthenticated_protected()` explicit opt-in (the auth-off surface)

### Changed
- **auth-off security semantics tightened**: protected routes deny by default (401); the `DEFAULT_IDENTITY_TOKEN_HASH` placeholder hash and the fixed full-permission identity injection were removed; opt-in injects an anonymous restricted identity (`denied()` scope, no token_hash)
- The `NoopRateLimitingService`/`NoopWebhookService` assembly logs upgraded to one-time error level (making the security downgrade explicit)
- SDK route tests consolidated to the unit level (semantically duplicated cases between tests/sdk_api_test.rs and src/presentation/sdk/tests.rs removed)
- The `wreq` dependency declared as a `0.16` placeholder (upstream incident: all of 5.x yanked and the 6.0.0-rc builds broken; enabling `engine-tls-fingerprint` requires adapting to the 0.16 API)

### Fixed
- `exe_path` cross-platform naming (the Linux `_exe_path` reference was wrong)
- Missing test construction for the three new `WorkerManagerDeps` fields (minimal test doubles added)
- clippy `-D warnings`: result-large-err / struct update no-op / unused variable
- rustdoc link debt (crate-level lint declarations)
- cargo-deny: lru upgraded to 0.18.2; paste transitive-dependency exemption registered (RUSTSEC-2024-0436/2026-0258)

### Breaking
- Under auth-off, protected routes return 401 by default (embedders that relied on unauthenticated full access must explicitly opt in; note: this tightening was briefly lost during the rc.2 assembly refactor and restored in Unreleased — see Unreleased → Breaking)

## [0.2.0-legacy] - 2026-07-29

> Historical section: this version was only ever published as a GitHub Release (the tag was re-made on 2026-11-15 to point at the official release) and never hit crates.io. The records below are kept for traceability.

### Added

- Garrison RBAC integration: uses garrison's `ApiKeyHandler::verify_with_namespace` to directly obtain `login_id` and `scopes`, deprecating the `GarrisonUtil::check_api_key` + session-lookup chain (resolving the login_id=None 401 caused by missing sessions under garrison's in-memory DAO)
- The `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` environment variable: injects the bootstrap admin key at service startup, writing it into both the garrison DAO and the crawlrs `api_keys` table + initializing the team, resolving cross-process key loss under garrison's in-memory DAO
- Axum middleware chain calibration: in `create_v2_routes_with_state`, `.layer(team_semaphore_middleware)` is applied before `.layer(auth_middleware_inner)` (later .layer = outer = runs first), ensuring `AuthState.team_id` is injected before the semaphore extracts it
- `team_semaphore_middleware`: `extract_team_id` reads `team_id` from `AuthState` (replacing the raw `Uuid` Extension read), fixing the v2 route 401
- API endpoint `DELETE /v1/crawl/{id}` (cancel a crawl); `POST /v1/scrape/{id}/_cancel` left the production routes at this stage (re-registered later in Unreleased — see Unreleased → Added, doc-audit defect #2)
- In-house dependency versions unified (shared rc.43 chain): confers 0.6.0-rc.2 / dbnexus 0.6.0-rc.2 / oxcache 0.5.0-rc.2 / trait-kit 0.5.0-rc.2 / sdforge 0.5.0-rc.2 / inklog 0.3.0-rc.2 / limiteron 0.3.0-rc.2 / garrison 0.9.0-rc.1

### Changed

- Authentication failure path: all protected routes no longer go through garrison's `TokenSession` session lookup (because garrison's `check_api_key` does not create sessions)
- The SDK router's (sdforge 0.5) auth middleware uniformly uses `from_fn_with_state(db_pool, auth_middleware_inner)`, consistent with the protected/v2 trio
- The public endpoint `PUBLIC_ENDPOINTS` comparison normalizes trailing slashes (`/health/` also matches)

### Fixed

- v2 route 401 Unauthorized: the combined root causes — (a) team_semaphore ran before auth and (b) team_id was read from the raw `Uuid` Extension rather than `AuthState`
- garrison `INVALID_TOKEN`: keys written by the gen_admin_key tool are invisible in another process (in-memory DAO process isolation), changed to injecting the bootstrap key from an environment variable
- `create_protected_routes_with_state` decoupled from the parallel `routes()` table in routes/handlers.rs: the current API treats create_protected_routes_with_state / create_v2_routes_with_state as authoritative (no backward compatibility)

### Security

- Brute-force protection (CWE-307): garrison IP-level failure counting and lockout continue to apply to all protected requests
- auth_middleware_inner's cache, key_expiration, and scope validations all remain in strict mode with no degraded branches

## [0.1.0] - 2026-07-22

### Added

- Enterprise-grade web scraping and crawling platform built with Rust
- Multi-engine support: Reqwest (HTTP), Playwright (JS rendering via chromiumoxide), FlareSolverr, WreqEngine (TLS fingerprint), MllmEngine (Vision LLM navigation)
- Search engine aggregation (Google, Bing, Baidu, Sogou)
- Garrison-based authentication (RBAC + JWT + firewall-bruteforce + audit-log)
- API key scope system (Read / Write / Admin) with team isolation
- Credits-based billing system for search/scrape/crawl/extract operations
- Geographic restriction enforcement (country allow/block lists, IP whitelist)
- Comprehensive SSRF protection with DNS resolution validation
- Task queue with priority scheduling and retry support
- Webhook delivery system for event notifications
- Audit logging for all API operations
- Rate limiting with circuit breaker support (limiteron)
- LRU cache for API key validation (TTL 120s, capacity 10000)
- Extraction engine with LLM support (genai integration)
- Admin CLI tools for credits management

### Security

- Constant-time comparison for HMAC signature verification
- SSRF validation on all URL-accepting endpoints (scrape/crawl/extract)
- Proxy URL SSRF validation in search and crawl paths
- IDOR protection on audit logs endpoint (CWE-862)
- IP-based brute-force lockout (5 failures → 15min lockout)

[0.2.0]: https://github.com/Kirky-X/crawlrs/releases/tag/v0.2.0
