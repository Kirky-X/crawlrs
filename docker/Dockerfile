# Unified Dockerfile for crawlrs
# Supports all service types: api, engine, extraction, worker

# Stage 1: Builder
# 使用用户预拉取的 Rust 1.92.0-bullseye 镜像
FROM rust:1.92.0-bullseye as builder

WORKDIR /usr/src/app

# Install build dependencies with retry logic and Alibaba Cloud mirror
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list
RUN sed -i 's/security.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list.d/debian*.sources 2>/dev/null || true
RUN apt-get update -o Acquire::Retries=5 && apt-get install -y --no-install-recommends \
        cmake \
    && rm -rf /var/lib/apt/lists/*

# Create a dummy project to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Copy Cargo config for mirror source (MUST be before COPY Cargo.toml)
COPY .cargo/config.toml .cargo/config.toml

COPY Cargo.toml Cargo.lock ./
COPY migration ./migration

# Build dependencies (this layer is cached if Cargo files don't change)
RUN cargo build --release --bin crawlrs
RUN rm -rf src

# Copy source code
COPY src/ src/
COPY Cargo.toml Cargo.lock ./
COPY config/ config/
COPY migration/ migration/
COPY .cargo/ .cargo/

# Build the application
# We touch the main file to ensure cargo rebuilds it
RUN touch src/main.rs && cargo build --release --bin crawlrs

# Stage 2: Runtime
# 使用 rust 镜像（用户已预拉取）作为运行时基础
FROM rust:1.92.0-bullseye

WORKDIR /app

# 安装运行时依赖 (最小化安装,依赖chrome容器提供浏览器服务)
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list
RUN sed -i 's/security.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list
RUN sed -i 's/deb.debian.org/mirrors.aliyun.com/g' /etc/apt/sources.list.d/debian*.sources 2>/dev/null || true
RUN apt-get update -o Acquire::Retries=5 && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m crawlrsuser

# Copy binary from builder
COPY --from=builder /usr/src/app/target/release/crawlrs /app/crawlrs

# Copy config directory
COPY config /app/config

# Default environment variables
ENV RUST_LOG=info
ENV APP_ENVIRONMENT=production

# Expose API port (used by api service)
EXPOSE 3000

# Switch to non-root user for security
USER crawlrsuser

# Default command
CMD ["api"]
