# Familiar

Familiar is a self-hosted personal AI agent designed for Linux servers. It provides a ChatGPT-like experience with real-time streaming, live tool execution, and isolated sandboxes for every user.

Built on [agentix](https://github.com/ozongzi/agentix), Familiar turns Rust functions and MCP servers into powerful AI capabilities with minimal overhead.

---

## Key Features

- **Isolated Execution**: Every user gets a dedicated Docker sandbox. Tools like shell commands and file operations run in a restricted environment.
- **Live Tool Streaming**: Watch the agent think and act. Tool arguments stream in real-time, and execution results are rendered immediately.
- **Dynamic MCP Integration**: Install and manage Model Context Protocol (MCP) servers (stdio or HTTP) on the fly to extend the agent's capabilities.
- **Resilient Generation**: Powered by Server-Sent Events (SSE) with a DB-backed job queue. Generations continue on the server even if you close the browser; reconnect to resume.
- **Semantic Memory**: Full-text and vector-based search across your entire conversation history, with automatic per-user memory extraction.
- **Sub-Agent Support**: Complex tasks can be delegated to specialized sub-agents via the `spawn` tool.
- **Multi-Provider**: Supports DeepSeek, OpenAI, Anthropic, Gemini, MiniMax, and any OpenAI-compatible API.
- **Performance Observability**: Built-in `⏱` tracing logs for every generation phase (pre-LLM setup, LLM stream connect, TTFT).

---

## Architecture

```text
Browser (Desktop/Mobile)
  │
  │  HTTPS / SSE (Server-Sent Events)
  ▼
Reverse Proxy (Caddy — automatic HTTPS)
  │
  ▼
Familiar Backend (Rust/Axum)  ◄──►  Postgres (pgvector)
  │
  │  tokio worker per generation job
  ▼
┌─────────────────────────────────────────────────────────┐
│ Docker Sandbox (Per-User Container)                     │
│  /workspace  ←→  artifacts_path/{user_id} (host mount) │
│                                                         │
│  Shell · Python · User MCPs · Transient Files           │
└─────────────────────────────────────────────────────────┘
```

### Generation Pipeline

```
HTTP POST /api/send
  └─► insert generation_job (DB)
  └─► spawn tokio worker
        ├─ load config + history + MCPs  (~50 ms)
        ├─ POST → LLM API  (TTFB varies by provider)
        ├─ stream tokens → pg_notify → SSE → browser
        └─ seal message + embed (async)
```

---

## Prerequisites

- **Linux server** with Docker installed
- **PostgreSQL** with the `pgvector` extension (or use the bundled `docker-compose.yml`)
- **Rust toolchain** + `x86_64-unknown-linux-musl` target (for cross-compilation)
- **Bun** (for building the frontend)
- **LLM API key** from any supported provider

---

## Getting Started

### 1. Build & Deploy

The project uses a `Makefile` for cross-compilation and deployment:

```bash
# Cross-compile backend + build frontend, rsync to server, docker compose up
make deploy

# Development
make dev-server   # backend on :3000
make dev-client   # Vite dev server on :5173
```

`make deploy` does **no compilation on the server** — the binary is cross-compiled locally and rsynced as a pre-built artifact.

### 2. Configuration

Familiar reads from environment variables / `config.toml` (path set via `FAMILIAR_CONFIG`):

| Key | Description |
|---|---|
| `DATABASE_URL` | PostgreSQL connection string |
| `artifacts_path` | Host directory for user sandbox workspaces |
| `EMBED_URL` | Embedding API endpoint (for semantic search) |

LLM provider and API keys are configured per-user or globally via the admin panel at runtime — no rebuild required.

### 3. Reverse Proxy

Use **Caddy** for automatic HTTPS and SSE proxying:

```
familiar.example.com {
    reverse_proxy localhost:8080
}
```

---

## Development

- **Backend**: Rust (Axum, sqlx, agentix). Spells (tools) live in `backend/src/spells/`.
- **Frontend**: React + TypeScript + Vite + CSS Modules.
- **Migrations**: `backend/migrations/` — applied automatically on startup.

---

## Logs

```bash
# On the server
docker logs familiar-familiar-1 -f 2>&1 | grep "⏱"
```

Sample output per generation:
```
⏱ load_from_db           ms=2
⏱ restore history        ms=4   messages=46
⏱ connect_mcps           ms=8   tools=0
⏱ total pre-LLM setup    ms=47
⏱ LLM stream connected   ms=320
⏱ TTFT (first token)     ms=1640
```
