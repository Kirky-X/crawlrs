# =============================================================================
# Crawlrs Unified Dockerfile
# =============================================================================
# This is the PRIMARY Dockerfile for building crawlrs from source.
#
# Related files:
#   - Dockerfile.runtime: Lightweight runtime from pre-built binary
#
# Usage:
#   Build:  docker build -t crawlrs:latest -f docker/Dockerfile .
#   Run:    docker run -p 8899:8899 crawlrs:latest
# =============================================================================

# ---------- Builder ----------
FROM rust:1.97-slim AS builder

# Install build dependencies (OpenSSL + pkg-config for TLS)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Configure Alibaba Cloud mirror for apt (speed up builds in China)
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list.d/debian*.sources 2>/dev/null || true

# Copy Cargo config for mirror source
COPY .cargo/config.toml .cargo/config.toml
COPY Cargo.toml Cargo.lock ./

# Cache dependencies: build deps first, then copy source
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN cargo build --features standard --release || true
RUN rm -rf src

# Copy real source and build the actual binary
COPY src/ src/
COPY examples/ examples/
COPY config/ config/

ARG BUILD_FEATURES=standard
RUN cargo build --features ${BUILD_FEATURES} --release --bin crawlrs

# ---------- Runtime ----------
FROM debian:trixie-slim AS runtime

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

# Copy i18n locale files
COPY --from=builder /build/locales/ /home/crawlrs/locales/

# Ensure config directory is accessible
RUN chown -R crawlrs:crawlrs /home/crawlrs

# Default environment variables
ENV RUST_LOG=info
ENV CRAWLRS__ENV_MODE=docker

USER crawlrs

# Expose API port
EXPOSE 8899

# Health check via the /health HTTP endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:8899/health || exit 1

# Run modes: "api" (default) or "worker"
ENTRYPOINT ["crawlrs"]
CMD ["api"]