# 🔒 Crawlrs Security

crawlrs treats security as a core design goal of the platform. This document covers the security policy, vulnerability reporting process, built-in security mechanisms, supply-chain gates, and the production hardening checklist. All content is based on the repository's actual code and configuration (`deny.toml`, `.github/workflows/`, `src/presentation/`, the garrison integration).

## 📋 Table of Contents

- [📌 Supported Versions](#-supported-versions)
- [🚨 Vulnerability Reporting](#-vulnerability-reporting)
- [🛡️ Security Design Overview](#️-security-design-overview)
- [⛓️ Supply Chain and Gates](#️-supply-chain-and-gates)
- [🏭 Production Hardening Checklist](#-production-hardening-checklist)
- [🧪 Security Test Coverage](#-security-test-coverage)

---

## 📌 Supported Versions

| Version | Status | Notes |
|------|------|------|
| 0.2.x | ✅ supported | current release line (including Unreleased changes) |
| 0.1.0 | ❌ unmaintained | the legacy API Key system was invalidated by the 0.2.0 authentication migration; upgrade promptly |

### Minimum Supported Rust Version (MSRV)

`Cargo.toml` declares `rust-version = "1.97.1"` (locked in sync across the workspace). Security fixes are released only on supported version lines.

### Dependency Security Baseline

- `cargo deny check` runs on every CI execution: advisories (`yanked = "deny"`), license allowlist, bans (`wildcards = "deny"`), source verification (crates.io only).
- `jsonwebtoken` has no direct declaration (a dead dependency with zero src references was removed on 2026-10-03); JWT capability comes with garrison (the `auth` feature), with the minimal `rust_crypto` backend.
- The TLS fingerprint engine's `wreq-util` (GPL-3.0) is deliberately excluded: `TlsEmulation` maps this project's own enum onto the Apache-2.0 wreq `EmulationProvider` API.

---

## 🚨 Vulnerability Reporting

### How to Report

**Do not report security vulnerabilities through public GitHub Issues.** Prefer GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/policy) private disclosure (repo page **Security → Report a vulnerability**); only if that channel is unavailable, email [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com) (contact: Kirky.X).

### What to Include

Where possible, include: affected versions and feature combination (feature flags), reproduction steps or a PoC, impact assessment (confidentiality/integrity/availability), and known mitigations.

### Our Commitment

- We will stay in contact with the reporter during the fix; the disclosure timeline is negotiated with the reporter.
- Fixes ship with the next supported release and are recorded in the Security section of `CHANGELOG.md` (e.g. the brute-force protection hardening in 0.2.0).

---

## 🛡️ Security Design Overview

### Authentication and Authorization (handled by garrison)

Since 0.2.0, the authentication engine is provided by **garrison v0.9** (the `auth` feature, implying `dep:garrison`):

- Bearer token format `garrison_key_id.garrison_secret`; the plaintext key is returned only once in the issuance response.
- JWTs are issued with `CRAWLRS__AUTH__JWT_SECRET` (HS256, ≥32 bytes); the secret is wrapped with zeroize and wiped from memory on Drop (CWE-316 protection). Weak secrets (empty or <32 bytes) **refuse to start in production** (the security rule in `config_validator.rs` returns `Err` and blocks only when `is_production`); non-production environments get a warning without blocking; but an empty secret is always rejected by `build_garrison_config` (`garrison_config.rs` `EmptySecret`).
- RBAC: preset `crawlrs:read/write/admin` permissions and `admin/user/read_only` roles, mapped to `ApiKeyScope` via `auth_bridge::map_perms_to_scope`, with multi-tenant isolation (`tenant-isolation`).
- Brute-force protection (`firewall-bruteforce`, CWE-307): 5 failures / 60-second window / 300-second lockout; IP context is injected by `garrison_ip_context_middleware`, and 401/429 are triggered directly by garrison (429 follows RFC 6585).
- Authentication events are persisted to the crawlrs `audit_logs` table + garrison's own schema (`audit-log` + `audit-inklog`).
- Authentication permission/role queries go through a three-layer cache (L1 oxcache 30s + L2 DAO 300s + L3 callback), avoiding N+1 queries on the authentication hot path.

### SSRF Protection (dual validation)

Every scrape request validates the target URL at two independent points: the pre-enqueue validation in the handler (`src/presentation/helpers/`) and a second validation before engine routing executes. Coverage includes redirect following (`ssrf_redirect_test`) and IP/protocol rules (`ssrf_mod_test`); protected endpoint signatures inject the geo-restriction repository via the `extract_handler` generic (the `teams` feature).

**kill-switch (dangerous)**: once set, `CRAWLRS_DISABLE_SSRF_PROTECTION` bypasses SSRF validation (activation point: `ssrf_protection_disabled` at `src/infrastructure/services/config_service.rs:222`; decision logic `src/infrastructure/security/ssrf/mod.rs:345`). This switch is **for dev/test debugging only**; at production startup `src/bootstrap/config/config_validator.rs` emits a prominent warn (non-blocking, **once at startup only**, no recurring warning at runtime). Production deployments must remove this environment variable. For the variable list and comments see [.env.example](../../.env.example).

### Third-Party Data Egress: SearXNG Public Instances

When `search.searxng.enabled = true` (**off by default**, opt-in):

- **Plaintext query egress**: user search terms are sent in plaintext to third-party public SearXNG instances (`GET {instance}/search?q=…&format=json`). These instances are outside this deployment's control, and their logging/retention policies are not governed by this repo. Default-off is the mitigation; privacy-sensitive deployments should keep it off.
- **Outbound target sources**: instances come from the searx.space dataset or `seed_urls`; admission enforces HTTPS and passes `is_internal_url` static SSRF validation (rejecting loopback/intranet/link-local addresses); outbound requests reuse the global `reqwest::Client` (with per-hop SSRF redirect validation).
- **Active probing surface**: when enabled, two periodic workers are started — collection (default 6h) and liveness probing (default 5min, at most `candidate_batch` candidates per round) — making real outbound requests; traffic pacing uses conservative defaults, but high-frequency deployments may still get rate-limited by instances (429 → failure count + cooldown).
- **Governance views**: `GET /v1/searxng/instances` (the registry) and `GET /v1/searxng/pool` (hot pool + circuit-breaker mirror state) sit in the auth-protected group; when disabled both uniformly return 503.

### Request Integrity and Secret Safety

- Webhook deliveries are signed per the [Standard Webhooks](https://github.com/standard-webhooks/standard-webhooks) spec; signature verification uses `subtle` constant-time comparison against timing side channels.
- HMAC / SHA-2 are used for internal signing chains; `bcrypt` for password hashing; `zeroize` for sensitive memory cleanup.
- The API Key plaintext is returned once at issuance only; admin auto-bootstrapping is disabled in production. Two bootstrap entry points: `CRAWLRS_BOOTSTRAP_ADMIN` (when true/1, the API process creates a random admin key itself, `src/main.rs`) and `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` (specifies the key id, `src/bootstrap/services.rs`) — both are recommended for dev/test only. The inconsistent naming of the two entry points (no `__` prefix vs the confers prefix) is a known issue; do not confuse them.

### HTTP-Layer Hardening

- `security_headers_middleware` uniformly injects security response headers (pinned by dedicated unit tests).
- The `[trusted_proxies]` configuration (a CIDR list) restricts trusted proxies, preventing client IP spoofing from bypassing rate limiting and the firewall.
  XFF parsing takes the **rightmost non-trusted hop**, and edge proxies must **overwrite** that header
  (nginx: `proxy_set_header X-Forwarded-For $remote_addr;`) — do not use
  `$proxy_add_x_forwarded_for` (it preserves client-supplied forged left-side values). Authentication failure
  counting (garrison) and IP rate limiting share this parsing convention.
- `[cors]` supports an `allowed_origins` allowlist; production should configure concrete origins rather than `*`.
- Outbound proxies are configured uniformly via `[proxy]`; rate limiting is provided by limiteron with distributed rate limiting, quotas (`quota-control`), and circuit breaking (`circuit-breaker`).

### Security Posture Self-Check

Run `crawlrs doctor --security` before startup to verify the compiled feature matrix, the SSRF kill switch and runtime hardening configuration. Each line carries a three-state prefix (`✓` ok, `!` warning, `✗` fail); any `✗` yields exit code 1. A typical full-feature output:

```
✓ auth_garrison: Garrison authentication compiled in
✓ webhook: Webhook delivery compiled in
✓ rate_limit_limiteron: limiteron rate limiting compiled in
✓ llm_boundary: LLM extraction boundary compiled in
✓ ssrf_dual_validation: SSRF dual validation active (handler + engine routing)
✓ rate_limit_config: rate limiting on (default 60 rpm)
✓ proxy_egress: 1 proxy URL(s) configured
✓ llm_sanitization: LLM injection sanitization on ([llm.sanitization])
```

Anti-bot escalation hops emit structured `antibot_escalation` audit events; replay a task's escalation chain with `crawlrs route-replay <task-id>`.

---

## ⛓️ Supply Chain and Gates

| Gate | Environment | Content |
|------|----------|------|
| `cargo deny check` | CI (cargo-deny-action) + release job | advisories (yanked denied), license allowlist (MIT/Apache-2.0/BSD/ISC/MPL-2.0 etc.), duplicate-version warnings, wildcard version denial, crates.io-only sources |
| RUSTSEC precondition check | CI feature-matrix (auth-on combinations) | grep asserts that `src/` contains no `RS256/RS384/RS512/use rsa::` code paths — the automated precondition guarding `deny.toml`'s ignore of RUSTSEC-2023-0071 (RSA Marvin Attack; crawlrs uses only HS256 and never touches the rsa crate); if the precondition breaks, the gate fails |
| CodeQL | CI (`codeql.yml`) | GitHub code-scanning static analysis |
| clippy | CI + e2e Stage 2 | `--features standard/full -- -D warnings` (the E2E suite additionally covers the agent-lib surface with `--all-targets`) |
| Private-key scan | `scripts/pre-commit-check.sh` | pre-commit detection of private-key and AWS key patterns |
| rustls advisory | recorded in `deny.toml` | RUSTSEC-2026-0285 (rustls TLS 1.3 handshake, CVSS 5.3) was closed out by the dependency tree locking the fixed version 0.23.45 — no ignore needed |

Known and accepted risks are commented line by line in `deny.toml` (risk rating + removal condition); for example, the RUSTSEC-2023-0071 ignore depends on the "no RSA code paths" precondition, which CI verifies automatically.

---

## 🏭 Production Hardening Checklist

- [ ] Set a strong random `CRAWLRS__AUTH__JWT_SECRET` (HS256, ≥32 bytes; weak secrets refuse to start in production, warn-only elsewhere)
- [ ] Issue API Keys via `POST /v1/admin/api-keys`, storing plaintext keys in a secrets manager immediately (returned once only)
- [ ] Disable both admin auto-bootstrap entry points in production (`CRAWLRS_BOOTSTRAP_ADMIN` and `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`, dev/test only)
- [ ] Configure an appropriate database connection pool (`[database] max_connections`) and least-privilege accounts
- [ ] Configure CORS with concrete origins (no `*` wildcard)
- [ ] Set rate limits (`default_rpm` / `burst_size`) and team concurrency (`concurrency.default_team_limit`) according to capacity
- [ ] Configure `[trusted_proxies]` (CIDR) to prevent IP spoofing from bypassing the firewall/rate limiting
- [ ] Enable Prometheus metrics export and wire up alerting (the metrics endpoint performs real exports) (docker/prometheus/ provides sample configurations). **Note**: `/metrics` now returns real Prometheus text metrics (in-process recorder; public, unauthenticated, un-rate-limited, with no tenant/URL/credential labels; a network-layer allowlist is advised in production); the `docker/prometheus/` samples are for reference only — see `PERFORMANCE.md` for the real metric list
- [ ] Enable structured log aggregation (inklog) and include audit logs (`/v1/audit/logs`) in patrols
- [ ] Deploy behind TLS termination (reverse proxy/gateway); health checks go through `/health`
- [ ] Review the Webhook `secret` configuration and verify receiver-side signature checks
- [ ] Set up database backup and disaster recovery

---

## 🧪 Security Test Coverage

| Scenario | Location |
|------|------|
| Authentication end-to-end (issuance/verification/RBAC/lockout) | `tests/integration/auth_garrison_test.rs` (ignored, real PostgreSQL) |
| Brute-force 429 semantics | same file + inline tests in `src/presentation/middleware/rate_limit_middleware.rs` |
| SSRF pre-check/redirect validation | `tests/unit/presentation/helpers/ssrf_mod_test.rs`, `ssrf_redirect_test.rs` |
| Security response headers | `tests/unit/presentation/middleware/security_headers_middleware_test.rs` |
| Token forgery/bypass | `tests/unit/presentation` (the middleware domain) |
| RSA precondition guard | CI feature-matrix job (grep assertion on auth-on combinations) |

For the full scenario matrix see [🧪 Test Scenario Matrix](TEST_SCENARIOS.md).
