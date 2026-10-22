# Multi-stage Dockerfile for B.O.S.S. (BEERCADE Operations & Supervisory System)
# Supports multi-architecture builds (linux/amd64, linux/arm64)

# ------------------------------------------------------------------------------
# Stage 1: Build SvelteKit Kiosk Frontend
# ------------------------------------------------------------------------------
FROM --platform=$BUILDPLATFORM node:20-alpine AS frontend-builder
WORKDIR /app/kiosk_ui

COPY kiosk_ui/package*.json ./
RUN npm ci || npm install

COPY kiosk_ui/ ./
RUN npm run build

# ------------------------------------------------------------------------------
# Stage 2: Build Rust Backend Daemon
# ------------------------------------------------------------------------------
FROM rust:latest AS backend-builder
WORKDIR /app/boss

# Install build dependencies (pkg-config, libssl-dev for TLS)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Compile release binary
RUN cargo build --release

# ------------------------------------------------------------------------------
# Stage 3: Final Minimal Production Runtime
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create persistent storage directories
RUN mkdir -p /data/beercade/sync /usr/share/boss/kiosk_ui

# Copy binaries and UI assets from builders
COPY --from=backend-builder /app/boss/target/release/boss-server /usr/local/bin/boss-server
COPY --from=frontend-builder /app/kiosk_ui/build /usr/share/boss/kiosk_ui

# Environment configuration
ENV BOSS_HOST=0.0.0.0
ENV BOSS_PORT=8989
ENV BOSS_NETPLAY_UDP_PORT=9750
ENV BOSS_STATIC_DIR=/usr/share/boss/kiosk_ui
ENV BOSS_SYNC_DIR=/data/beercade/sync
ENV RUST_LOG=info

# Expose ports
# 8989/tcp: HTTP/WebSockets/SSE & Kiosk UI
# 9750/udp: Netplay Discovery Relay & Telemetry Arbiter
EXPOSE 8989/tcp
EXPOSE 9750/udp

VOLUME ["/data/beercade/sync"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8989/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/boss-server"]
