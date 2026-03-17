# Familiar

Familiar is a self-hosted personal AI agent designed for Linux servers. It provides a Claude-like experience with real-time streaming, live tool execution, and isolated sandboxes for every user.

Built on [ds-api](https://github.com/ozongzi/ds-api), Familiar turns Rust functions and MCP servers into powerful AI capabilities with minimal overhead.

---

## Key Features

- **Isolated Execution**: Every user gets a dedicated Docker sandbox. Tools like shell commands and file operations run in a restricted environment.
- **Claude-style Analysis**: Rich UI for file interactions. The agent can "present" files, allowing you to preview code, view data, and download results directly in the chat.
- **Live Tool Streaming**: Watch the agent "think" and act. Tool arguments stream in real-time, and execution results are rendered immediately.
- **Dynamic MCP Integration**: Install and manage Model Context Protocol (MCP) servers (stdio or http) on the fly to extend the agent's capabilities.
- **Resilient Generation**: Powered by Server-Sent Events (SSE). Generations continue on the server even if you close the browser; simply reconnect to replay the event stream.
- **Semantic Search**: Full-text and vector-based search across your entire conversation history.
- **Sub-Agent Support**: Complex tasks can be delegated to specialized sub-agents via the `spawn` tool.

---

## Architecture

```text
Browser (Desktop/Mobile)
  │
  │  HTTPS / SSE (Server-Sent Events)
  ▼
Reverse Proxy (e.g., Caddy)
  │
  ▼
Familiar Backend (Rust/Axum) <───> Postgres (pgvector)
  │
  │ (Dynamic Orchestration)
  ▼
┌─────────────────────────────────────────────────────────┐
│ Docker Sandbox (Per-User Container)                     │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ /workspace (Mounted Host Storage)                   │ │
│ │                                                     │ │
│ │ [User MCPs]         [Transient Files]               │ │
│ │ (Shell, Python)     (Data, Logs, Charts)            │ │
│ └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Multi-Tenancy & Isolation
- **System MCPs**: Defined in `config.toml`, these run on the host and are used for trusted system-level tasks.
- **User MCPs**: Installed dynamically or loaded from the database, these run inside the user's Docker sandbox via `docker exec`.
- **Path Mapping**: Host storage at `artifacts_path/{user_id}` is transparently mapped to `/workspace` inside the sandbox.

---

## Prerequisites

- **Linux Server** with Docker installed.
- **PostgreSQL** with the `pgvector` extension.
- **Rust Toolchain** (for building the backend).
- **Bun** (for building the frontend).
- **LLM API Key**: DeepSeek or any OpenAI-compatible provider.

---

## Getting Started

### 1. Build & Deploy
The project uses a `Makefile` to simplify cross-compilation and deployment:

```bash
# Cross-compile backend, build frontend, and deploy to your server
make deploy

# Or run locally for development
make dev-server   # Starts backend on :3000
make dev-client   # Starts Vite dev server on :5173
```

### 2. Environment Variables
Familiar reads configuration from a `config.toml` (specified by `FAMILIAR_CONFIG`). Key settings include:

| Variable | Description |
|---|---|
| `DATABASE_URL` | PostgreSQL connection string |
| `DEEPSEEK_API_KEY` | Your LLM provider API key |
| `artifacts_path` | Host directory for user sandboxes |
| `EMBED_URL` | Embedding API endpoint for semantic search |

### 3. Service Configuration
It is recommended to run Familiar as a `systemd` service and use a reverse proxy like **Caddy** to handle SSL/TLS and proxy the SSE/API traffic.

---

## Development

- **Backend**: Rust (Axum, sqlx, ds-api). Spells (tools) are located in `backend/src/spells/`.
- **Frontend**: React (TypeScript, Vite, CSS Modules).
- **Database Migrations**: Located in `backend/migrations/`.

---

## Logs
Monitor the agent's activity and sandbox execution:
```bash
journalctl -u familiar -f
```
