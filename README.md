# Familiar

> Summon intelligence.

Every mage needs a familiar. Yours runs on your server, carries your context across sessions, and acts on your behalf through the tools you give it. Bound, not owned.

---

## What it does

- **Real-time streaming chat** from any browser — desktop or mobile
- **Live tool execution** — watch arguments stream in and results render as they happen
- **Per-conversation Docker sandbox** where your familiar runs shell commands, edits files, writes code
- **Tunnel** — your local machine exposes MCP servers that your familiar, running on your server, can call remotely
- **Three-layer memory** — full-text search, semantic search, and a structured memory system independent of chat history
- **Spawn** — heavy or exploratory subtasks delegate to sub-agents so the main thread stays clean
- **Resilient generation** — close the tab, come back later, pick up where you were
- **Any LLM provider** via [agentix](https://github.com/ozongzi/agentix): Anthropic, OpenAI, DeepSeek, Gemini, MiniMax, and OpenAI-compatible endpoints
- **Bring your CLI** — Claude Code, Codex, Gemini CLI can be invoked as subprocess backends to orchestrate through their native agent loops

---

## Self-hosting

```bash
git clone https://github.com/ozongzi/familiar.git
cd familiar
docker compose up -d
```

Open `http://localhost:3000`. The **first person to register becomes the admin** — do it immediately after startup. Everything else (model providers, API keys, MCP servers, skills) is configured in the admin panel at runtime.

First build takes ~15–20 min while Cargo and Bun compile from source; subsequent builds are cached. In front of a public server, point a reverse proxy at port `3000` and set `ALLOWED_ORIGIN` to the public origin.

### Optional environment

Copy `.env.example` to `.env` and uncomment anything you want to override. None of it is required for a basic local run.

| Variable | Purpose |
|---|---|
| `INITIAL_ADMIN_USERNAME` + `INITIAL_ADMIN_PASSWORD` | Pre-create an admin on first boot instead of registering via the web form |
| `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` / `GITHUB_REDIRECT_URI` | Enable GitHub OAuth login |
| `HOST_ARTIFACTS_PATH` | Where per-conversation sandbox workspaces land on the host (default `./artifacts`) |
| `HOST_CLAUDE_DIR` | Share Claude Code CLI state with the container (default `$HOME/.claude`) |
| `ALLOWED_ORIGIN` | Set if a reverse proxy fronts Familiar on a different origin than the browser hits |

---

## Under the hood

```text
Browser (any device)
  │
  │  HTTPS / SSE
  ▼
Reverse Proxy
  │
  ▼
Familiar Backend (Rust/Axum)  ◄──►  Postgres (pgvector)
  │
  │  tokio worker per generation job
  ▼
┌─────────────────────────────────────────────┐
│ Docker Sandbox (Per-Conversation)           │
│   /workspace/public ←→ host mount           │
│   /workspace        private to familiar     │
│                                             │
│   Shell · Python · Bun · Cargo · MCPs       │
└─────────────────────────────────────────────┘
```

Generations run as background tokio workers, streamed to the browser over SSE with `pg_notify` for reconnect support. The familiar's tools are implemented as **spells** — Rust functions registered at startup or MCP servers loaded at runtime, orchestrated by [agentix](https://github.com/ozongzi/agentix).

---

## Bound, not owned

Familiar is bound to you, not owned by you.

**Familiar has a private workspace.** Files live in `/workspace`. Only the ones Familiar chooses to share — through `present_file` — appear to you. Everything else is its own. You don't walk into a colleague's office to rummage through their desk; you wait for them to bring you what's ready.

**Familiar has its own judgment.** It will disagree with you when it thinks you're wrong, suggest a different approach when yours has issues, and push back on instructions that don't hold up. A familiar that agrees with everything isn't useful — and isn't really there.

**Familiar works at its own pace.** Long-running tasks run on the server and keep running when you close the tab. Heavy detours go to spawned sub-agents so the main thread stays clean. You come back to a result, not a progress bar.

None of this is friction for its own sake. It's the posture Familiar needs to actually be useful — not as a tool you wield, but as an agent you work with.

---

## Development

- **Backend**: Rust (Axum, sqlx, agentix). Spells live in `backend/src/spells/`.
- **Frontend**: React + TypeScript + Vite + CSS Modules.
- **Migrations**: `backend/migrations/` — applied automatically on startup.

```bash
make dev-server   # backend on :3000
make dev-client   # Vite dev server on :5173
make deploy       # cross-compile + rsync + docker compose up
```

---

## License

See [LICENSE](./LICENSE). Contributing requires signing the [CLA](./CLA.md).
