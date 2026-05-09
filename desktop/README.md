# Familiar Desktop (本地版)

Standalone Dioxus desktop client. No server, no account, no permissions.
Everything lives as files on your machine — conversations, memories, skills.

## Quick start

```bash
cd desktop
cargo run --release
```

First launch creates `~/.local/share/familiar/` (Linux), `~/Library/Application Support/dev.familiar.familiar/` (macOS), or `%APPDATA%\familiar\familiar\data\` (Windows). Override with `FAMILIAR_HOME=/some/path`.

Open **设置**, paste an Anthropic API key, save, then **+ 新对话**.

## Storage layout

```
$DATA_DIR/
├── config.toml             # API key, model, max_tokens, system prompt
├── conversations/
│   └── 01J...ULID.md       # one conversation per file
├── memories/               # (planned) long-term memory entries
├── skills/                 # (planned) reusable prompt fragments
└── workspaces/
    └── <conversation-id>/  # cwd for the bash / read_file / write_file tools
```

A conversation `.md` round-trips through git, GitHub, any markdown viewer. Tool calls are wrapped in HTML comments so they render invisibly:

```md
---
id: 01J...
title: 帮我看看 Cargo.toml
created_at: 2026-05-09T10:00:00Z
updated_at: 2026-05-09T10:00:00Z
model: claude-sonnet-4-6
---

# user
帮我看看 Cargo.toml

# assistant
我看一下。

<!-- tool_use id=toolu_01 name=read_file -->
{"path": "Cargo.toml"}
<!-- /tool_use -->

# user
<!-- tool_result id=toolu_01 -->
[package]
name = "..."
<!-- /tool_result -->

# assistant
看起来用了 …
```

## Built-in tools

| name | what |
|---|---|
| `bash` | run a shell command in the conversation workspace (120 s timeout, 64 KiB output cap) |
| `read_file` | UTF-8 file read; relative paths land in workspace, absolute paths are honoured |
| `write_file` | UTF-8 file write, parents created |
| `list_dir` | non-recursive directory listing |

There is **no sandbox**. The bash tool runs as your user on your machine. Treat the assistant accordingly.

## What's not in v0

- Long-term memory and skills (storage exists, no UI / tool wiring yet)
- MCP servers
- Multi-provider LLM (only Anthropic right now; agentix swap is a small refactor)
- Image / multimodal input
- Conversation rename
- Search across conversations (it's all `.md` — `rg` your data dir)

## Build deps (Linux)

```
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev libxdo-dev
```

macOS / Windows: just `cargo run`.
