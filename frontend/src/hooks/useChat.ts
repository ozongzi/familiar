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
    const res = await fetch(`/api/stream/${streamId}`, {
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

  // SSE refs (replace WebSocket refs)
  const streamIdRef = useRef<string | null>(null);
  const abortControllerRef = useRef<AbortController | null>(null);

  // Coordination refs (kept from original)
  const attachedConvRef = useRef<string | null>(null);
  const reattachingRef = useRef(false);
  const historyReadyRef = useRef(false);
  const activeTextKeyRef = useRef<string | null>(null);
  const statusRef = useRef<ChatStatus>("idle");
  const onConversationCreatedRef = useRef(options.onConversationCreated);
  const spawnToolArgsRef = useRef<
    Map<string, { name: string; argsRaw: string }>
  >(new Map());

  useEffect(() => {
    onConversationCreatedRef.current = options.onConversationCreated;
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

  const setHistory = useCallback((msgs: Message[]) => {
    const toolResultMap = new Map<string, unknown>();
    for (const m of msgs) {
      if (m.role === "tool" && m.tool_call_id && m.content) {
        let parsed: unknown = m.content;
        try {
          parsed = JSON.parse(m.content);
        } catch {
          /* leave as string */
        }
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
        try {
          calls = JSON.parse(m.tool_calls) as RawToolCall[];
        } catch {
          /* skip */
        }
        for (const tc of calls) {
          console.warn("tc = ", tc);
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
            /* not JSON — fall through */
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
    spawnToolArgsRef.current.clear();
    updateStatus("idle");
    setErrorMsg(null);
    attachedConvRef.current = null;
    historyReadyRef.current = false;
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
        return false;
      } else if (event.type === "aborted") {
        sealActiveText();
        updateStatus("idle");
        clearPersistedStreamId(attachedConvRef.current);
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
        setBubbles((prev) => {
          const exists = prev.some(
            (b) => b.key === `tool-${event.id}` && b.kind === "tool",
          );
          console.log(
            `[tool_call] id=${event.id}, exists=${exists}, currentBubbles=${prev.length}`,
            prev.map((b) => b.key),
          );
          if (exists) {
            return prev.map((b) => {
              if (b.key !== `tool-${event.id}` || b.kind !== "tool") return b;
              const newArgsRaw = b.argsRaw + event.delta;
              const desc = extractDescription(newArgsRaw);
              return {
                ...b,
                argsRaw: newArgsRaw,
                description: desc ?? b.description,
              };
            });
          }
          sealActiveText();
          console.warn("appending", event);
          const toolBubble: ToolBubble = {
            kind: "tool",
            key: `tool-${event.id}`,
            role: "tool",
            name: event.name,
            description: extractDescription(event.delta) ?? "",
            argsRaw: event.delta,
            result: null,
            pending: true,
          };
          return [...prev, toolBubble];
        });
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
                }
              : b,
          ),
        );
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

  const interrupt = useCallback(
    (text: string) => {
      const streamId = streamIdRef.current;
      if (!streamId || !token) return;
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

      fetch(`/api/stream/${streamId}/interrupt`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ content: text }),
      }).catch(console.error);
    },
    [token],
  );

  const abort = useCallback(() => {
    const streamId = streamIdRef.current;
    if (!streamId || !token) return;
    fetch(`/api/stream/${streamId}/abort`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    }).catch(console.error);
  }, [token]);

  const answerQuestion = useCallback(
    (text: string) => {
      const streamId = streamIdRef.current;
      if (!streamId || !token) return;
      fetch(`/api/stream/${streamId}/answer`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ content: text }),
      }).catch(console.error);
    },
    [token],
  );

  // ─── Reattach ─────────────────────────────────────────────────────────────

  const reattach = useCallback((convId: string, tok: string) => {
    // Reattach using in-memory stream id first; fall back to persisted id (for refresh recovery).
    const streamId = streamIdRef.current ?? readPersistedStreamId(convId);
    if (!streamId) return;
    streamIdRef.current = streamId;
    if (attachedConvRef.current === convId) return;

    attachedConvRef.current = convId;
    reattachingRef.current = true;

    const ac = new AbortController();
    abortControllerRef.current = ac;

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
      },
      ac.signal,
    );
  }, []);

  // ─── Send ─────────────────────────────────────────────────────────────────

  const send = useCallback(
    async (text: string) => {
      if (!token) return;
      if (statusRef.current === "connecting") return;

      if (statusRef.current === "streaming") {
        interrupt(text);
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
      if (!convId) {
        convId = await createConversation();
        if (!convId) {
          setErrorMsg("创建对话失败，请重试");
          return;
        }
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
      updateStatus("connecting");

      // Step 1: POST message → get stream_id
      let streamId: string;
      try {
        const res = await fetch(`/api/conversations/${convId}/messages`, {
          method: "POST",
          headers: {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify({ content: text }),
        });
        if (!res.ok) {
          const err = await res
            .json()
            .catch(() => ({ error: `HTTP ${res.status}` }));
          throw new Error(
            (err as { error?: string }).error ?? `HTTP ${res.status}`,
          );
        }
        const data = (await res.json()) as { stream_id: string };
        streamId = data.stream_id;
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
          const key = activeTextKeyRef.current;
          if (key) {
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            activeTextKeyRef.current = null;
          }
          updateStatus("error");
          setErrorMsg(err.message ?? "连接出错，请重试");
          abortControllerRef.current = null;
        },
        ac.signal,
      );
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
