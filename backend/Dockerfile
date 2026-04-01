# ── Build ─────────────────────────────────────────────────────────────────────
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    musl-tools pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build

# Cache dependency compilation
COPY Cargo.toml Cargo.lock ./
COPY src/main.rs src/main.rs
RUN mkdir -p src && \
    cargo build --release --target x86_64-unknown-linux-musl 2>/dev/null || true

COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# ── Runtime ───────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/frontend/dist /app/artifacts

COPY --from=builder \
    /build/target/x86_64-unknown-linux-musl/release/familiar \
    /usr/local/bin/familiar

COPY migrations /migrations

EXPOSE 8080
CMD ["/usr/local/bin/familiar"]
