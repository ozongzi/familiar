#!/usr/bin/env node
/**
 * tunnel-bridge.cjs
 *
 * 启动 @playwright/mcp 子进程（stdio），连接服务器 WS 隧道，双向转发 MCP 消息。
 *
 * 环境变量：
 *   FAMILIAR_TOKEN        — Bearer token
 *   FAMILIAR_SERVER       — 服务器地址，如 https://familiar.example.com
 *   FAMILIAR_RESOURCE_DIR — Tauri resource 目录（由 Rust 传入）
 */

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");

const TOKEN = process.env.FAMILIAR_TOKEN;
const SERVER = process.env.FAMILIAR_SERVER;
const RESOURCE_DIR = process.env.FAMILIAR_RESOURCE_DIR;

const LOG_FILE = path.join(os.tmpdir(), "familiar-tunnel.log");
function log(msg) {
  const line = `[${new Date().toISOString()}] ${msg}\n`;
  process.stderr.write(line);
  try { fs.appendFileSync(LOG_FILE, line); } catch {}
}

log("tunnel-bridge 启动");
log(`NODE: ${process.execPath}`);
log(`TOKEN: ${TOKEN ? "已设置" : "未设置"}`);
log(`SERVER: ${SERVER}`);
log(`RESOURCE_DIR: ${RESOURCE_DIR}`);

if (!TOKEN || !SERVER) {
  log("错误：缺少 FAMILIAR_TOKEN 或 FAMILIAR_SERVER");
  process.exit(1);
}

const wsBase = SERVER.replace(/^http/, "ws").replace(/\/$/, "");
const TUNNEL_URL = `${wsBase}/api/tunnel`;

// ws 包路径（装在 mcp-bundle 里）
const WS_MODULE = RESOURCE_DIR
  ? path.join(RESOURCE_DIR, "mcp-bundle", "node_modules", "ws", "index.js")
  : path.join(__dirname, "mcp-bundle", "node_modules", "ws", "index.js");

const MCP_CLI = RESOURCE_DIR
  ? path.join(RESOURCE_DIR, "mcp-bundle", "node_modules", "@playwright", "mcp", "cli.js")
  : path.join(__dirname, "mcp-bundle", "node_modules", "@playwright", "mcp", "cli.js");

log(`ws 模块: ${WS_MODULE} (存在: ${fs.existsSync(WS_MODULE)})`);
log(`MCP CLI: ${MCP_CLI} (存在: ${fs.existsSync(MCP_CLI)})`);

const WebSocket = require(WS_MODULE);

// ── 主逻辑 ────────────────────────────────────────────────────────────────────

const RECONNECT_MS = 3000;
const PING_MS = 20000;

let mcpProc = null;
let ws = null;
let stopping = false;
let pingTimer = null;

function startMcp() {
  if (mcpProc) return;
  log("启动 @playwright/mcp...");

  const isWindows = process.platform === "win32";
  const channelArgs = isWindows ? ["--channel", "msedge"] : [];

  const [cmd, args] = fs.existsSync(MCP_CLI)
    ? [process.execPath, [MCP_CLI, ...channelArgs]]
    : ["npx", ["-y", "@playwright/mcp@latest", ...channelArgs]];

  log(`启动 MCP: ${cmd} ${args.join(" ")}`);

  mcpProc = spawn(cmd, args, {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env },
  });

  mcpProc.stderr?.on("data", (chunk) => {
    log(`[MCP stderr] ${chunk.toString().trim()}`);
  });

  mcpProc.on("exit", (code) => {
    log(`MCP 进程退出 (code ${code})`);
    mcpProc = null;
    if (!stopping) setTimeout(startMcp, 1000);
  });

  mcpProc.stdout.on("data", (chunk) => {
    if (ws?.readyState === WebSocket.OPEN) {
      chunk.toString().split("\n").filter(Boolean).forEach((l) => ws.send(l));
    }
  });
}

function connect() {
  if (stopping) return;
  log(`连接 ${TUNNEL_URL}...`);

  ws = new WebSocket(TUNNEL_URL, {
    headers: { Authorization: `Bearer ${TOKEN}` },
  });

  ws.on("open", () => {
    log("隧道已连接");
    pingTimer = setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: "ping" }));
      }
    }, PING_MS);
  });

  ws.on("message", (data) => {
    const text = data.toString();
    try { if (JSON.parse(text)?.type === "pong") return; } catch {}
    if (mcpProc?.stdin?.writable) mcpProc.stdin.write(text + "\n");
  });

  ws.on("close", (code) => {
    clearInterval(pingTimer);
    log(`连接断开 (${code})`);
    ws = null;
    if (!stopping) setTimeout(connect, RECONNECT_MS);
  });

  ws.on("error", (e) => log(`WS 错误: ${e.message}`));
}

function shutdown() {
  stopping = true;
  clearInterval(pingTimer);
  ws?.close();
  mcpProc?.kill();
  process.exit(0);
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

startMcp();
connect();
