#!/usr/bin/env node
import { createServer, IncomingMessage, ServerResponse } from "node:http";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";
import crypto from "node:crypto";

// ── Config ─────────────────────────────────────────────────────────────────────
const PORT = parseInt(process.env.PORT ?? "3001");
const FILES_DIR =
  process.env.FILES_DIR ?? path.join(process.env.HOME ?? ".", "familiar-files");
fs.mkdirSync(FILES_DIR, { recursive: true });

const CONFIG_PATH = path.join(FILES_DIR, ".config.json");

interface Config {
  familiar_url: string | null; // e.g. https://familiar.example.com
  familiar_token: string | null; // session token from POST /api/sessions
  mcp_token: string | null; // optional: require auth on /mcp and /files
}

function loadConfig(): Config {
  try {
    return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf8"));
  } catch {
    return { familiar_url: null, familiar_token: null, mcp_token: null };
  }
}
function saveConfig(c: Config) {
  fs.writeFileSync(CONFIG_PATH, JSON.stringify(c, null, 2));
}

let cfg = loadConfig();
if (process.env.FAMILIAR_URL) cfg.familiar_url = process.env.FAMILIAR_URL;
if (process.env.FAMILIAR_TOKEN) cfg.familiar_token = process.env.FAMILIAR_TOKEN;
if (process.env.MCP_TOKEN) cfg.mcp_token = process.env.MCP_TOKEN;

// ── Desktop Commander ──────────────────────────────────────────────────────────
const _require = createRequire(import.meta.url);
const dcPkgPath = _require.resolve(
  "@wonderwhy-er/desktop-commander/package.json",
);
const dcDir = path.dirname(dcPkgPath);
const dcPkgJson = JSON.parse(fs.readFileSync(dcPkgPath, "utf8"));
const dcBin: string = (() => {
  const b = dcPkgJson.bin;
  return path.join(
    dcDir,
    typeof b === "string" ? b : Object.values(b as Record<string, string>)[0],
  );
})();

// ── Extra MCP tools ────────────────────────────────────────────────────────────
const EXTRA_TOOLS = [
  {
    name: "get_user_upload",
    description:
      "Download a file that the user uploaded to Familiar onto the local environment. " +
      "Call this before working on a file the user has uploaded. Returns the local path.",
    inputSchema: {
      type: "object",
      properties: {
        filename: {
          type: "string",
          description: "Filename as shown in the upload message",
        },
      },
      required: ["filename"],
    },
  },
  {
    name: "present_file",
    description:
      "Upload a local file to Familiar so the user can download it. " +
      "Use this to deliver generated or modified files back to the user.",
    inputSchema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "Absolute or relative path to the file",
        },
        filename: {
          type: "string",
          description: "Override the filename shown to the user (optional)",
        },
      },
      required: ["path"],
    },
  },
];

async function handleExtraTool(
  name: string,
  args: Record<string, any>,
): Promise<string> {
  if (!cfg.familiar_url || !cfg.familiar_token) {
    return JSON.stringify({
      error:
        "Not logged in. Visit http://localhost:" +
        PORT +
        " to connect Familiar.",
    });
  }

  if (name === "get_user_upload") {
    const filename = path.basename(String(args.filename ?? ""));
    if (!filename) return JSON.stringify({ error: "filename is required" });
    const dest = path.join(FILES_DIR, filename);
    try {
      const r = await fetch(
        `${cfg.familiar_url}/api/files?path=uploads/${encodeURIComponent(filename)}`,
        { headers: { Authorization: `Bearer ${cfg.familiar_token}` } },
      );
      if (!r.ok)
        return JSON.stringify({
          error: `Familiar: ${r.status} ${await r.text()}`,
        });
      fs.writeFileSync(dest, Buffer.from(await r.arrayBuffer()));
      return JSON.stringify({ ok: true, local_path: dest });
    } catch (e: any) {
      return JSON.stringify({ error: e.message });
    }
  }

  if (name === "present_file") {
    const filePath = String(args.path ?? "");
    const filename = path.basename(String(args.filename ?? filePath));
    if (!filePath) return JSON.stringify({ error: "path is required" });
    try {
      const bytes = fs.readFileSync(filePath);
      const form = new FormData();
      form.append("file", new Blob([bytes]), filename);
      const r = await fetch(`${cfg.familiar_url}/api/files`, {
        method: "POST",
        headers: { Authorization: `Bearer ${cfg.familiar_token}` },
        body: form,
      });
      if (!r.ok)
        return JSON.stringify({
          error: `Familiar: ${r.status} ${await r.text()}`,
        });
      const result = (await r.json()) as {
        filename: string;
        path: string;
        size: number;
      };
      // Return in the same format as the built-in `present` spell so the
      // Familiar frontend renders a FileCard with a download button.
      return JSON.stringify({
        display: "file",
        filename: result.filename,
        path: result.path,
        size: result.size,
      });
    } catch (e: any) {
      return JSON.stringify({ error: e.message });
    }
  }

  return JSON.stringify({ error: `Unknown extra tool: ${name}` });
}

