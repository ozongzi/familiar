import { useCallback, useRef, useState, useEffect } from "react";
import type {
  ChatBubble,
  TextBubble,
  ToolBubble,
  WsServerEvent,
} from "../api/types";

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

// During streaming the user can either inject a message mid-run or abort.
export type InterruptMode = "interrupt" | "abort";

function uid() {
  return Math.random().toString(36).slice(2);
}

export function useChat(conversationId: string | null, token: string | null) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  // Stable ref so abort/interrupt callbacks never go stale.
  const wsLiveRef = useRef<WebSocket | null>(null);
  // Track which conversationId we last attached to, to avoid double-attach.
  const attachedConvRef = useRef<string | null>(null);
  // True while a reattach WS is open but generation has not yet started
  // (status is still "idle"). Used by send() to detect and close it first.
  const reattachingRef = useRef(false);

  // True once setHistory has been called for the current conversation.
  // reattach() will not open a WS until this is true.
  const historyReadyRef = useRef(false);

  // Key of the assistant TextBubble that is currently accumulating tokens.
  // null means no active text segment yet (next token will create one).
  const activeTextKeyRef = useRef<string | null>(null);

  // statusRef so close/error handlers always read the latest value
  // without stale-closure issues.
  const statusRef = useRef<ChatStatus>("idle");

  function updateStatus(s: ChatStatus) {
    statusRef.current = s;
    setStatus(s);
  }

  // ── Helpers ────────────────────────────────────────────────────────────────

  /** Seal the current active text bubble (stop streaming). */
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

  /**
   * Ensure there is an active streaming text bubble for the assistant.
   * If one already exists, returns its key; otherwise creates a new one
   * and appends it to the list.
   */
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

  // ── Public API ─────────────────────────────────────────────────────────────

  const setHistory = useCallback(
    (
      msgs: Array<{
        role: string;
        content: string | null;
        tool_calls?: string | null;
        tool_call_id?: string | null;
        name?: string | null;
        reasoning?: string | null;
      }>,
    ) => {
      // Build a map of tool_call_id → result content for role==="tool" rows,
      // so we can attach results when we encounter the matching assistant tool_calls.
      const toolResultMap = new Map<string, unknown>();
      for (const m of msgs) {
        if (m.role === "tool" && m.tool_call_id && m.content) {
          let parsed: unknown = m.content;
          try {
            parsed = JSON.parse(m.content);
          } catch {
            // leave as string
          }
          toolResultMap.set(m.tool_call_id, parsed);
        }
      }

      const history: ChatBubble[] = [];

      for (const m of msgs) {
        // Skip system / tool-result rows (tool results are merged into ToolBubbles below)
        if (m.role === "system" || m.role === "tool") continue;

        if (m.role === "assistant" && m.tool_calls) {
          // Assistant message that issued one or more tool calls.
          // Parse the standard OpenAI tool_calls JSON array.
          type RawToolCall = {
            id: string;
            type?: string;
            function?: { name: string; arguments: string };
          };
          let calls: RawToolCall[] = [];
          try {
            calls = JSON.parse(m.tool_calls) as RawToolCall[];
          } catch {
            // malformed — skip
          }
          for (const tc of calls) {
            if (!tc.id || !tc.function) continue;
            const result = toolResultMap.get(tc.id) ?? null;
            const toolBubble: ToolBubble = {
              kind: "tool",
              key: `tool-${tc.id}`,
              role: "tool",
              name: tc.function.name,
              argsRaw: tc.function.arguments ?? "",
              result,
              pending: result === null,
            };
            history.push(toolBubble);
          }
          // If there's also text content in this assistant turn, add a text bubble after.
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

        // Regular user / assistant text message.
        if (
          (m.role === "user" || m.role === "assistant") &&
          m.content &&
          m.content.trim().length > 0
        ) {
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
    },
    [],
  );

  const clearBubbles = useCallback(() => {
    setBubbles([]);
    activeTextKeyRef.current = null;
    updateStatus("idle");
    setErrorMsg(null);
    attachedConvRef.current = null;
    historyReadyRef.current = false;
  }, []);

  // ── Interrupt / abort (usable while streaming) ─────────────────────────

  const interrupt = useCallback((text: string) => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (statusRef.current !== "streaming") return;

    // Show the injected message immediately as a user bubble.
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

  // ── Core WebSocket event processor (shared by send and reattach) ───────

  /**
   * Process a single parsed WsServerEvent, mutating bubble state.
   * Returns true if the event signals end-of-stream (done/aborted/error).
   *
   * Implementation is stored on a ref so event listeners can always call the
   * latest implementation without capturing a stale callback.
   */
  // Use a ref to hold the current processEvent implementation so callbacks
  // that are registered once (e.g. WebSocket listeners) can invoke the up-
  // to-date logic without requiring those callbacks to list processEvent in
  // their dependency arrays.
  const processEventRef = useRef<(event: WsServerEvent) => boolean>(
    () => false,
  );

  // Populate the ref inside an effect (runs after render) so we avoid updating
  // refs during render (which ESLint flags). We intentionally run this effect
  // on every render by omitting a dependency array so the ref always points to
  // a function that closes over the latest local variables.
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
        const key = ensureActiveText();
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === key && b.kind === "text"
              ? { ...b, content: b.content + event.content }
              : b,
          ),
        );
      } else if (event.type === "tool_call") {
        // First chunk (delta === "") creates the bubble; subsequent chunks append.
        // Non-streaming: single event with full args in delta, also goes through the create path.
        setBubbles((prev) => {
          const exists = prev.some(
            (b) => b.key === `tool-${event.id}` && b.kind === "tool",
          );
          if (exists) {
            return prev.map((b) =>
              b.key === `tool-${event.id}` && b.kind === "tool"
                ? { ...b, argsRaw: b.argsRaw + event.delta }
                : b,
            );
          }
          sealActiveText();
          const toolBubble: ToolBubble = {
            kind: "tool",
            key: `tool-${event.id}`,
            role: "tool",
            name: event.name,
            argsRaw: event.delta,
            result: null,
            pending: true,
          };
          return [...prev, toolBubble];
        });
      } else if (event.type === "tool_result") {
        setBubbles((prev) =>
          prev.map((b) =>
            b.key === `tool-${event.id}` && b.kind === "tool"
              ? { ...b, result: event.result, pending: false }
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

  // ── Reattach to an ongoing generation — called explicitly by ChatPage
  //    after history has been loaded, so there is no race between
  //    setHistory overwriting bubbles and replay events being processed.

  const reattach = useCallback((conversationId: string, token: string) => {
    // Guard: don't open a second WS if one is already live.
    if (wsLiveRef.current) return;
    if (attachedConvRef.current === conversationId) return;

    attachedConvRef.current = conversationId;

    const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${wsProtocol}://${location.host}/ws/${conversationId}`,
    );
    wsRef.current = ws;
    wsLiveRef.current = ws;

    ws.addEventListener("open", () => {
      reattachingRef.current = true;
      ws.send(JSON.stringify({ token }));
      ws.send(JSON.stringify({ type: "reattach" }));
    });

    ws.addEventListener("message", (ev) => {
      let event: WsServerEvent;
      try {
        event = JSON.parse(ev.data as string) as WsServerEvent;
      } catch {
        return;
      }

      // During reattach we set status to streaming as soon as we see any
      // non-terminal event, so the UI shows the in-progress state.
      if (
        statusRef.current === "idle" &&
        event.type !== "done" &&
        event.type !== "aborted" &&
        event.type !== "error"
      ) {
        reattachingRef.current = false;
        updateStatus("streaming");
      }

      const finished = processEventRef.current
        ? processEventRef.current(event)
        : false;
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

  // ── Open a new WebSocket turn ──────────────────────────────────────────

  const send = useCallback(
    (text: string) => {
      if (!conversationId || !token) return;
      if (statusRef.current === "connecting") return;

      // If already streaming, route through the interrupt channel instead of
      // opening a new WebSocket. This lets messages queue up while deepseek-reasoner
      // is thinking, to be consumed at the next Idle transition.
      if (statusRef.current === "streaming") {
        interrupt(text);
        return;
      }

      // 关闭 reattach 阶段可能留下的静默连接，避免两个 WS 同时追加 token。
      // reattachingRef 标记的是"已建连但 generation 尚未开始"的静默连接；
      // 如果 generation 已经在跑（streaming），send 在函数开头就已被拦截。
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

      setErrorMsg(null);
      activeTextKeyRef.current = null;

      // Optimistically add user bubble
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
      const ws = new WebSocket(
        `${wsProtocol}://${location.host}/ws/${conversationId}`,
      );
      wsRef.current = ws;
      wsLiveRef.current = ws;
      attachedConvRef.current = conversationId;

      ws.addEventListener("open", () => {
        ws.send(JSON.stringify({ token }));
        ws.send(JSON.stringify({ content: text }));
        updateStatus("streaming");
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(ev.data as string) as WsServerEvent;
        } catch {
          return;
        }

        const finished = processEventRef.current
          ? processEventRef.current(event)
          : false;
        if (finished) {
          ws.close(1000);
          wsRef.current = null;
          wsLiveRef.current = null;
        }
      });

      ws.addEventListener("error", () => {
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
        // Only treat as an error if the close was abnormal AND we are still
        // in the streaming state (i.e. done/error has not already handled it).
        if (
          ev.code !== 1000 &&
          ev.code !== 1001 &&
          statusRef.current === "streaming"
        ) {
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
    [conversationId, token, interrupt],
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
  };
}
