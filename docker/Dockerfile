# Unified Dockerfile for crawlrs
# Supports all service types: api, engine, extraction, worker

# Stage 1: Builder
FROM rust:1.76-bullseye as builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Create a dummy project to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
COPY Cargo.toml Cargo.lock ./
COPY migration ./migration

# Build dependencies (this layer is cached if Cargo files don't change)
RUN cargo build --release --bin crawlrs
RUN rm -rf src

# Copy source code
COPY . .

# Build the application
# We touch the main file to ensure cargo rebuilds it
RUN touch src/main.rs && cargo build --release --bin crawlrs

# Stage 2: Runtime
FROM debian:bullseye-slim

WORKDIR /app

# Install runtime dependencies
# - ca-certificates & libssl-dev: for HTTPS
# - chromium: for the crawler engine (if running in local mode)
# - curl: for healthchecks
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    chromium \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /usr/src/app/target/release/crawlrs /app/crawlrs

# Copy config directory
COPY config /app/config

# Create a non-root user for security
RUN useradd -m crawlrsuser
USER crawlrsuser

# Default environment variables
ENV RUST_LOG=info
ENV APP_ENVIRONMENT=production

# Expose API port (used by api service)
EXPOSE 3000

# Entrypoint
# Usage: docker run <image> <subcommand>
# Example: docker run <image> api
ENTRYPOINT ["./crawlrs"]
