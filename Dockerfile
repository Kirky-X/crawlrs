# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# =============================================================================
# Multi-stage build for crawlrs
# =============================================================================
# Stage 1: Builder — compiles the binary with parameterized feature flags
# Stage 2: Runtime — minimal image with only the binary and required libs

# ---------- Builder ----------
FROM rust:1.87-slim AS builder

# Install build dependencies (OpenSSL + pkg-config for TLS)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies: copy manifests first, build deps, then copy source
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN cargo build --features standard --release || true

# Copy real source and build the actual binary
COPY src/ src/
COPY config/ config/

# Feature set is parameterizable via BUILD_FEATURES build arg
# Default: standard preset (engine-playwright + metrics; HTTP 抓取栈与 7 组件均为非可选依赖)
# Note: `default = []` 已可用（核心依赖全部为非可选），但 standard 包含 metrics + playwright
ARG BUILD_FEATURES=standard
RUN cargo build --features ${BUILD_FEATURES} --release --bin crawlrs

# ---------- Runtime ----------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies (libssl + ca-certs for HTTPS, curl for healthcheck)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN useradd --create-home --uid 1000 crawlrs
WORKDIR /home/crawlrs

# Copy binary from builder
COPY --from=builder /build/target/release/crawlrs /usr/local/bin/crawlrs

# Copy default config (can be overridden via volume or env vars)
COPY --from=builder /build/config/ /home/crawlrs/config/

# Ensure config directory is writable by non-root user
RUN chown -R crawlrs:crawlrs /home/crawlrs

USER crawlrs

# Expose API port
EXPOSE 8899

# Health check via the /health HTTP endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:8899/health || exit 1

# Configuration via environment variables (CRAWLRS__ prefix, confers)
# See .env.example and config/default.toml for all options
# Run modes: "api" (default, starts HTTP server + workers) or "worker" (worker-only)
ENTRYPOINT ["crawlrs"]
CMD ["api"]