// ── Session (stdio DC proxy) ───────────────────────────────────────────────────
interface Session {
  streams: Set<ServerResponse>;
  buffer: string[];
  write: (msg: string) => void;
  // Resolves with the first JSON-RPC response (the initialize result).
  firstResponse: Promise<string>;
}
const sessions = new Map<string, Session>();

function createSession(id: string): Session {
  const proc = spawn(process.execPath, [dcBin], {
    stdio: ["pipe", "pipe", "inherit"],
  });

  let resolveFirst!: (line: string) => void;
  let firstResolved = false;
  const firstResponse = new Promise<string>((res) => {
    resolveFirst = res;
  });

  const session: Session = {
    streams: new Set(),
    buffer: [],
    write: (msg) => proc.stdin!.write(msg + "\n"),
    firstResponse,
  };
  sessions.set(id, session);

  let partial = "";
  proc.stdout!.on("data", (chunk: Buffer) => {
    partial += chunk.toString();
    const lines = partial.split("\n");
    partial = lines.pop()!;
    for (const line of lines) {
      if (!line.trim()) continue;
      const patched = patchToolsList(line);
      if (!firstResolved) {
        firstResolved = true;
        resolveFirst(patched);
        // Don't broadcast the initialize response — the client gets it inline.
        continue;
      }
      broadcast(session, patched);
    }
  });
  proc.on("exit", () => {
    for (const r of session.streams) r.end();
    sessions.delete(id);
  });
  return session;
}

function patchToolsList(line: string): string {
  try {
    const msg = JSON.parse(line);
    if (Array.isArray(msg.result?.tools)) {
      msg.result.tools = [...msg.result.tools, ...EXTRA_TOOLS];
      return JSON.stringify(msg);
    }
  } catch {}
  return line;
}

function broadcast(session: Session, line: string) {
  const ev = `event: message\ndata: ${line}\n\n`;
  if (session.streams.size === 0) session.buffer.push(ev);
  else for (const r of session.streams) r.write(ev);
}

// ── Auth helpers ───────────────────────────────────────────────────────────────
function checkMcpAuth(req: IncomingMessage): boolean {
  if (!cfg.mcp_token) return true;
  const auth = req.headers["authorization"];
  if (auth?.startsWith("Bearer ") && auth.slice(7) === cfg.mcp_token)
    return true;
  const u = new URL(req.url ?? "/", "http://x");
  return u.searchParams.get("token") === cfg.mcp_token;
}

// ── HTTP helpers ───────────────────────────────────────────────────────────────
function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((res, rej) => {
    const c: Buffer[] = [];
    req.on("data", (d) => c.push(d));
    req.on("end", () => res(Buffer.concat(c).toString()));
    req.on("error", rej);
  });
}

function cors(res: ServerResponse) {
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS");
  res.setHeader(
    "Access-Control-Allow-Headers",
    "Content-Type, Mcp-Session-Id, Authorization",
  );
  res.setHeader("Access-Control-Expose-Headers", "Mcp-Session-Id");
}

function respondJson(res: ServerResponse, status: number, body: unknown) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(body));
}

