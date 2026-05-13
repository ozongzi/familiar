import { useCallback, useMemo, useRef, useState, useEffect } from "react";
import type {
  ChatBubble,
  TextBubble,
  ToolBubble,
  UploadBubble,
  WsServerEvent,
  Message,
} from "../api/types";
import { getServerBase } from "../utils/tauri";
import { api } from "../api/client";

const BASE = () => getServerBase();

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

export type InterruptMode = "interrupt" | "abort";

const SSE_REATTACH_DELAY_MS = 500;

function uid() {
  return Math.random().toString(36).slice(2);
}

const STREAM_ID_KEY_PREFIX = "familiar_stream_id:";

function streamStorageKey(conversationId: string) {
  return `${STREAM_ID_KEY_PREFIX}${conversationId}`;
}

function persistStreamId(conversationId: string, streamId: string) {
  sessionStorage.setItem(streamStorageKey(conversationId), streamId);
}

function readPersistedStreamId(conversationId: string): string | null {
  return sessionStorage.getItem(streamStorageKey(conversationId));
}

function clearPersistedStreamId(conversationId: string | null) {
  if (!conversationId) return;
  sessionStorage.removeItem(streamStorageKey(conversationId));
}

function extractStreamingWidgetCode(raw: string): string | null {
  const keyMatch = raw.match(/"widget_code"\s*:\s*"/);
  if (!keyMatch || keyMatch.index === undefined) return null;
  const valueStart = keyMatch.index + keyMatch[0].length;
  const rest = raw.slice(valueStart);
  let value = "";
  let i = 0;
  while (i < rest.length) {
    const ch = rest[i];
    if (ch === "\\") {
      if (i + 1 < rest.length) {
        const next = rest[i + 1];
        const escapes: Record<string, string> = {
          '"': '"',
          "\\": "\\",
          "/": "/",
          "'": "'",
          b: "\b",
          f: "\f",
          n: "\n",
          r: "\r",
          t: "\t",
        };
        if (next === "u" && i + 5 < rest.length) {
          const hex = rest.slice(i + 2, i + 6);
          if (/^[0-9a-fA-F]{4}$/.test(hex)) {
            value += String.fromCharCode(parseInt(hex, 16));
            i += 6;
            continue;
          }
        }
        value += escapes[next] ?? next;
        i += 2;
      } else break;
    } else if (ch === '"') {
      return value;
    } else {
      value += ch;
      i++;
    }
  }
  return value.length > 0 ? value : null;
}

// Extract loading_messages array from partial JSON args during streaming.
// Returns the array of strings if the key + at least one complete string is present.
function extractLoadingMessages(raw: string): string[] | null {
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (Array.isArray(parsed.loading_messages)) {
      return (parsed.loading_messages as unknown[]).filter(
        (m): m is string => typeof m === "string",
      );
    }
  } catch {
    // Partial JSON — try a simple regex scan for the first few completed strings
    const m = raw.match(
      /"loading_messages"\s*:\s*\[((?:[^\]]*?"[^"]*"[^\]]*?)+)/,
    );
    if (!m) return null;
    const inner = m[1];
    const items: string[] = [];
    const re = /"((?:[^"\\]|\\.)*)"/g;
    let hit: RegExpExecArray | null;
    while ((hit = re.exec(inner)) !== null) {
      try {
        items.push(JSON.parse(`"${hit[1]}"`));
      } catch {
        items.push(hit[1]);
      }
    }
    return items.length > 0 ? items : null;
  }
  return null;
}

function tryParseWidgetArgs(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function extractWidgetCode(result: unknown): string | null {
  if (typeof result === "string") return result || null;
  if (result && typeof result === "object") {
    const r = result as Record<string, unknown>;
    if (typeof r.widget_code === "string") return r.widget_code || null;
    if (typeof r.html === "string") return r.html || null;
    if (typeof r.content === "string") return r.content || null;
  }
  return null;
}

function extractDescription(raw: string): string | null {
  const m = raw.match(/"description"\s*:\s*"((?:[^"\\]|\\.)*)(")?/);
  if (!m) return null;
  const value = m[1];
  if (!value) return null;
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

// ─── SSE helpers ─────────────────────────────────────────────────────────────

async function* readSseStream(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  signal: AbortSignal,
): AsyncGenerator<string> {
  const decoder = new TextDecoder();
  let buffer = "";

  while (!signal.aborted) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    // Split on double newline (SSE event separator)
    const parts = buffer.split("\n\n");
    buffer = parts.pop() ?? "";

    for (const part of parts) {
      for (const line of part.split("\n")) {
        if (line.startsWith("data: ")) {
          yield line.slice(6);
        }
      }
    }
  }
}

