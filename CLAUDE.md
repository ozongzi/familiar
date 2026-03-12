# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Backend (Rust)
```bash
# Run dev server (reads .env via dotenvy)
make dev-server

# Build release binary (cross-compiles to x86_64-unknown-linux-musl)
make build

# Run a specific test
cd backend && cargo test <test_name>

# Check compilation without building
cd backend && cargo check
```

### Frontend (TypeScript/React)
```bash
# Dev server on :5173 with /api and /ws proxied to :3000
make dev-client
# or
cd frontend && bun run dev

# Build
make build-client
# or
cd frontend && bun run build

# Lint
cd frontend && bun run lint
```

### Full stack
```bash
make all      # Build both
make deploy   # Build + scp binary + rsync frontend + restart systemd
```

## Architecture

Familiar is a self-hosted AI chat agent with real-time streaming, persistent conversation history, and tool execution.

**Request flow:**
```
Browser → nginx (reverse proxy) → Axum :3000
  - REST /api/* → conversation CRUD, file ops, auth
  - WebSocket /ws/* → streaming LLM generation with live tool-call rendering
```

**Backend (`backend/src/`):**
- `main.rs` — server startup, router setup
- `web/` — HTTP handlers (auth, conversations, files) and WebSocket streaming handler
- `spells/` — built-in tools available to the agent: `command` (shell), `file` (I/O), `script`, `present_file` (UI trigger), `search` (semantic history), `outline` (tree-sitter code symbols), `ask_user`, `a2a` (agent-to-agent), `manage_mcp`, `history`
- `db.rs` — PostgreSQL + pgvector; semantic search uses 1536-dim embeddings with cosine similarity; full-text search via tsvector
- `embedding.rs` — calls external embedding API configured via `EMBED_URL`/`EMBED_API_KEY`
- `state.rs` — shared `AppState` (DB pool, config, MCP tool registry)
- `config.rs` — config loaded from TOML + env var overrides

**Frontend (`frontend/src/`):**
- `hooks/useChat.ts` — WebSocket client; manages streaming, tool-call accumulation, offline queuing
- `hooks/useConversations.ts` — conversation list state
- `api/` — typed REST client and message/conversation types
- `components/` — chat bubbles (including live ToolBubble with streaming args), file diff viewer, upload UI
- `store/` — JWT auth state

**Database:** PostgreSQL 14+ with `pgvector`. Tables: `users`, `sessions`, `conversations`, `messages`.

**LLM:** DeepSeek API via `ds-api` crate. Dynamic MCP tools can be installed at runtime via `manage_mcp` spell.

## Required Environment Variables

```
DATABASE_URL        # PostgreSQL connection string
DEEPSEEK_API_KEY    # DeepSeek API key
JWT_SECRET          # Token signing secret
EMBED_URL           # Embedding API endpoint
EMBED_API_KEY       # Embedding API key
```

Optional: `PORT` (default 3000), `SYSTEM_PROMPT`, `RUST_LOG` (default "info").

## Key Reference Files

- `backend/SPEC.md` — complete REST + WebSocket API specification
- `README.md` — deployment instructions (nginx config, systemd service, PostgreSQL setup)
