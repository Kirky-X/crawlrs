# 🤝 Crawlrs Contributing Guide

Welcome to crawlrs development! This document covers environment setup, the development workflow, code standards, and the PR submission process. All conventions come from the repository's actual configuration (`Cargo.toml`, `.github/workflows/ci.yml`, `scripts/pre-commit-check.sh`, `AGENTS.md`).

## 📋 Table of Contents

- [👋 Welcome](#-welcome)
- [🧰 Environment Setup](#-environment-setup)
- [🔄 Development Workflow](#-development-workflow)
- [🧪 Testing and Verification](#-testing-and-verification)
- [📐 Code Standards](#-code-standards)
- [🔧 Submitting a PR](#-submitting-a-pr)
- [🐛 Reporting Issues](#-reporting-issues)

---

## 👋 Welcome

crawlrs is an enterprise-grade web scraping platform built with Rust. Whether it is fixing bugs, adding engines, improving docs, or enhancing tests — [Issues](https://github.com/Kirky-X/crawlrs/issues/new) and [PRs](https://github.com/Kirky-X/crawlrs/pulls) are welcome. Do not disclose security issues publicly; see [🔒 Security Docs · Vulnerability Reporting](SECURITY.md).

---

## 🧰 Environment Setup

### Prerequisites

| Dependency | Version | Purpose |
|------|------|------|
| Rust | 1.97.1+ (`Cargo.toml` `rust-version`; stable works) | toolchain |
| protoc | any recent version | sdforge build dependency (CI installs it via taiki-e/install-action) |
| PostgreSQL | 16+ | integration tests (started automatically by testcontainers, or an external instance) |
| Docker | 20+ | testcontainers / docker-compose test environment |
| Python 3 | 3.x (optional) | run the `tests/python/` API tests |

### Clone and Build

```bash
git clone https://github.com/Kirky-X/crawlrs.git
cd crawlrs

cargo build            # default features (platform + db-postgres)
cargo build --features full   # all engines and capabilities
```

> Note: the in-house foundation dependencies are all pulled from the registry (fluxweir 0.1, vecboost 0.3, nebulaid 0.3, alchemize 0.1 etc. are all on the registry); this repo has no path/git dependencies, so a clean checkout resolves and builds directly.

### Verify the Environment Is Ready

```bash
scripts/pre-commit-check.sh all   # fmt → clippy → check → build → private-key scan
```

---

## 🔄 Development Workflow

### Branches and Commits

```bash
# Sync main, then create a feature branch
git checkout -b feature/amazing-feature

# Commit messages follow Conventional Commits: type(scope): subject
# type ∈ feat / fix / docs / refactor / test / chore (consistent with existing git log history)
git commit -m 'feat(engines): add amazing feature'
```

### Implementation Requirements (Hard Rules)

- Placeholder implementations are forbidden: no `TODO` / `FIXME` / `unimplemented!()` / `todo!()`.
- Handle all error paths and boundary conditions; validate external input.
- Use the project's existing infrastructure (real PostgreSQL, oxcache, etc.); logging goes uniformly through the `log::` facade (`log::info!` etc., **not** tracing).
- Write tests for new features; add doc comments to public APIs.

### Spec-Driven Changes (Optional)

This project uses specmark to manage larger changes: `specmark/specs/<cap>/spec.md` holds capability specs, and `specmark/changes/<change>/` holds in-progress change proposals (propose → apply → converge → archive). Small changes do not need this process.

---

## 🧪 Testing and Verification

### Pre-Commit Verification Order (identical to CI)

```bash
# 1. Formatting and static checks
cargo fmt -p crawlrs -p crawlrs-examples -- --check
cargo clippy --features "standard" -- -D warnings
cargo clippy --features "full" -- -D warnings

# 2. Compilation (CI check-default / check-full jobs)
cargo check
cargo check --features "full"

# 3. Unit tests (need PostgreSQL 16, started automatically by testcontainers locally)
cargo test --features "standard" --lib
cargo test --features "full" --lib

# 4. Integration and all test targets
cargo test --features "full,test-mocks" --tests --no-fail-fast

# 5. Supply chain and coverage
cargo deny check
cargo llvm-cov --features "full" --fail-under-lines 90
```

One-click script: `scripts/pre-commit-check.sh all`; the full quality suite: `./tests/e2e/e2e-suite.sh` (feature matrix + static + tests + integration + benchmarks + report). For the complete list of scenarios and commands see [🧪 Test Scenario Matrix](TEST_SCENARIOS.md).

### CI Checks (PR Gate)

`ci.yml` contains **11 jobs** (`.github/workflows/ci.yml`):

| # | Job | Content |
|---|-----|------|
| 1 | `check-default` | `cargo check` (default features) |
| 2 | `check-full` | `cargo check --features "full"` |
| 3 | `feature-matrix` | 7 combinations + garrison gating assertions + RSA precondition check |
| 4 | `fmt` | `cargo fmt -p crawlrs -p crawlrs-examples -- --check` |
| 5 | `clippy` | `cargo clippy --features "standard" -- -D warnings` + `--features "full" -- -D warnings` |
| 6 | `test` | `cargo test` (Rust 1.97.1 / stable matrix) |
| 7 | `doc` | `cargo doc --features "full" --no-deps` (`RUSTDOCFLAGS=-D warnings`, zero warnings) |
| 8 | `deny` | `cargo deny check` |
| 9 | `integration-test` | `cargo test --features "full,test-mocks" --tests` (PostgreSQL 16 service container) |
| 10 | `acceptance-test` | BDD acceptance: `cargo test --test acceptance --features platform` (excluding `@external` tags) |
| 11 | `coverage` | `cargo llvm-cov`, line coverage ≥ 90% |

---

## 📐 Code Standards

- Formatting follows `cargo fmt`; clippy warnings are errors (`-D warnings`). **Two scopes**: CI's clippy job runs only `cargo clippy --features "standard" -- -D warnings` and `--features "full" -- -D warnings` (ci.yml:180,182, **without** `--all-targets`); `--all-targets -D warnings` appears only in E2E suite Stage 2 (`tests/e2e/e2e-suite.sh`). `[lints.rust.unexpected_cfgs]` is deny-level.
- Concurrency primitive conventions: short critical sections use `parking_lot` (not tokio sync primitives); metric counters use `DashMap`.
- Architecture conventions: follow the DDD four layers (presentation → application → domain → infrastructure); DI components are assembled by the capability Modules in `src/di/` (`SettingsModule`/`CacheModule`/`EngineModule` etc.); new engines implement the `ScraperEngine` trait and wire into `EngineRouter`.
- New features must be correctly feature-gated: provide Noop implementations when business capabilities are off, and cover the corresponding combinations in the `tests/e2e/feature-matrix.sh` matrix.
- Commit granularity: one logical change per commit; avoid mixing in unrelated formatting.

---

## 🔧 Submitting a PR

1. Fork the repo and create a feature branch (`feature/xxx` or `fix/xxx`).
2. Finish the implementation and tests; pass every command in the "pre-commit verification order" locally.
3. Push the branch and open a PR (base: `main` — the repo currently has only main as the long-lived branch), describing the motivation, impact surface, and testing.
4. Wait for CI to go green; if feature-matrix assertions fail, check whether the new feature is correctly gated.
5. Merge after maintainer review; keep the branch in sync with main before merging (CI uses a per-ref cancellation concurrency group).

---

## 🐛 Reporting Issues

- **Bugs**: [open an Issue](https://github.com/Kirky-X/crawlrs/issues/new) with the version (`/v1/version` or release number), feature combination, reproduction steps, and logs (sanitized).
- **Feature suggestions**: [an Issue](https://github.com/Kirky-X/crawlrs/issues/new) describing the scenario and expected behavior; propose large capabilities through specmark propose first.
- **Security vulnerabilities**: prefer GitHub [Security Advisories](https://github.com/Kirky-X/crawlrs/security/policy) private disclosure (fallback email [Kirky-X@outlook.com](mailto:Kirky-X@outlook.com), contact Kirky.X); do not disclose publicly — see [🔒 Security Docs](SECURITY.md) for the process.
