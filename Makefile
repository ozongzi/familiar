HOST = root@familiar.fhmmt.games
BIN  = backend/target/x86_64-unknown-linux-musl/release/familiar
REMOTE_SRC = /root

.PHONY: build build-client dev deploy clean

# ── Rust backend ─────────────────────────────────
build:
	cd backend && CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
		cargo build --release -p familiar --target x86_64-unknown-linux-musl

# ── Frontend ──────────────────────────────────────────────────────────────────
build-client:
	cd frontend && bun install --frozen-lockfile && bun run build

# Start frontend dev server (proxies /api and /ws to localhost:3000)
dev-client:
	cd frontend && bun run dev

# Start backend in dev mode (reads .env automatically via dotenvy)
dev-server:
	cargo run -p familiar

# ── Full build (backend + frontend) ───────────────────────────────────────────
all: build-client build

# ── Deploy (local cross-compile → scp binary + client, then restart) ─────────
# scp/rsync first, restart last — never stop before copying so the running
# process is never killed mid-tool-call by its own deploy.
deploy: all
	scp $(BIN) $(HOST):/usr/local/bin/familiar.new
	ssh $(HOST) "mv /usr/local/bin/familiar.new /usr/local/bin/familiar"
	ssh $(HOST) "mkdir -p /srv/familiar/frontend/dist"
	rsync -av --delete frontend/dist/ $(HOST):/srv/familiar/frontend/dist
	scp backend/config.prod.toml $(HOST):/srv/familiar/config.toml
	ssh $(HOST) "systemctl restart familiar"
	@echo "✓ deployed"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
