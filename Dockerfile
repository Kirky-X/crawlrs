
FROM rust:1.76-bullseye as builder

WORKDIR /usr/src/app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Build app
COPY . .
RUN cargo build --release

# Runtime image
FROM debian:bullseye-slim

WORKDIR /app

# Install dependencies for chromiumoxide (if using headless chrome)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    chromium \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/app/target/release/crawlrs /app/crawlrs
COPY config /app/config

# Create a non-root user
RUN useradd -m crawlrsuser
USER crawlrsuser

ENV RUST_LOG=info
ENV APP_ENVIRONMENT=production

EXPOSE 3000

CMD ["./crawlrs"]
