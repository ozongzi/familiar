#!/usr/bin/env node
/**
 * tunnel-bridge.cjs
 *
 * 启动 @playwright/mcp 及本地 MCP 子进程，各自连一条 WS 隧道，双向转发 MCP 消息。
 *
 * 环境变量：
 *   FAMILIAR_TOKEN        — Bearer token
 *   FAMILIAR_SERVER       — 服务器地址，如 https://familiar.example.com
 *   FAMILIAR_RESOURCE_DIR — Tauri resource 目录
 *   FAMILIAR_LOCAL_MCPS   — JSON 数组，本地 MCP 配置
 *                           [{ name, type: "stdio", command, args } | { name, type: "http", url }]
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
log(`TOKEN: ${TOKEN ? "已设置" : "未设置"}`);
log(`SERVER: ${SERVER}`);

if (!TOKEN || !SERVER) {
  log("错误：缺少 FAMILIAR_TOKEN 或 FAMILIAR_SERVER");
  process.exit(1);
}

const wsBase = SERVER.replace(/^http/, "ws").replace(/\/$/, "");
const TUNNEL_URL = `${wsBase}/api/tunnel`;

const WS_MODULE = RESOURCE_DIR
  ? path.join(RESOURCE_DIR, "mcp-bundle", "node_modules", "ws", "index.js")
  : path.join(__dirname, "mcp-bundle", "node_modules", "ws", "index.js");

const MCP_CLI = RESOURCE_DIR
  ? path.join(RESOURCE_DIR, "mcp-bundle", "node_modules", "@playwright", "mcp", "cli.js")
  : path.join(__dirname, "mcp-bundle", "node_modules", "@playwright", "mcp", "cli.js");

const WebSocket = require(WS_MODULE);

const RECONNECT_MS = 3000;
const PING_MS = 20000;

let stopping = false;

// ── 单条 MCP ↔ 隧道 桥接 ──────────────────────────────────────────────────────

/**
 * 启动一个 stdio MCP 进程并与一条 WS 隧道连接桥接。
 * @param {string} label  日志标签
 * @param {string} cmd    可执行文件
 * @param {string[]} args 参数
 */
function bridgeStdio(label, cmd, args) {
  let proc = null;
  let ws = null;
  let pingTimer = null;

  function startProc() {
    if (proc) return;
    log(`[${label}] 启动: ${cmd} ${args.join(" ")}`);
    proc = spawn(cmd, args, {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env },
    });
    proc.stderr?.on("data", (d) => log(`[${label}] stderr: ${d.toString().trim()}`));
    proc.on("exit", (code) => {
      log(`[${label}] 进程退出 (${code})`);
      proc = null;
      if (!stopping) setTimeout(startProc, 1000);
    });
    proc.stdout.on("data", (chunk) => {
      if (ws?.readyState === WebSocket.OPEN) {
        chunk.toString().split("\n").filter(Boolean).forEach((l) => ws.send(l));
      }
    });
  }

  function connectWs() {
    if (stopping) return;
    log(`[${label}] 连接隧道...`);
    ws = new WebSocket(TUNNEL_URL, { headers: { Authorization: `Bearer ${TOKEN}` } });
    ws.on("open", () => {
      log(`[${label}] 隧道已连接`);
      pingTimer = setInterval(() => {
        if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "ping" }));
      }, PING_MS);
    });
    ws.on("message", (data) => {
      const text = data.toString();
      try { if (JSON.parse(text)?.type === "pong") return; } catch {}
      if (proc?.stdin?.writable) proc.stdin.write(text + "\n");
    });
    ws.on("close", (code) => {
      clearInterval(pingTimer);
      log(`[${label}] 连接断开 (${code})`);
      ws = null;
      if (!stopping) setTimeout(connectWs, RECONNECT_MS);
    });
    ws.on("error", (e) => log(`[${label}] WS 错误: ${e.message}`));
  }

  startProc();
  connectWs();

  return () => {
    proc?.kill();
    ws?.close();
  };
}

// ── 启动 @playwright/mcp ──────────────────────────────────────────────────────

const isWindows = process.platform === "win32";
const channelArgs = isWindows ? ["--browser", "msedge"] : [];
const [playwrightCmd, playwrightArgs] = fs.existsSync(MCP_CLI)
  ? [process.execPath, [MCP_CLI, ...channelArgs]]
  : ["npx", ["-y", "@playwright/mcp@latest", ...channelArgs]];

const stopPlaywright = bridgeStdio("playwright", playwrightCmd, playwrightArgs);

// ── 启动本地 MCP ──────────────────────────────────────────────────────────────

let localMcps = [];
try {
  const raw = process.env.FAMILIAR_LOCAL_MCPS;
  if (raw) localMcps = JSON.parse(raw);
} catch (e) {
  log(`解析 FAMILIAR_LOCAL_MCPS 失败: ${e.message}`);
}

log(`本地 MCP 数量: ${localMcps.length}`);

const stopLocalFns = localMcps.map((mcp) => {
  if (mcp.type === "stdio") {
    const cmd = mcp.command;
    const args = mcp.args || [];
    return bridgeStdio(mcp.name || cmd, cmd, args);
  } else {
    log(`[${mcp.name}] 跳过（HTTP 类型由服务器直连，无需本地桥接）`);
    return () => {};
  }
});

// ── 退出清理 ──────────────────────────────────────────────────────────────────

function shutdown() {
  stopping = true;
  stopPlaywright();
  stopLocalFns.forEach((fn) => fn());
  process.exit(0);
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