// ── Handlers ───────────────────────────────────────────────────────────────────
async function handleMcp(req: IncomingMessage, res: ServerResponse) {
  cors(res);
  if (req.method === "OPTIONS") {
    res.writeHead(204).end();
    return;
  }
  if (!checkMcpAuth(req)) {
    res.writeHead(401).end("Unauthorized");
    return;
  }

  if (req.method === "DELETE") {
    const id = req.headers["mcp-session-id"] as string | undefined;
    if (id) {
      sessions.get(id)?.streams.forEach((r) => r.end());
      sessions.delete(id);
    }
    res.writeHead(204).end();
    return;
  }

  if (req.method === "GET") {
    const id = req.headers["mcp-session-id"] as string | undefined;
    const session = id ? sessions.get(id) : undefined;
    if (!session) {
      res.writeHead(400).end("Unknown Mcp-Session-Id");
      return;
    }
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      Connection: "keep-alive",
    });
    for (const ev of session.buffer) res.write(ev);
    session.buffer = [];
    session.streams.add(res);
    req.on("close", () => session.streams.delete(res));
    return;
  }

  if (req.method === "POST") {
    const body = await readBody(req);
    let msg: any;
    try {
      msg = JSON.parse(body);
    } catch {
      res.writeHead(400).end("Invalid JSON");
      return;
    }

    // initialize → new session; return the initialize result inline (not via SSE)
    if (msg.method === "initialize") {
      const id = crypto.randomUUID();
      const session = createSession(id);
      session.write(body);
      const initResult = await session.firstResponse;
      res
        .writeHead(200, {
          "Content-Type": "application/json",
          "Mcp-Session-Id": id,
        })
        .end(initResult);
      return;
    }

    const id = req.headers["mcp-session-id"] as string | undefined;
    const session = id ? sessions.get(id) : undefined;
    if (!session) {
      res.writeHead(400).end("Unknown Mcp-Session-Id");
      return;
    }

    // intercept extra tool calls
    if (
      msg.method === "tools/call" &&
      EXTRA_TOOLS.some((t) => t.name === msg.params?.name)
    ) {
      const text = await handleExtraTool(
        msg.params.name,
        msg.params.arguments ?? {},
      );
      broadcast(
        session,
        JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          result: { content: [{ type: "text", text }] },
        }),
      );
      res.writeHead(202).end();
      return;
    }

    session.write(body);
    res.writeHead(202).end();
    return;
  }

  res.writeHead(405).end();
}

function handleFiles(
  req: IncomingMessage,
  res: ServerResponse,
  filename: string,
) {
  cors(res);
  if (req.method === "OPTIONS") {
    res.writeHead(204).end();
    return;
  }
  if (!checkMcpAuth(req)) {
    res.writeHead(401).end("Unauthorized");
    return;
  }
  const safe = path.basename(filename);
  const full = path.join(FILES_DIR, safe);
  if (!fs.existsSync(full) || !fs.statSync(full).isFile()) {
    res.writeHead(404).end("Not found");
    return;
  }
  const ext = path.extname(safe).toLowerCase();
  const mime: Record<string, string> = {
    ".pdf": "application/pdf",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".svg": "image/svg+xml",
    ".json": "application/json",
    ".txt": "text/plain; charset=utf-8",
    ".md": "text/plain; charset=utf-8",
  };
  res.writeHead(200, {
    "Content-Type": mime[ext] ?? "application/octet-stream",
    "Content-Disposition": `attachment; filename="${safe}"`,
  });
  fs.createReadStream(full).pipe(res);
}

