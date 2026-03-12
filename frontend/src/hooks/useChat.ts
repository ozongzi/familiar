import { useCallback, useRef, useState, useEffect } from "react";
import type {
  ChatBubble,
  TextBubble,
  ToolBubble,
  UploadBubble,
  WsServerEvent,
  Message,
} from "../api/types";

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

export type InterruptMode = "interrupt" | "abort";

function uid() {
  return Math.random().toString(36).slice(2);
}

/**
 * Try to extract the `description` field value from a partial or complete
 * JSON args string.  Returns null if the field hasn't arrived yet.
 *
 * We assume `description` is always the FIRST parameter in the JSON object,
 * so it appears very early in the streaming delta.  The regex handles:
 *   - partial values still being streamed (no closing quote yet)
 *   - complete values (closing quote present)
 *   - escaped characters inside the string
 */
function extractDescription(raw: string): string | null {
  // Match: "description"\s*:\s*"<captured>" where the closing quote is optional
  // (stream may be cut off mid-value).
  const m = raw.match(/"description"\s*:\s*"((?:[^"\\]|\\.)*)(")?/);
  if (!m) return null;
  const value = m[1];
  if (!value) return null;
  // Unescape JSON string escapes so the UI shows clean text.
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

interface UseChatOptions {
  /** Called with the new conversation ID and first user message text right
   *  after a conversation is created in draft mode.  The caller uses this to
   *  fire-and-forget an auto-title request without blocking the send path. */
  onConversationCreated?: (id: string, firstMessage: string) => void;
}

export function useChat(
  conversationId: string | null,
  token: string | null,
  /** Factory that creates a real conversation and returns its id, used when
   *  the chat is in draft mode (conversationId === null) and the user sends
   *  the first message. */
  createConversation: () => Promise<string | null>,
  options: UseChatOptions = {},
) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  const wsLiveRef = useRef<WebSocket | null>(null);
  const attachedConvRef = useRef<string | null>(null);
  const reattachingRef = useRef(false);
  const historyReadyRef = useRef(false);
  const activeTextKeyRef = useRef<string | null>(null);
  const statusRef = useRef<ChatStatus>("idle");
  const onConversationCreatedRef = useRef(options.onConversationCreated);
  useEffect(() => {
    onConversationCreatedRef.current = options.onConversationCreated;
  });

  function updateStatus(s: ChatStatus) {
    statusRef.current = s;
    setStatus(s);
  }

  // ── Cleanup on unmount ────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      wsRef.current?.close(1000);
    };
  }, []);

  // ── Helpers ────────────────────────────────────────────────────────────────

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

  function appendSpawnOutput(chunk: string): boolean {
    let updated = false;
    setBubbles((prev) => {
      for (let i = prev.length - 1; i >= 0; i--) {
        const b = prev[i];
        if (b.kind === "tool" && b.name === "spawn" && b.pending) {
          updated = true;
          const next = [...prev];
          next[i] = {
            ...b,
            spawnOutput: (b.spawnOutput ?? "") + chunk,
          };
          return next;
        }
      }
      return prev;
    });
    return updated;
  }

  // ── Public API ─────────────────────────────────────────────────────────────

  const setHistory = useCallback((msgs: Message[]) => {
    const toolResultMap = new Map<string, unknown>();
    for (const m of msgs) {
      if (m.role === "tool" && m.tool_call_id && m.content) {
        let parsed: unknown = m.content;
        try { parsed = JSON.parse(m.content); } catch { /* leave as string */ }
        toolResultMap.set(m.tool_call_id, parsed);
      }
    }

    const history: ChatBubble[] = [];

    for (const m of msgs) {
      if (m.role === "system" || m.role === "tool") continue;

      if (m.role === "assistant" && m.tool_calls) {
        type RawToolCall = {
          id: string;
          type?: string;
          function?: { name: string; arguments: string };
        };
        let calls: RawToolCall[] = [];
        try { calls = JSON.parse(m.tool_calls) as RawToolCall[]; } catch { /* skip */ }
        for (const tc of calls) {
          if (!tc.id || !tc.function) continue;
          const result = toolResultMap.get(tc.id) ?? null;
          const argsRaw = tc.function.arguments ?? "";
          const toolBubble: ToolBubble = {
            kind: "tool",
            key: `tool-${tc.id}`,
            role: "tool",
            name: tc.function.name,
            description: extractDescription(argsRaw) ?? "",
            argsRaw,
            result,
            pending: result === null,
            spawnOutput: undefined,
          };
          history.push(toolBubble);
        }
        if (m.content && m.content.trim().length > 0) {
          history.push({
            kind: "text",
            key: uid(),
            role: "assistant",
            content: m.content,
            reasoning: m.reasoning ?? "",
            streaming: false,
          });
        }
        continue;
      }

      if (
        (m.role === "user" || m.role === "assistant") &&
        m.content &&
        m.content.trim().length > 0
      ) {
        // Detect special file-upload user messages saved by the upload endpoint.
        if (m.role === "user") {
          try {
            const parsed = JSON.parse(m.content) as Record<string, unknown>;
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
              };
              history.push(uploadBubble);
              continue;
            }
          } catch {
            /* not JSON — fall through to normal text bubble */
          }
        }

        history.push({
          kind: "text",
          key: uid(),
          role: m.role as "user" | "assistant",
          content: m.content,
          reasoning: m.reasoning ?? "",
          streaming: false,
        });
      }
    }

    setBubbles(history);
    historyReadyRef.current = true;
  }, []);

  const clearBubbles = useCallback(() => {
    setBubbles([]);
    activeTextKeyRef.current = null;
    updateStatus("idle");
    setErrorMsg(null);
    attachedConvRef.current = null;
    historyReadyRef.current = false;
  }, []);

  /** Add a file-upload bubble to the chat (called after a successful upload). */
  const addUploadBubble = useCallback(
    (filename: string, path: string, size: number) => {
      const bubble: UploadBubble = {
        kind: "upload",
        key: uid(),
        role: "user",
        filename,
        path,
        size,
      };
      setBubbles((prev) => [...prev, bubble]);
    },
    [],
  );

  // ── Interrupt / abort ──────────────────────────────────────────────────────

  const interrupt = useCallback((text: string) => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (statusRef.current !== "streaming") return;
    const userBubble: TextBubble = {
      kind: "text",
      key: uid(),
      role: "user",
      content: text,
      reasoning: "",
      streaming: false,
    };
    setBubbles((prev) => [...prev, userBubble]);
    ws.send(JSON.stringify({ type: "interrupt", content: text }));
  }, []);

  const abort = useCallback(() => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: "abort" }));
  }, []);

  const answerQuestion = useCallback((text: string) => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: "answer", content: text }));
  }, []);

  // ── processEvent ref ───────────────────────────────────────────────────────

  const processEventRef = useRef<(event: WsServerEvent) => boolean>(() => false);

  useEffect(() => {
    processEventRef.current = (event: WsServerEvent): boolean => {
      if (event.type === "user_interrupt") {
        return false;
      } else if (event.type === "aborted") {
        sealActiveText();
        updateStatus("idle");
        return true;
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
        if (event.source === "spawn" && appendSpawnOutput(event.content)) {
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
        if (event.source === "spawn") return false;
        setBubbles((prev) => {
          const exists = prev.some(
            (b) => b.key === `tool-${event.id}` && b.kind === "tool",
          );
          if (exists) {
            return prev.map((b) => {
              if (b.key !== `tool-${event.id}` || b.kind !== "tool") return b;
              const newArgsRaw = b.argsRaw + event.delta;
              // Try to extract description from the growing argsRaw.
              // We require description to be the first parameter so the pattern
              // appears very early in the stream.
              const desc = extractDescription(newArgsRaw);
              return {
                ...b,
                argsRaw: newArgsRaw,
                description: desc ?? b.description,
              };
            });
          }
          sealActiveText();
          const toolBubble: ToolBubble = {
            kind: "tool",
            key: `tool-${event.id}`,
            role: "tool",
            name: event.name,
            description: extractDescription(event.delta) ?? "",
            argsRaw: event.delta,
            result: null,
            pending: true,
            spawnOutput: event.name === "spawn" ? "" : undefined,
          };
          return [...prev, toolBubble];
        });
      } else if (event.type === "tool_result") {
        if (event.source === "spawn") return false;
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === `tool-${event.id}` && b.kind === "tool"
              ? {
                  ...b,
                  result: event.result,
                  pending: false,
                  spawnOutput:
                    b.name === "spawn" &&
                    (!b.spawnOutput || b.spawnOutput.length === 0) &&
                    event.result &&
                    typeof event.result === "object" &&
                    "result" in (event.result as Record<string, unknown>)
                      ? String((event.result as Record<string, unknown>).result ?? "")
                      : b.spawnOutput,
                }
              : b,
          ),
        );
      } else if (event.type === "done") {
        sealActiveText();
        updateStatus("idle");
        return true;
      } else if (event.type === "error") {
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

  // ── Reattach ───────────────────────────────────────────────────────────────

  const reattach = useCallback((convId: string, tok: string) => {
    if (wsLiveRef.current) return;
    if (attachedConvRef.current === convId) return;

    attachedConvRef.current = convId;

    const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${wsProtocol}://${location.host}/ws/${convId}`);
    wsRef.current = ws;
    wsLiveRef.current = ws;

    ws.addEventListener("open", () => {
      reattachingRef.current = true;
      ws.send(JSON.stringify({ token: tok }));
      ws.send(JSON.stringify({ type: "reattach" }));
    });

    ws.addEventListener("message", (ev) => {
      let event: WsServerEvent;
      try { event = JSON.parse(ev.data as string) as WsServerEvent; }
      catch { return; }

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
        ws.close(1000);
        wsRef.current = null;
        wsLiveRef.current = null;
      }
    });

    ws.addEventListener("error", () => {
      reattachingRef.current = false;
      wsRef.current = null;
      wsLiveRef.current = null;
    });

    ws.addEventListener("close", () => {
      reattachingRef.current = false;
      wsRef.current = null;
      wsLiveRef.current = null;
    });
  }, []);

  // ── Send ───────────────────────────────────────────────────────────────────

  const send = useCallback(
    async (text: string) => {
      if (!token) return;
      if (statusRef.current === "connecting") return;

      if (statusRef.current === "streaming") {
        interrupt(text);
        return;
      }

      if (reattachingRef.current) {
        const existing = wsLiveRef.current;
        if (existing && existing.readyState === WebSocket.OPEN) {
          existing.close(1000);
        }
        reattachingRef.current = false;
        wsRef.current = null;
        wsLiveRef.current = null;
        attachedConvRef.current = null;
      }

      // Draft mode: create the conversation first, then send.
      let convId = conversationId;
      if (!convId) {
        convId = await createConversation();
        if (!convId) {
          setErrorMsg("创建对话失败，请重试");
          return;
        }
        // Notify caller so it can fire auto-title logic.
        onConversationCreatedRef.current?.(convId, text);
      }

      setErrorMsg(null);
      activeTextKeyRef.current = null;

      // Optimistically add user bubble.
      const userBubble: TextBubble = {
        kind: "text",
        key: uid(),
        role: "user",
        content: text,
        reasoning: "",
        streaming: false,
      };
      setBubbles((prev) => [...prev, userBubble]);

      updateStatus("connecting");

      const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(`${wsProtocol}://${location.host}/ws/${convId}`);
      wsRef.current = ws;
      wsLiveRef.current = ws;
      attachedConvRef.current = convId;

      ws.addEventListener("open", () => {
        ws.send(JSON.stringify({ token }));
        ws.send(JSON.stringify({ content: text }));
        updateStatus("streaming");
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try { event = JSON.parse(ev.data as string) as WsServerEvent; }
        catch { return; }

        const finished = processEventRef.current(event);
        if (finished) {
          ws.close(1000);
          wsRef.current = null;
          wsLiveRef.current = null;
        }
      });

      ws.addEventListener("error", () => {
        // The browser fires an error event before every close, including
        // normal closes initiated by ws.close(1000) after "done".  Only
        // treat it as a real error if we're still actively streaming.
        if (statusRef.current !== "streaming" && statusRef.current !== "connecting") {
          wsRef.current = null;
          wsLiveRef.current = null;
          return;
        }
        const key = activeTextKeyRef.current;
        if (key) {
          setBubbles((prev) => prev.filter((b) => b.key !== key));
          activeTextKeyRef.current = null;
        }
        updateStatus("error");
        setErrorMsg("连接出错，请重试");
        wsRef.current = null;
        wsLiveRef.current = null;
      });

      ws.addEventListener("close", (ev) => {
        wsRef.current = null;
        wsLiveRef.current = null;
        if (ev.code !== 1000 && ev.code !== 1001 && statusRef.current === "streaming") {
          const key = activeTextKeyRef.current;
          if (key) {
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            activeTextKeyRef.current = null;
          }
          updateStatus("error");
          setErrorMsg("连接已断开，请重试");
        }
      });
    },
    [conversationId, token, interrupt, createConversation],
  );

  return {
    bubbles,
    status,
    errorMsg,
    send,
    interrupt,
    abort,
    answerQuestion,
    reattach,
    setHistory,
    clearBubbles,
    addUploadBubble,
  };
}
