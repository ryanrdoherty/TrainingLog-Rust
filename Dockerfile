# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.94-slim-bookworm AS builder

# System deps needed for sqlx, openssl, etc.
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependency compilation separately from source.
# Copy manifests first — this layer is only rebuilt when dependencies change.
COPY Cargo.toml Cargo.lock ./

# Create a dummy main so cargo can compile deps
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Now copy real source and build
COPY src ./src
COPY migrations ./migrations

# Touch main.rs so cargo knows it changed
RUN touch src/main.rs
RUN cargo build --release

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root
RUN useradd -ms /bin/bash appuser
USER appuser

WORKDIR /app

COPY --from=builder /app/target/release/sports-log ./sports-log
COPY --from=builder /app/migrations ./migrations

EXPOSE 3000

CMD ["./sports-log"]
