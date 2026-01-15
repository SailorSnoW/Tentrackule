# ============================================================================
# Stage 1: Build
# ============================================================================
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create dummy project for dependency caching
RUN cargo new --bin tentrackule
WORKDIR /app/tentrackule

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Build dependencies only (cached layer)
RUN cargo build --release && rm -rf src target/release/deps/tentrackule*

# Copy actual source code
COPY src ./src
COPY assets ./assets

# Build the actual binary
RUN cargo build --release --locked

# ============================================================================
# Stage 2: Runtime
# ============================================================================
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -s /bin/false appuser

# Copy binary from builder
COPY --from=builder /app/tentrackule/target/release/tentrackule /app/tentrackule

# Create directories and set permissions
RUN mkdir -p /app /data/.cache/images && chown -R appuser:appuser /app /data

# Switch to non-root user
USER appuser

WORKDIR /data

ENTRYPOINT ["/app/tentrackule"]
