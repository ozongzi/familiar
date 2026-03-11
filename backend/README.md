# familiar

A personal AI agent that runs on your Linux server. Chat from any browser — on your phone or desktop.

Built on [`ds-api`](../ds-api) — Rust functions become AI tools with zero boilerplate.

---

## What it does

- Full-featured web UI (React + Vite) with real-time streaming
- Tool calls rendered live as they happen — arguments stream in token by token
- File editing tools with inline diff view
- Script execution with syntax-highlighted preview
- Per-conversation persistent history with semantic + full-text search
- Generation survives browser refresh — reconnect and replay seamlessly
- Interrupt or abort mid-generation from the UI

---

## Architecture

```
Browser (any device)
  │
  │  HTTPS / WSS
  ▼
nginx  ──────────────────────────────────────────┐
  │  /api/*  /ws/*  → :3000                      │
  │  /       → /srv/familiar/client/dist (static) │
  └─────────────────────────────────────────────┘
                    │
                    ▼
           familiar (axum, :3000)
                    │
         ┌──────────┴──────────┐
         │                     │
   REST /api/*           WebSocket /ws/:id
   (auth, files,         (streaming generation,
    history)              tool events, interrupt/abort)
                               │
                        DeepseekAgent (ds-api)
                               │
                    ┌──────────┼──────────┐
                 FileTool  ScriptTool  CommandTool
                 HistoryTool  PresentFileTool
```

Each conversation runs a background generation task on the server. Clients subscribe via WebSocket and receive a replay of buffered events on reconnect — the agent keeps running even if you close the tab.

---

## Tools

### `execute`
Run a shell command via `sh -c`.
- `command` — the command to run
- `cwd` *(optional)* — working directory

### `run_py`
Run a Python script with `uv run`. Supports [PEP 723 inline metadata](https://peps.python.org/pep-0723/) for declaring dependencies:
```python
# /// script
# requires-python = ">=3.11"
# dependencies = ["requests", "rich>=13"]
# ///
```

### `run_ts`
Run a TypeScript script with Bun. Import any npm package directly — Bun installs it automatically:
```ts
import { format } from "date-fns";
```

### `write`
Overwrite a file with new content. Parent directories are created automatically.
- `path`, `content`

### `str_replace`
Replace a unique text fragment in a file. Returns the surrounding context lines on success so the model can verify the change without an extra `get` call.
- `path`, `old_str`, `new_str`
- `old_str` must match exactly once — ambiguous matches are rejected with a helpful error.

### `get`
Read file content. Line numbers are **1-based**.
- `path`
- `from` *(optional)* — start line (default: 1)
- `to` *(optional)* — end line (default: last line)

### `patch`
Replace a line range with new content (1-based, both ends inclusive).
- `path`, `from`, `to`, `new_content`
- Use `str_replace` for most edits — `patch` requires exact line numbers.

### `get_file_info`
Returns file size and total line count. Call this before `get` on large files.

### `list_dir`
List a directory. Returns structured entries (dirs first, then files with sizes).

### `read_binary`
Read a binary file in xxd-style hex+ASCII format.
- `path`, `begin`, `end` (byte offsets)

### `present_file`
Expose a file to the user as a downloadable card in the UI. Supports inline preview for text files.

### `search_history_fts`
Full-text search over conversation history (PostgreSQL FTS).
- `query`, `limit` *(optional, default 10)*

### `search_history_semantic`
Semantic search over conversation history using vector embeddings.
- `query`, `limit` *(optional, default 5)*

---

## Prerequisites

- Rust toolchain (`cargo`) with `x86_64-unknown-linux-musl` target for cross-compilation
- Node.js / Bun for building the frontend
- A Linux server with:
  - PostgreSQL (with `pgvector` extension)
  - nginx
  - A TLS certificate (Certbot recommended)
- A DeepSeek API key from [platform.deepseek.com](https://platform.deepseek.com)
- An embedding API endpoint (for semantic history search)

---

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | ✅ | — | PostgreSQL connection string |
| `DEEPSEEK_API_KEY` | ✅ | — | DeepSeek API key |
| `JWT_SECRET` | ✅ | — | Secret for signing auth tokens |
| `EMBED_URL` | ✅ | — | Embedding API base URL |
| `EMBED_API_KEY` | ✅ | — | Embedding API key |
| `PORT` | — | `3000` | Port the server listens on |
| `SYSTEM_PROMPT` | — | — | System prompt prepended to every conversation |
| `RUST_LOG` | — | `info` | Log level |

---

## Build & deploy

A `Makefile` at the workspace root handles everything:

```bash
# Full build (frontend + cross-compiled backend) and deploy to server
make deploy

# Build backend only (cross-compiled musl binary for Linux)
make build

# Build frontend only
make build-client

# Local dev
make dev-server   # backend on :3000
make dev-client   # Vite dev server with proxy
```

`make deploy` does:
1. Builds the frontend (`bun run build`)
2. Cross-compiles the backend (`x86_64-unknown-linux-musl`)
3. `scp`s the binary to `/usr/local/bin/familiar`
4. `rsync`s `dist/` to `/srv/familiar/client/dist/`
5. Restarts the `familiar` systemd service

### First-time server setup

**1. PostgreSQL**

```bash
sudo -u postgres psql -c "CREATE USER familiar WITH PASSWORD 'yourpassword';"
sudo -u postgres psql -c "CREATE DATABASE familiar OWNER familiar;"
sudo -u postgres psql -d familiar -c "CREATE EXTENSION vector;"
```

**2. Configuration file**

```bash
sudo mkdir -p /etc/familiar
sudo nano /etc/familiar/config.toml   # fill in the variables above
sudo chmod 600 /etc/familiar/config.toml
```

**3. Systemd unit**

```ini
[Unit]
Description=Familiar — personal AI agent
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
WorkingDirectory=/srv/familiar
Environment=RUST_LOG=info
ExecStart=/usr/local/bin/familiar
# Notes: configuration is read from /etc/familiar/config.toml by the service
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable familiar
sudo systemctl start familiar
```

**4. nginx**

```nginx
server {
    server_name familiar.yourdomain.com;

    location / {
        root /srv/familiar/client/dist;
        try_files $uri $uri/ /index.html;
    }

    location /api/ {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_read_timeout 300s;
    }

    location /ws/ {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 3600s;
    }

    listen 443 ssl;
    ssl_certificate     /etc/letsencrypt/live/familiar.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/familiar.yourdomain.com/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;
}

server {
    if ($host = familiar.yourdomain.com) {
        return 301 https://$host$request_uri;
    }
    listen 80;
    server_name familiar.yourdomain.com;
    return 404;
}
```

Get a TLS certificate:

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d familiar.yourdomain.com
```

---

## Logs

```bash
journalctl -u familiar -f
```
