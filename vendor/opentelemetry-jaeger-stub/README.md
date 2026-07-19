# opentelemetry-jaeger stub

This directory ships an empty stub crate that replaces `opentelemetry-jaeger`
0.22.0 on the local toolchain. It exists to break a transitive dependency
chain that pulls in a vulnerable version of `opentelemetry_sdk`.

## Why this stub exists

- **CVE-2026-48504** — `opentelemetry_sdk` 0.23.0 (reachable transitively via
  `opentelemetry-jaeger` 0.22.0) contains a denial-of-service vector in the
  span processor. The fix shipped in `opentelemetry_sdk` 0.24.0, but
  `opentelemetry-jaeger` 0.22 has not been re-published against the fixed
  SDK and is effectively abandoned.
- **Only consumer in this tree** — `limiteron` 0.2.6 declares
  `opentelemetry-jaeger` as a dependency but its `init_jaeger_tracer` helper
  is never invoked by `crawlrs`. The dependency is dead weight that drags in
  the vulnerable SDK. Patching `limiteron` is out of scope (third-party
  crate), so the stub neutralises the chain at the cargo resolution layer.
- **Created** — 2026-07-19 (see git history for the introducing commit).

## What the stub provides

- The crate name and version (`opentelemetry-jaeger` 0.22.0) match the
  upstream package so cargo's resolver prefers this local copy when
  `[patch.crates-io]` is configured in the workspace `Cargo.toml`.
- All feature flags declared by the upstream crate are mirrored as empty
  features (`rt-tokio`, `collector`, `reqwest`, …) so dependents that ask
  for a feature still resolve.
- No symbols are exported. Any caller that actually tried to use
  `opentelemetry_jaeger::` APIs would fail to link — by design, since the
  only consumer never calls them.

## Checking whether the upstream fix is available

Before removing this stub, verify that the vulnerable transitive chain is
gone. Run from the workspace root:

```sh
cargo tree -i opentelemetry_sdk | grep -E "0\.2[3-9]|0\.1[0-9]"
cargo tree -i opentelemetry-jaeger | grep -E "0\.2[2-9]|0\.3[0-9]"
```

The stub can be removed once **all** of the following are true:

1. `cargo tree -i opentelemetry_sdk` no longer reports any 0.23.x line, OR
   `opentelemetry_sdk` 0.23.x is no longer in `Cargo.lock`.
2. `limiteron` (or its successor) either drops `opentelemetry-jaeger` from
   its dependency list, or pins to a version of `opentelemetry-jaeger` that
   itself depends on `opentelemetry_sdk` >= 0.24.0.
3. A workspace `cargo audit` reports zero findings for CVE-2026-48504.

## Removal steps

1. Delete this directory: `rm -rf vendor/opentelemetry-jaeger-stub`.
2. Remove the `[patch.crates-io]` entry for `opentelemetry-jaeger` in the
   workspace `Cargo.toml`.
3. Run `cargo update -p opentelemetry-jaeger` and confirm cargo resolves to
   the upstream crate.
4. Re-run `cargo audit` and the `cargo tree -i` checks above.
5. Run the full test suite (`cargo test --no-default-features --lib --tests`)
   to confirm no test relied on the stub's empty-symbol behavior.

## TODO

- Track upstream `limiteron` releases that drop or upgrade
  `opentelemetry-jaeger`: see issue <!-- TODO: insert tracking issue URL -->.
- When `limiteron` ships a fixed version, bump the workspace
  `limiteron = "0.2"` requirement and re-evaluate removal.
