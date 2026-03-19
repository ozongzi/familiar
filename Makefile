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

# ── Docker Sandbox ────────────────────────────────────────────────────────────
build-sandbox:
	@LOCAL_HASH=$$(git -C ../autocheck-mcp rev-parse HEAD 2>/dev/null || find ../autocheck-mcp/src -type f | sort | xargs sha256sum | sha256sum | cut -d' ' -f1); \
	REMOTE_HASH=$$(ssh $(HOST) "cat /root/autocheck-mcp/.deployed-hash 2>/dev/null"); \
	if [ "$$LOCAL_HASH" = "$$REMOTE_HASH" ]; then \
		echo "✨ sandbox image up-to-date, skipping build"; \
	else \
		echo "⌛ Building autocheck-mcp for linux/amd64 locally..."; \
		docker buildx build --platform linux/amd64 -t autocheck-mcp:latest --load ../autocheck-mcp; \
		echo "⌛ Pushing to remote..."; \
		docker save autocheck-mcp:latest | gzip | ssh $(HOST) "gunzip | docker load"; \
		ssh $(HOST) "echo $$LOCAL_HASH > /root/autocheck-mcp/.deployed-hash"; \
		echo "✓ sandbox image built and pushed"; \
	fi

# ── Deploy (local cross-compile → scp binary + client, then restart) ─────────
# scp/rsync first, restart last — never stop before copying so the running
# process is never killed mid-tool-call by its own deploy.
deploy: all build-sandbox
	scp $(BIN) $(HOST):/usr/local/bin/familiar.new
	ssh $(HOST) "mv /usr/local/bin/familiar.new /usr/local/bin/familiar"
	ssh $(HOST) "mkdir -p /srv/familiar/frontend/dist"
	rsync -av --delete frontend/dist/ $(HOST):/srv/familiar/frontend/dist
	ssh $(HOST) "systemctl restart familiar"
	@echo "✓ deployed"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
