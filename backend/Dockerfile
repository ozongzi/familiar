# Pre-compiled binary is copied in via docker-compose build context.
# No Rust toolchain needed on the server.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/frontend/dist /app/artifacts

COPY familiar /usr/local/bin/familiar
COPY migrations /migrations

EXPOSE 8080
CMD ["/usr/local/bin/familiar"]
