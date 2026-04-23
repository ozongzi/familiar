HOST       = root@yinli.tech
BIN        = backend/target/x86_64-unknown-linux-musl/release/familiar
REMOTE_DIR = /opt/familiar

.PHONY: build build-client dev deploy clean

# ── Rust backend ─────────────────────────────────
build:
	cd backend && CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc \
		cargo build --release -p familiar --target x86_64-unknown-linux-musl

# ── Tauri desktop client ──────────────────────────────────────────────────────
prepare-tauri:
	cd frontend/src-tauri && \
		PLAYWRIGHT_BROWSERS_PATH=./playwright-browsers \
		npx --yes playwright install chromium
	cd frontend/src-tauri && \
		npm install --prefix ./mcp-bundle @playwright/mcp

build-tauri: prepare-tauri
	cd frontend && bun tauri build

# ── Frontend (web) ────────────────────────────────────────────────────────────
build-client:
	cd frontend && bun install && bun run build

# Start frontend dev server (proxies /api and /ws to localhost:3000)
dev-client:
	cd frontend && bun run dev

# Start backend in dev mode (reads env from shell)
dev-server:
	AGENTIX_LOG_BODIES=1 cargo run -p familiar

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
		docker buildx build --platform linux/amd64 -f ../autocheck-mcp/Dockerfile -t autocheck-mcp:latest --load ..; \
		echo "⌛ Pushing to remote..."; \
		docker save autocheck-mcp:latest | gzip | ssh $(HOST) "gunzip | docker load"; \
		ssh $(HOST) "echo $$LOCAL_HASH > /root/autocheck-mcp/.deployed-hash"; \
		echo "✓ sandbox image built and pushed"; \
	fi

# ── Deploy: local build → rsync → docker compose up (no build on server) ──────
# 1. Cross-compile backend locally
# 2. Build frontend locally
# 3. Copy binary + dist into place so Dockerfile.deploy can COPY them
# 4. rsync everything to server
# 5. docker compose up --build (rebuilds only the slim runtime layer, fast)
deploy: build build-client
	cp $(BIN) backend/familiar
	rsync -av --delete frontend/dist/    $(HOST):$(REMOTE_DIR)/frontend/dist/
	rsync -av docker-compose.yml         $(HOST):$(REMOTE_DIR)/
	rsync -av backend/Dockerfile.deploy  $(HOST):$(REMOTE_DIR)/backend/Dockerfile
	rsync -av .dockerignore.deploy       $(HOST):$(REMOTE_DIR)/.dockerignore
	rsync -av backend/familiar           $(HOST):$(REMOTE_DIR)/backend/
	rsync -av backend/migrations/        $(HOST):$(REMOTE_DIR)/backend/migrations/
	ssh $(HOST) "mkdir -p $(REMOTE_DIR)/docker/sandbox $(REMOTE_DIR)/artifacts"
	rsync -av docker/sandbox/            $(HOST):$(REMOTE_DIR)/docker/sandbox/
	ssh $(HOST) "cd $(REMOTE_DIR) && docker compose up -d --build"
	rm -f backend/familiar
	@echo "✓ deployed"

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