async function openSseStream(
  streamId: string,
  tok: string,
  onEvent: (data: string) => void,
  onError: (e: Error) => void,
  signal: AbortSignal,
) {
  try {
    const res = await fetch(`${BASE()}/api/stream/${streamId}`, {
      headers: { Authorization: `Bearer ${tok}` },
      signal,
    });
    if (!res.ok || !res.body) {
      onError(new Error(`SSE connect failed: ${res.status}`));
      return;
    }
    const reader = res.body.getReader();
    for await (const data of readSseStream(reader, signal)) {
      onEvent(data);
    }
  } catch (e) {
    if ((e as { name?: string }).name === "AbortError") return;
    onError(e as Error);
  }
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

interface UseChatOptions {
  onConversationCreated?: (id: string, firstMessage: string) => void;
  shouldAutoTitle?: (id: string) => boolean;
}

export function useChat(
  conversationId: string | null,
  token: string | null,
  createConversation: () => Promise<string | null>,
  options: UseChatOptions = {},
) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [tokenUsage, setTokenUsage] = useState<{
    contextTokens: number;
    compactTriggerTokens: number;
  } | null>(null);


  // SSE refs (replace WebSocket refs)
  const streamIdRef = useRef<string | null>(null);
  const abortControllerRef = useRef<AbortController | null>(null);

  // Saved stream_id when an "ask" event arrives, so answerQuestion can use it
  // even after "done" clears streamIdRef.
  const askStreamIdRef = useRef<string | null>(null);

  // Coordination refs (kept from original)
  const attachedConvRef = useRef<string | null>(null);
  const reattachingRef = useRef(false);
  const historyReadyRef = useRef(false);
  const activeTextKeyRef = useRef<string | null>(null);
  const statusRef = useRef<ChatStatus>("idle");
  const onConversationCreatedRef = useRef(options.onConversationCreated);
  const shouldAutoTitleRef = useRef(options.shouldAutoTitle);
  const autoTitleAttemptedRef = useRef<Set<string>>(new Set());
  const hasPriorUserMessageRef = useRef(false);
  const spawnToolArgsRef = useRef<
    Map<string, { name: string; argsRaw: string }>
  >(new Map());
  const activeToolKeysRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    onConversationCreatedRef.current = options.onConversationCreated;
    shouldAutoTitleRef.current = options.shouldAutoTitle;
  });

  function updateStatus(s: ChatStatus) {
    statusRef.current = s;
    setStatus(s);
  }

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
    };
  }, []);

  // ─── Bubble helpers ───────────────────────────────────────────────────────

  function sealActiveText() {
    const key = activeTextKeyRef.current;
    if (!key) return;
    setBubbles((prev) =>
      prev.map((b) =>
        b.key === key && b.kind === "text" ? { ...b, streaming: false } : b,
      ),
    );
    activeTextKeyRef.current = null;
  }

  function ensureActiveText(): string {
    if (activeTextKeyRef.current) return activeTextKeyRef.current;
    const key = uid();
    activeTextKeyRef.current = key;
    const bubble: TextBubble = {
      kind: "text",
      key,
      role: "assistant",
      content: "",
      reasoning: "",
      streaming: true,
    };
    setBubbles((prev) => [...prev, bubble]);
    return key;
  }

  function appendSpawnText(chunk: string) {
    setBubbles((prev) => {
      for (let i = prev.length - 1; i >= 0; i--) {
        const b = prev[i];
        if (b.kind !== "tool" || b.name !== "spawn" || !b.pending) continue;
        const events = b.spawnEvents ?? [];
        const last = events[events.length - 1];
        const next = [...prev];
        if (last?.kind === "text") {
          next[i] = {
            ...b,
            spawnEvents: [
              ...events.slice(0, -1),
              { kind: "text", key: last.key, content: last.content + chunk },
            ],
          };
        } else {
          next[i] = {
            ...b,
            spawnEvents: [
              ...events,
              { kind: "text", key: uid(), content: chunk },
            ],
          };
        }
        return next;
      }
      return prev;
    });
  }

  function upsertSpawnChild(child: ToolBubble) {
    setBubbles((prev) => {
      for (let i = prev.length - 1; i >= 0; i--) {
        const b = prev[i];
        if (b.kind !== "tool" || b.name !== "spawn" || !b.pending) continue;
        const events = b.spawnEvents ?? [];
        const idx = events.findIndex(
          (e) => e.kind === "tool" && e.bubble.key === child.key,
        );
        const next = [...prev];
        next[i] = {
          ...b,
          spawnEvents:
            idx >= 0
              ? events.map((e, j) =>
                  j === idx ? { kind: "tool" as const, bubble: child } : e,
                )
              : [...events, { kind: "tool" as const, bubble: child }],
        };
        return next;
      }
      return prev;
    });
  }

  // ─── Public helpers ───────────────────────────────────────────────────────

  const setHistory = useCallback(
    (msgs: Message[]) => {
      hasPriorUserMessageRef.current = msgs.some(
        (m) => m.role === "user" && !!m.content?.trim(),
      );
      const toolResultMap = new Map<string, unknown>();
      const toolImagesMap = new Map<string, string[]>();
      for (const m of msgs) {
        if (m.role === "tool" && m.tool_call_id && m.content) {
          let parsed: unknown = m.content;
          try {
            const outer = JSON.parse(m.content);
            // The backend stores tool results as Vec<agentix::Content>, serialized as
            // [{type:"text",text:"<json>"}]. Unwrap to get the actual result value.
            if (Array.isArray(outer)) {
              const textBlock = (outer as unknown[]).find(
                (b): b is { type: string; text: string } =>
                  b !== null &&
                  typeof b === "object" &&
                  (b as Record<string, unknown>).type === "text" &&
                  typeof (b as Record<string, unknown>).text === "string",
              );
              if (textBlock) {
                try {
                  parsed = JSON.parse(textBlock.text);
                } catch {
                  parsed = textBlock.text;
                }
              } else {
                parsed = outer;
              }

              // Extract image parts — sandbox refs become /api/files URLs
              type ImageBlock = {
                type: "image";
                data: { url?: string; base64?: string };
                mime_type: string;
              };
              const imageUrls = (outer as unknown[])
                .filter(
                  (b): b is ImageBlock =>
                    b !== null &&
                    typeof b === "object" &&
                    (b as Record<string, unknown>).type === "image",
                )
                .map((b) => {
                  const raw = b.data?.url ?? "";
                  if (raw.startsWith("__sandbox__:")) {
                    const filename = raw.slice("__sandbox__:".length);
                    const params = new URLSearchParams({
                      path: `/workspace/${filename}`,
                    });
                    if (conversationId)
                      params.set("conversation_id", conversationId);
                    if (token) params.set("token", token);
                    return `/api/files?${params.toString()}`;
                  }
                  if (b.data?.base64) {
                    return `data:${b.mime_type};base64,${b.data.base64}`;
                  }
                  return raw;
                })
                .filter(Boolean);
              if (imageUrls.length > 0) {
                toolImagesMap.set(m.tool_call_id, imageUrls);
              }
            } else {
              parsed = outer;
            }
          } catch {
            /* leave as string */
          }
          toolResultMap.set(m.tool_call_id, parsed);
        }
      }

      const history: ChatBubble[] = [];
      const consumedMsgIds = new Set<number>();

      for (let mi = 0; mi < msgs.length; mi++) {
        const m = msgs[mi];
        if (m.role === "system" || m.role === "tool") continue;
        if (consumedMsgIds.has(m.id)) continue;

        if (m.role === "assistant" && m.tool_calls) {
          type RawToolCall = {
            id: string;
            name: string;
            arguments: string;
          };
          let calls: RawToolCall[] = [];
          try {
            calls = JSON.parse(m.tool_calls) as RawToolCall[];
          } catch {
            /* skip */
          }
          const hasAssistantText = !!(m.content && m.content.trim().length > 0);
          const hasAssistantReasoning = !!(
            m.reasoning && m.reasoning.trim().length > 0
          );
          if (hasAssistantText || hasAssistantReasoning) {
            const key = uid();
            history.push({
              kind: "text",
              key,
              role: "assistant",
              content: m.content ?? "",
              reasoning: m.reasoning ?? "",
              streaming: m.streaming,
              msgId: m.id,
              siblings: m.siblings,
            });
            if (m.streaming) activeTextKeyRef.current = key;
          }
          for (const tc of calls) {
            const { id, name, arguments: argsRaw = "" } = tc;
            if (!id || !name) continue;
            let result = toolResultMap.get(id) ?? null;

            // `ask` 工具将 { __ask__: true, ... } 作为 tool result 存入 DB。
            // 用户的真实回答是作为下一条 user 消息存的。
            // 检测到 __ask__ 标记（或 result 为 null）时，找到那条回答，嵌入为 { answer: ... }，
            // 并标记该 user 消息已消费（不再渲染成独立气泡）。
            const isAskMarker =
              result !== null &&
              typeof result === "object" &&
              (result as Record<string, unknown>).__ask__ === true;
            if (name === "ask" && (result === null || isAskMarker)) {
              result = null; // reset — will be set to { answer } if found
              for (let j = mi + 1; j < msgs.length; j++) {
                const next = msgs[j];
                if (next.role !== "user") continue;
                const c = next.content;
                if (!c || !c.trim()) continue;
                // 跳过文件上传和多模态消息——它们不是文字回答。
                let isFileUpload = false;
                try {
                  const p = JSON.parse(c) as Record<string, unknown>;
                  if (p.__type === "file_upload") isFileUpload = true;
                } catch {
                  /* not JSON */
                }
                if (isFileUpload || c.startsWith("__multimodal__:")) break;
                result = { answer: c };
                consumedMsgIds.add(next.id);
                break;
              }
            }

            // visualize → 恢复 widgetCode 到 ToolBubble
            const widgetCode =
              name === "visualize" && result !== null
                ? (extractWidgetCode(result) ??
                  extractWidgetCode(tryParseWidgetArgs(argsRaw)))
                : null;

            // diagram → 从 args 里取回 mermaid 代码
            let diagramCode: string | undefined;
            if (name === "diagram") {
              try {
                const parsed = JSON.parse(argsRaw) as Record<string, unknown>;
                if (typeof parsed.code === "string") diagramCode = parsed.code;
              } catch {
                /* ignore */
              }
            }

            const historyImages = toolImagesMap.get(id);
            const toolBubble: ToolBubble = {
              kind: "tool",
              key: `tool-${id}`,
              role: "tool",
              name,
              description: extractDescription(argsRaw) ?? "",
              argsRaw,
              result,
              pending: result === null,
              ...(widgetCode ? { widgetCode } : {}),
              ...(diagramCode ? { diagramCode } : {}),
              ...(historyImages && historyImages.length > 0
                ? { images: historyImages }
                : {}),
            };
            history.push(toolBubble);
          }
          continue;
        }

        const hasContent = !!(m.content && m.content.trim().length > 0);
        const hasAssistantReasoningOnly =
          m.role === "assistant" &&
          !!(m.reasoning && m.reasoning.trim().length > 0);
        if (
          (m.role === "user" || m.role === "assistant") &&
          (hasContent || hasAssistantReasoningOnly)
        ) {
          const content = m.content ?? "";
          if (m.role === "user") {
            try {
              const parsed = JSON.parse(content) as Record<string, unknown>;
              if (
                parsed.__type === "file_upload" &&
                typeof parsed.filename === "string" &&
                typeof parsed.path === "string" &&
                typeof parsed.size === "number"
              ) {
                const uploadBubble: UploadBubble = {
                  kind: "upload",
                  key: uid(),
                  role: "user",
                  filename: parsed.filename,
                  path: parsed.path,
                  size: parsed.size,
                  conversationId: conversationId ?? undefined,
                };
                history.push(uploadBubble);
                continue;
              }
            } catch {
              /* not JSON — fall through */
            }

            // Multimodal message: "__multimodal__:[{type,text/image_url,...}]"
            if (content.startsWith("__multimodal__:")) {
              const json = content.slice("__multimodal__:".length);
              try {
                // agentix 0.10.1 internal tag format:
                //   { type: "text", text: "..." }
                //   { type: "image", data: { base64: "..." }, mime_type: "..." }
                type Part =
                  | { type: "text"; text: string }
                  | {
                      type: "image";
                      data: { base64?: string; url?: string };
                      mime_type: string;
                    };
                const parts = JSON.parse(json) as Part[];

                // File upload: text part is the file_upload JSON marker
                const textContent = parts
                  .filter(
                    (p): p is Extract<Part, { type: "text" }> =>
                      p.type === "text",
                  )
                  .map((p) => p.text)
                  .join("");
                try {
                  const parsed = JSON.parse(textContent) as Record<
                    string,
                    unknown
                  >;
                  if (
                    parsed.__type === "file_upload" &&
                    typeof parsed.filename === "string" &&
                    typeof parsed.path === "string" &&
                    typeof parsed.size === "number"
                  ) {
                    history.push({
                      kind: "upload",
                      key: uid(),
                      role: "user",
                      filename: parsed.filename,
                      path: parsed.path,
                      size: parsed.size,
                      conversationId: conversationId ?? undefined,
                    });
                    continue;
                  }
                } catch {
                  /* text is not file_upload JSON */
                }

                const images = parts
                  .filter(
                    (p): p is Extract<Part, { type: "image" }> =>
                      p.type === "image",
                  )
                  .map((p) =>
                    p.data.base64
                      ? `data:${p.mime_type};base64,${p.data.base64}`
                      : (p.data.url ?? ""),
                  )
                  .filter(Boolean);
                history.push({
                  kind: "text",
                  key: uid(),
                  role: "user",
                  content: textContent,
                  reasoning: "",
                  streaming: false,
                  images: images.length > 0 ? images : undefined,
                  msgId: m.id,
                  siblings: m.siblings,
                });
                continue;
              } catch {
                /* fall through to plain text */
              }
            }
          }

          const key = uid();
          history.push({
            kind: "text",
            key,
            role: m.role as "user" | "assistant",
            content,
            reasoning: m.reasoning ?? "",
            streaming: m.streaming,
            msgId: m.id,
            siblings: m.siblings,
          });
          if (m.role === "assistant" && m.streaming)
            activeTextKeyRef.current = key;
        }
      }

      setBubbles(history);
      historyReadyRef.current = true;
    },
    [conversationId, token],
  );

  const clearBubbles = useCallback(() => {
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    streamIdRef.current = null;
    setBubbles([]);
    activeTextKeyRef.current = null;
    spawnToolArgsRef.current.clear();
    activeToolKeysRef.current.clear();
    updateStatus("idle");
    setErrorMsg(null);
    attachedConvRef.current = null;
    historyReadyRef.current = false;
    hasPriorUserMessageRef.current = false;
  }, []);

  const addUploadBubble = useCallback(
    (filename: string, path: string, size: number) => {
      const bubble: UploadBubble = {
        kind: "upload",
        key: uid(),
        role: "user",
        filename,
        path,
        size,
        conversationId: conversationId ?? undefined,
      };
      setBubbles((prev) => [...prev, bubble]);
    },
    [],
  );

  // ─── Event processor ──────────────────────────────────────────────────────

  const processEventRef = useRef<(event: WsServerEvent) => boolean>(
    () => false,
  );

  useEffect(() => {
    processEventRef.current = (event: WsServerEvent): boolean => {
      if (event.type === "user_interrupt") {
        sealActiveText();
        return false;
      } else if (event.type === "aborted") {
        sealActiveText();
        updateStatus("idle");
        clearPersistedStreamId(attachedConvRef.current);
        return true;
      } else if (event.type === "partial_sync") {
        // Server sends accumulated partial content on reconnect.
        // Replace the streaming bubble content entirely instead of appending.
        const key = ensureActiveText();
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === key && b.kind === "text"
              ? {
                  ...b,
                  content: event.content,
                  reasoning: event.reasoning ?? "",
                }
              : b,
          ),
        );
      } else if (event.type === "reasoning_token") {
        const key = ensureActiveText();
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === key && b.kind === "text"
              ? { ...b, reasoning: b.reasoning + event.content }
              : b,
          ),
        );
      } else if (event.type === "token") {
        if (event.source === "spawn") {
          appendSpawnText(event.content);
          return false;
        }
        const key = ensureActiveText();
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === key && b.kind === "text"
              ? { ...b, content: b.content + event.content }
              : b,
          ),
        );
      } else if (event.type === "tool_call") {
        if (event.source === "spawn") {
          const acc = spawnToolArgsRef.current.get(event.id) ?? {
            name: event.name,
            argsRaw: "",
          };
          acc.argsRaw += event.delta;
          spawnToolArgsRef.current.set(event.id, acc);
          upsertSpawnChild({
            kind: "tool",
            key: `spawn-tool-${event.id}`,
            role: "tool",
            name: event.name,
            description: extractDescription(acc.argsRaw) ?? "",
            argsRaw: acc.argsRaw,
            result: null,
            pending: true,
          });
          return false;
        }
        const toolKey = `tool-${event.id}`;
        if (activeToolKeysRef.current.has(toolKey)) {
          setBubbles((prev) =>
            prev.map((b) => {
              if (b.kind !== "tool" || b.key !== toolKey) return b;
              if (b.name === "visualize") {
                const rawArgs = (b._rawArgs ?? "") + event.delta;
                const parsed = extractWidgetCode(tryParseWidgetArgs(rawArgs));
                const streamed = parsed ?? extractStreamingWidgetCode(rawArgs);
                const loadingMsgs =
                  b.widgetLoadingMessages ??
                  extractLoadingMessages(rawArgs) ??
                  undefined;
                return {
                  ...b,
                  _rawArgs: rawArgs,
                  widgetCode: streamed ?? b.widgetCode,
                  widgetLoadingMessages: loadingMsgs,
                };
              }
              const newArgsRaw = b.argsRaw + event.delta;
              const desc = extractDescription(newArgsRaw);
              return {
                ...b,
                argsRaw: newArgsRaw,
                description: desc ?? b.description,
              };
            }),
          );
        } else {
          activeToolKeysRef.current.add(toolKey);
          sealActiveText();
          setBubbles((prev) => {
            const toolBubble: ToolBubble = {
              kind: "tool",
              key: toolKey,
              role: "tool",
              name: event.name,
              description: extractDescription(event.delta) ?? "",
              argsRaw: event.delta,
              result: null,
              pending: true,
              ...(event.name === "visualize"
                ? {
                    widgetCode: extractStreamingWidgetCode(event.delta) ?? "",
                    widgetLoadingMessages:
                      extractLoadingMessages(event.delta) ?? undefined,
                    _rawArgs: event.delta,
                  }
                : event.name === "diagram"
                  ? {
                      diagramCode: "",
                      _rawArgs: event.delta,
                    }
                  : {}),
            };
            return [...prev, toolBubble];
          });
        }
      } else if (event.type === "tool_progress") {
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === `tool-${event.id}` && b.kind === "tool"
              ? {
                  ...b,
                  progressLines: [...(b.progressLines ?? []), event.progress],
                }
              : b,
          ),
        );
      } else if (event.type === "tool_result") {
        if (event.source === "spawn") {
          const acc = spawnToolArgsRef.current.get(event.id);
          spawnToolArgsRef.current.delete(event.id);
          upsertSpawnChild({
            kind: "tool",
            key: `spawn-tool-${event.id}`,
            role: "tool",
            name: acc?.name ?? event.name,
            description: extractDescription(acc?.argsRaw ?? "") ?? "",
            argsRaw: acc?.argsRaw ?? "",
            result: event.result,
            pending: false,
          });
          return false;
        }

        // visualize tool → 把 widgetCode 写进 ToolBubble
        if (event.name === "visualize") {
          setBubbles((prev) =>
            prev.map((b) => {
              if (b.key !== `tool-${event.id}` || b.kind !== "tool") return b;
              const rawArgs = b._rawArgs ?? b.argsRaw;
              const widgetCode =
                extractWidgetCode(event.result) ??
                extractWidgetCode(tryParseWidgetArgs(rawArgs)) ??
                b.widgetCode;
              return {
                ...b,
                result: event.result,
                pending: false,
                ...(widgetCode ? { widgetCode } : {}),
              };
            }),
          );
          return false;
        }

        // diagram tool → 把 code 写进 ToolBubble
        if (event.name === "diagram") {
          setBubbles((prev) =>
            prev.map((b) => {
              if (b.key !== `tool-${event.id}` || b.kind !== "tool") return b;
              const rawArgs = b._rawArgs ?? b.argsRaw;
              let diagramCode: string | undefined;
              try {
                const parsed = JSON.parse(rawArgs) as Record<string, unknown>;
                if (typeof parsed.code === "string") diagramCode = parsed.code;
              } catch {
                /* ignore */
              }
              return {
                ...b,
                result: event.result,
                pending: false,
                ...(diagramCode ? { diagramCode } : {}),
              };
            }),
          );
          return false;
        }

        setBubbles((prev) =>
          prev.map((b) =>
            b.key === `tool-${event.id}` && b.kind === "tool"
              ? {
                  ...b,
                  result: event.result,
                  pending: false,
                  ...(event.args && event.args.length > 0
                    ? { argsRaw: event.args }
                    : {}),
                  ...(event.images && event.images.length > 0
                    ? { images: event.images.map((img) => img.url) }
                    : {}),
                }
              : b,
          ),
        );
      } else if (event.type === "ask") {
        // Save stream_id now — "done" will arrive next and clear streamIdRef.
        askStreamIdRef.current = streamIdRef.current;
      } else if (event.type === "usage") {
        setTokenUsage({
          contextTokens: event.context_tokens,
          compactTriggerTokens: event.compact_trigger_tokens,
        });
      } else if (event.type === "done") {
        sealActiveText();
        updateStatus("idle");
        clearPersistedStreamId(attachedConvRef.current);
        return true;
      } else if (event.type === "error") {
        clearPersistedStreamId(attachedConvRef.current);
        const key = activeTextKeyRef.current;
        if (key) {
          setBubbles((prev) => prev.filter((b) => b.key !== key));
          activeTextKeyRef.current = null;
        }
        updateStatus("error");
        setErrorMsg(event.message);
        return true;
      }
      return false;
    };
  });

  // ─── Stream actions ───────────────────────────────────────────────────────

  const abort = useCallback(() => {
    const streamId = streamIdRef.current;
    if (!streamId || !token) return;
    fetch(`${BASE()}/api/stream/${streamId}/abort`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    }).catch(console.error);
  }, [token]);

  // ─── Reattach ─────────────────────────────────────────────────────────────

  const reattach = useCallback((convId: string, tok: string) => {
    // Use per-conversation persisted stream id only (for page-refresh recovery).
    // Never use streamIdRef.current here — it belongs to whatever conv was last active.
    const existingStreamId = readPersistedStreamId(convId);

    if (attachedConvRef.current === convId) return;
    attachedConvRef.current = convId;
    reattachingRef.current = true;

    const ac = new AbortController();
    abortControllerRef.current = ac;

    const startStream = (streamId: string) => {
      streamIdRef.current = streamId;
      openSseStream(
        streamId,
        tok,
        (data) => {
          let event: WsServerEvent;
          try {
            event = JSON.parse(data) as WsServerEvent;
          } catch {
            return;
          }

          if (
            statusRef.current === "idle" &&
            event.type !== "done" &&
            event.type !== "aborted" &&
            event.type !== "error"
          ) {
            reattachingRef.current = false;
            updateStatus("streaming");
          }

          const finished = processEventRef.current(event);
          if (finished) {
            reattachingRef.current = false;
            streamIdRef.current = null;
            clearPersistedStreamId(convId);
            abortControllerRef.current = null;
          }
        },
        (err) => {
          console.error("[SSE] reattach error:", err);
          reattachingRef.current = false;
          if (
            statusRef.current === "streaming" ||
            statusRef.current === "connecting"
          ) {
            setTimeout(() => {
              if (
                statusRef.current !== "streaming" &&
                statusRef.current !== "connecting"
              )
                return;
              attachedConvRef.current = null;
              reattach(convId, tok);
            }, SSE_REATTACH_DELAY_MS);
            return;
          }
        },
        ac.signal,
      );
    };

    if (existingStreamId) {
      startStream(existingStreamId);
      return;
    }

    // No stream_id — ask the backend for a fresh one that points to the
    // conversation's broadcast channel (works even if generation is ongoing).
    fetch(`${BASE()}/api/conversations/${convId}/reattach`, {
      method: "POST",
      headers: { Authorization: `Bearer ${tok}` },
      signal: ac.signal,
    })
      .then((r) => r.json())
      .then((data: { stream_id?: string }) => {
        if (!data.stream_id) {
          reattachingRef.current = false;
          return;
        }
        startStream(data.stream_id);
      })
      .catch((e) => {
        if ((e as { name?: string }).name === "AbortError") return;
        reattachingRef.current = false;
      });
  }, []);

  // ─── Answer (ask tool response) ───────────────────────────────────────────

  const answerQuestion = useCallback(
    (text: string) => {
      const streamId = askStreamIdRef.current;
      const convId = attachedConvRef.current;
      if (!streamId || !convId || !token) return;
      askStreamIdRef.current = null;

      fetch(`${BASE()}/api/stream/${streamId}/answer`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ content: text }),
      })
        .then((r) => (r.ok ? r.json() : null))
        .then((data: { stream_id?: string } | null) => {
          if (!data?.stream_id) return;
          const newStreamId = data.stream_id;
          streamIdRef.current = newStreamId;
          persistStreamId(convId, newStreamId);
          activeTextKeyRef.current = null;
          updateStatus("streaming");

          const ac = new AbortController();
          abortControllerRef.current = ac;

          openSseStream(
            newStreamId,
            token,
            (evData) => {
              let event: WsServerEvent;
              try {
                event = JSON.parse(evData) as WsServerEvent;
              } catch {
                return;
              }
              const finished = processEventRef.current(event);
              if (finished) {
                streamIdRef.current = null;
                clearPersistedStreamId(convId);
                abortControllerRef.current = null;
                ac.abort();
              }
            },
            (err) => {
              if (
                statusRef.current !== "streaming" &&
                statusRef.current !== "connecting"
              )
                return;
              console.warn(
                "[SSE] answer stream disconnected, trying reattach:",
                err,
              );
              updateStatus("connecting");
              abortControllerRef.current = null;
              setTimeout(() => {
                attachedConvRef.current = null;
                reattach(convId, token);
              }, SSE_REATTACH_DELAY_MS);
            },
            ac.signal,
          );
        })
        .catch(console.error);
    },
    [token, reattach],
  );

  // ─── Send ─────────────────────────────────────────────────────────────────

  const send = useCallback(
    async (text: string) => {
      if (!token) return;
      if (statusRef.current === "connecting") return;

      if (statusRef.current === "streaming") {
        const oldStreamId = streamIdRef.current;
        const convId = attachedConvRef.current;
        if (!oldStreamId || !convId) return;

        const userBubble: TextBubble = {
          kind: "text",
          key: uid(),
          role: "user",
          content: text,
          reasoning: "",
          streaming: false,
        };
        setBubbles((prev) => [...prev, userBubble]);
        hasPriorUserMessageRef.current = true;

        let newStreamId: string;
        try {
          const res = await fetch(
            `${BASE()}/api/stream/${oldStreamId}/interrupt`,
            {
              method: "POST",
              headers: {
                Authorization: `Bearer ${token}`,
                "Content-Type": "application/json",
              },
              body: JSON.stringify({ content: text }),
            },
          );
          if (!res.ok) return;
          const data = (await res.json()) as { stream_id: string };
          newStreamId = data.stream_id;
        } catch {
          return;
        }

        // Tear down old SSE connection.
        abortControllerRef.current?.abort();
        abortControllerRef.current = null;

        streamIdRef.current = newStreamId;
        persistStreamId(convId, newStreamId);
        activeTextKeyRef.current = null;
        updateStatus("streaming");

        const ac = new AbortController();
        abortControllerRef.current = ac;

        openSseStream(
          newStreamId,
          token,
          (data) => {
            let event: WsServerEvent;
            try {
              event = JSON.parse(data) as WsServerEvent;
            } catch {
              return;
            }
            const finished = processEventRef.current(event);
            if (finished) {
              streamIdRef.current = null;
              clearPersistedStreamId(convId);
              abortControllerRef.current = null;
              ac.abort();
            }
          },
          (err) => {
            if (
              statusRef.current !== "streaming" &&
              statusRef.current !== "connecting"
            )
              return;
            console.warn(
              "[SSE] interrupt stream disconnected, trying reattach:",
              err,
            );
            updateStatus("connecting");
            abortControllerRef.current = null;
            setTimeout(() => {
              attachedConvRef.current = null;
              reattach(convId, token);
            }, SSE_REATTACH_DELAY_MS);
          },
          ac.signal,
        );
        return;
      }

      if (reattachingRef.current) {
        abortControllerRef.current?.abort();
        reattachingRef.current = false;
        abortControllerRef.current = null;
        clearPersistedStreamId(attachedConvRef.current);
        attachedConvRef.current = null;
        streamIdRef.current = null;
      }

      let convId = conversationId;
      let createdConversation = false;
      if (!convId) {
        convId = await createConversation();
        if (!convId) {
          setErrorMsg("创建对话失败，请重试");
          return;
        }
        createdConversation = true;
      }

      const hadPriorUserMessage = hasPriorUserMessageRef.current;
      const shouldTitle =
        createdConversation || shouldAutoTitleRef.current?.(convId) === true;
      if (
        shouldTitle &&
        !hadPriorUserMessage &&
        !autoTitleAttemptedRef.current.has(convId)
      ) {
        autoTitleAttemptedRef.current.add(convId);
        onConversationCreatedRef.current?.(convId, text);
      }

      setErrorMsg(null);
      activeTextKeyRef.current = null;

      const userBubble: TextBubble = {
        kind: "text",
        key: uid(),
        role: "user",
        content: text,
        reasoning: "",
        streaming: false,
      };
      setBubbles((prev) => [...prev, userBubble]);
      hasPriorUserMessageRef.current = true;
      updateStatus("connecting");

      // Step 1: POST message → get stream_id
      let streamId: string;
      try {
        const body: Record<string, unknown> = { content: text };
        const res = await fetch(
          `${BASE()}/api/conversations/${convId}/messages`,
          {
            method: "POST",
            headers: {
              Authorization: `Bearer ${token}`,
              "Content-Type": "application/json",
            },
            body: JSON.stringify(body),
          },
        );
        if (!res.ok) {
          const err = await res
            .json()
            .catch(() => ({ error: `HTTP ${res.status}` }));
          throw new Error(
            (err as { error?: string }).error ?? `HTTP ${res.status}`,
          );
        }
        const data = (await res.json()) as {
          stream_id: string;
          user_message_id?: number;
          siblings?: number[];
        };
        streamId = data.stream_id;
        if (data.user_message_id != null) {
          const mid = data.user_message_id;
          const sibs = data.siblings;
          setBubbles((prev) =>
            prev.map((b) =>
              b.key === userBubble.key
                ? { ...b, msgId: mid, ...(sibs ? { siblings: sibs } : {}) }
                : b,
            ),
          );
        }
      } catch (e) {
        updateStatus("error");
        setErrorMsg((e as Error).message ?? "发送失败，请重试");
        return;
      }

      streamIdRef.current = streamId;
      attachedConvRef.current = convId;
      persistStreamId(convId, streamId);
      updateStatus("streaming");

      const ac = new AbortController();
      abortControllerRef.current = ac;

      // Step 2: subscribe to SSE stream
      openSseStream(
        streamId,
        token,
        (data) => {
          let event: WsServerEvent;
          try {
            event = JSON.parse(data) as WsServerEvent;
          } catch {
            return;
          }
          console.log("[SSE] event:", event);

          const finished = processEventRef.current(event);
          if (finished) {
            streamIdRef.current = null;
            clearPersistedStreamId(convId);
            abortControllerRef.current = null;
            ac.abort();
          }
        },
        (err) => {
          if (
            statusRef.current !== "streaming" &&
            statusRef.current !== "connecting"
          )
            return;
          console.warn("[SSE] stream disconnected, trying reattach:", err);
          updateStatus("connecting");
          abortControllerRef.current = null;
          setTimeout(() => {
            if (!convId || !token) return;
            attachedConvRef.current = null;
            reattach(convId, token);
          }, SSE_REATTACH_DELAY_MS);
        },
        ac.signal,
      );
    },
    [conversationId, token, createConversation, reattach],
  );

  // A pending compact (inject in the chain without a downstream anchor)
  // hard-locks the chat: the backend rejects new user messages with 409
  // until retryCompact succeeds.
  const pendingCompact = useMemo(() => {
    let injectIdx = -1;
    for (let i = bubbles.length - 1; i >= 0; i--) {
      const b = bubbles[i];
      if (
        b.kind === "text" &&
        b.role === "user" &&
        b.content.startsWith("[系统检查点]")
      ) {
        injectIdx = i;
        break;
      }
    }
    if (injectIdx < 0) return false;
    for (let i = injectIdx; i < bubbles.length; i++) {
      const b = bubbles[i];
      if (b.kind === "text" && b.summaryStartId != null) {
        return false;
      }
    }
    return true;
  }, [bubbles]);

  const retryCompact = useCallback(async () => {
    if (!token || !conversationId) return;
    if (statusRef.current === "streaming" || statusRef.current === "connecting")
      return;

    setErrorMsg(null);
    updateStatus("connecting");

    let streamId: string;
    try {
      const res = await api.retryCompact(token, conversationId);
      streamId = res.stream_id;
    } catch (e) {
      updateStatus("error");
      setErrorMsg((e as Error).message ?? "重试压缩失败");
      return;
    }

    streamIdRef.current = streamId;
    attachedConvRef.current = conversationId;
    persistStreamId(conversationId, streamId);
    updateStatus("streaming");

    const ac = new AbortController();
    abortControllerRef.current = ac;

    openSseStream(
      streamId,
      token,
      (data) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(data) as WsServerEvent;
        } catch {
          return;
        }
        const finished = processEventRef.current(event);
        if (finished) {
          streamIdRef.current = null;
          clearPersistedStreamId(conversationId);
          abortControllerRef.current = null;
          ac.abort();
        }
      },
      (err) => {
        if (
          statusRef.current !== "streaming" &&
          statusRef.current !== "connecting"
        )
          return;
        console.warn("[SSE] retry-compact stream disconnected:", err);
        updateStatus("connecting");
        abortControllerRef.current = null;
        setTimeout(() => {
          if (!conversationId || !token) return;
          attachedConvRef.current = null;
          reattach(conversationId, token);
        }, SSE_REATTACH_DELAY_MS);
      },
      ac.signal,
    );
  }, [conversationId, token, reattach]);

  const branch = useCallback(
    async (msgId: number, bubbleKey: string, newText: string) => {
      if (!token || !conversationId) return;
      const res = await fetch(
        `${BASE()}/api/conversations/${conversationId}/branch`,
        {
          method: "POST",
          headers: {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({ message_id: msgId }),
        },
      );
      if (!res.ok) return;

      // Keep everything before the edited user bubble; drop it and everything after.
      setBubbles((prev) => {
        const idx = prev.findIndex((b) => b.key === bubbleKey);
        return idx >= 0 ? prev.slice(0, idx) : prev;
      });

      updateStatus("idle");
      await send(newText);
    },
    [token, conversationId, send],
  );

  const switchSibling = useCallback(
    async (targetMsgId: number) => {
      if (!token || !conversationId) return;
      const res = await fetch(
        `${BASE()}/api/conversations/${conversationId}/activate`,
        {
          method: "POST",
          headers: {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({ message_id: targetMsgId }),
        },
      );
      if (!res.ok) return;
      const refreshed = await fetch(
        `${BASE()}/api/conversations/${conversationId}/messages`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      if (!refreshed.ok) return;
      const msgs: Message[] = await refreshed.json();
      setHistory(msgs);
    },
    [token, conversationId, setHistory],
  );

  return {
    bubbles,
    status,
    errorMsg,
    send,
    abort,
    answerQuestion,
    reattach,
    setHistory,
    clearBubbles,
    addUploadBubble,
    branch,
    switchSibling,
    tokenUsage,
    pendingCompact,
    retryCompact,
  };
}