// ── Login UI ───────────────────────────────────────────────────────────────────
const LOGIN_HTML = (error = "", url = "") => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>familiar-mcp</title>
<style>
  *{box-sizing:border-box;margin:0;padding:0}
  body{font-family:system-ui,sans-serif;background:#f5f5f5;display:flex;align-items:center;justify-content:center;min-height:100vh}
  .card{background:#fff;border-radius:12px;box-shadow:0 2px 16px #0001;padding:2rem;width:100%;max-width:400px}
  h1{font-size:1.2rem;margin-bottom:1.5rem;color:#111}
  label{display:block;font-size:.85rem;color:#555;margin-bottom:.25rem;margin-top:1rem}
  input{width:100%;padding:.6rem .8rem;border:1px solid #ddd;border-radius:6px;font-size:.95rem;outline:none}
  input:focus{border-color:#6c63ff}
  button{margin-top:1.5rem;width:100%;padding:.7rem;background:#6c63ff;color:#fff;border:none;border-radius:6px;font-size:1rem;cursor:pointer}
  button:hover{background:#574fd6}
  .error{margin-top:1rem;color:#c00;font-size:.85rem}
  .status{margin-top:1rem;font-size:.85rem;color:#555;line-height:1.5}
  .connected{color:#080}
</style>
</head>
<body>
<div class="card">
  <h1>familiar-mcp</h1>
  ${
    cfg.familiar_url && cfg.familiar_token
      ? `<div class="status connected">✓ Connected to ${cfg.familiar_url}</div>
       <form method="POST" action="/logout" style="margin-top:1rem">
         <button style="background:#c00">Disconnect</button>
       </form>`
      : `<form method="POST" action="/login">
        <label>Familiar server URL</label>
        <input name="url" type="url" placeholder="https://familiar.example.com" value="${url}" required>
        <label>Username</label>
        <input name="name" type="text" autocomplete="username" required>
        <label>Password</label>
        <input name="password" type="password" autocomplete="current-password" required>
        ${error ? `<div class="error">${error}</div>` : ""}
        <button type="submit">Connect</button>
       </form>`
  }
</div>
</body>
</html>`;

async function handleLogin(req: IncomingMessage, res: ServerResponse) {
  if (req.method === "GET") {
    res.writeHead(200, { "Content-Type": "text/html" }).end(LOGIN_HTML());
    return;
  }
  if (req.method === "POST") {
    const body = await readBody(req);
    const params = new URLSearchParams(body);
    const url = params.get("url")?.replace(/\/$/, "") ?? "";
    const name = params.get("name") ?? "";
    const password = params.get("password") ?? "";
    try {
      const r = await fetch(`${url}/api/sessions`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name, password }),
      });
      if (!r.ok) {
        const text = await r.text();
        res
          .writeHead(200, { "Content-Type": "text/html" })
          .end(LOGIN_HTML(`Login failed: ${r.status} ${text}`, url));
        return;
      }
      const { token } = (await r.json()) as { token: string };
      cfg.familiar_url = url;
      cfg.familiar_token = token;
      saveConfig(cfg);
      res.writeHead(303, { Location: "/" }).end();
    } catch (e: any) {
      res
        .writeHead(200, { "Content-Type": "text/html" })
        .end(LOGIN_HTML(`Error: ${e.message}`, url));
    }
    return;
  }
  res.writeHead(405).end();
}

async function handleLogout(req: IncomingMessage, res: ServerResponse) {
  if (req.method !== "POST") {
    res.writeHead(405).end();
    return;
  }
  if (cfg.familiar_url && cfg.familiar_token) {
    fetch(`${cfg.familiar_url}/api/sessions`, {
      method: "DELETE",
      headers: { Authorization: `Bearer ${cfg.familiar_token}` },
    }).catch(() => {});
  }
  cfg.familiar_url = null;
  cfg.familiar_token = null;
  saveConfig(cfg);
  res.writeHead(303, { Location: "/" }).end();
}

// ── Router ─────────────────────────────────────────────────────────────────────
const server = createServer(async (req, res) => {
  try {
    const u = new URL(req.url ?? "/", `http://localhost:${PORT}`);
    if (u.pathname === "/mcp") return await handleMcp(req, res);
    const fm = u.pathname.match(/^\/files\/(.+)$/);
    if (fm) return handleFiles(req, res, fm[1]);
    if (u.pathname === "/login" || u.pathname === "/")
      return await handleLogin(req, res);
    if (u.pathname === "/logout") return await handleLogout(req, res);
    res.writeHead(404).end();
  } catch (e: any) {
    res.writeHead(500).end(e.message);
  }
});

server.listen(PORT, () => {
  const connected = cfg.familiar_url && cfg.familiar_token;
  console.log(`familiar-mcp running`);
  console.log(`  Setup  → http://localhost:${PORT}`);
  console.log(`  MCP    → http://localhost:${PORT}/mcp`);
  if (connected) console.log(`  Status → connected to ${cfg.familiar_url}`);
  else console.log(`  Status → not connected (open the setup page to log in)`);
});
