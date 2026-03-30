#!/usr/bin/env node
/**
 * tunnel-bridge.cjs
 *
 * 启动本地 MCP 子进程，各自连一条 WS 隧道，双向转发 MCP 消息。
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

const WebSocket = require(WS_MODULE);

const RECONNECT_MS = 3000;
const PING_MS = 20000;

let stopping = false;

// ── 单条 MCP ↔ 隧道 桥接 ──────────────────────────────────────────────────────

/**
 * 启动一个 stdio MCP 进程并与一条 WS 隧道连接桥接。
 * @param {string} label        日志标签
 * @param {string} cmd          可执行文件
 * @param {string[]} args       参数
 * @param {number|null} msgTimeoutMs  每条请求的超时（ms），超时则 kill+restart 进程；null = 不限时
 */
function bridgeStdio(label, cmd, args, msgTimeoutMs = null) {
  let proc = null;
  let ws = null;
  let pingTimer = null;
  let msgTimer = null;
  let pendingRequestId = null;

  function clearMsgTimer() {
    if (msgTimer) { clearTimeout(msgTimer); msgTimer = null; }
  }

  function killAndRestart() {
    log(`[${label}] 消息超时，发送错误响应`);
    clearMsgTimer();
    // 向服务器发一个 JSON-RPC error，让 backend worker 收到错误而不是一直等待
    // 不 kill 进程——playwright 会在自己的超时后恢复，kill 会把 Chrome 一起干掉
    if (pendingRequestId !== null && ws?.readyState === WebSocket.OPEN) {
      const errMsg = JSON.stringify({
        jsonrpc: "2.0",
        id: pendingRequestId,
        error: { code: -32000, message: "Tool timed out" },
      });
      ws.send(errMsg);
      log(`[${label}] 已发送超时错误响应 id=${pendingRequestId}`);
    }
    pendingRequestId = null;
  }

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
      clearMsgTimer();
      proc = null;
      if (!stopping) setTimeout(startProc, 1000);
    });
    proc.stdout.on("data", (chunk) => {
      const lines = chunk.toString().split("\n").filter(Boolean);
      lines.forEach((l) => {
        try {
          const msg = JSON.parse(l);
          if ("result" in msg || "error" in msg) {
            clearMsgTimer();
            pendingRequestId = null;
          }
        } catch {}
        if (ws?.readyState === WebSocket.OPEN) ws.send(l);
      });
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
      if (proc?.stdin?.writable) {
        proc.stdin.write(text + "\n");
        if (msgTimeoutMs) {
          // 记录请求 ID，超时时用来发 error 响应
          try {
            const parsed = JSON.parse(text);
            if (parsed.id !== undefined) pendingRequestId = parsed.id;
          } catch {}
          clearMsgTimer();
          msgTimer = setTimeout(killAndRestart, msgTimeoutMs);
        }
      }
    });
    ws.on("close", (code) => {
      clearInterval(pingTimer);
      clearMsgTimer();
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
  stopLocalFns.forEach((fn) => fn());
  process.exit(0);
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
