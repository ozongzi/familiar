# syntax=docker/dockerfile:1.7

# Build context is the project root (see docker-compose.yml's `context: .`).
# This image builds the frontend with Bun, the backend with Cargo, and
# assembles a Debian-slim runtime that ships the binary, migrations, and
# built SPA. `git clone && docker compose up` should Just Work.

# ── Frontend ──────────────────────────────────────────────────────────────────
FROM oven/bun:1 AS fe-builder
WORKDIR /fe
COPY frontend/package.json frontend/bun.lock frontend/bunfig.toml ./
RUN bun install
COPY frontend ./
RUN bun run build

# ── Backend ───────────────────────────────────────────────────────────────────
FROM rust:1.85-slim-bookworm AS be-builder
WORKDIR /be
RUN apt-get update && apt-get install -y \
      pkg-config libssl-dev ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN cargo build --release

# ── Runtime ───────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
      ca-certificates curl gnupg \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg \
    && chmod a+r /etc/apt/keyrings/docker.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian bookworm stable" \
       > /etc/apt/sources.list.d/docker.list \
    && apt-get update && apt-get install -y docker-ce-cli \
    && curl -fsSL https://claude.ai/install.sh | bash \
    && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.local/bin:${PATH}"
RUN mkdir -p /app/frontend/dist /app/artifacts

COPY --from=be-builder /be/target/release/familiar /usr/local/bin/familiar
COPY --from=be-builder /be/migrations /migrations
COPY --from=fe-builder /fe/dist /app/frontend/dist

EXPOSE 3000
CMD ["/usr/local/bin/familiar"]
