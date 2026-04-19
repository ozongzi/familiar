import React, {
  memo,
  useState,
  useCallback,
  useEffect,
  useRef,
  useMemo,
  useLayoutEffect,
} from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";
import type { ChatBubble, ToolBubble, UploadBubble } from "../api/types";
import { buildToolArgsView } from "./messageBubble.toolParsing";
import { FilePreviewContent } from "./FilePreviewContent";
import { BashTool, WriteTool, MultiWriteTool } from "./BashWriteTools";
import { PlanTool } from "./PlanTool";
import styles from "./MessageBubble.module.css";
import { getServerBase } from "../utils/tauri";

const BASE = () => getServerBase();

// ─── Helpers ──────────────────────────────────────────────────────────────────

/** Strip the server-injected timestamp prefix "[YYYY-MM-DD HH:MM UTC] " from user messages. */
function stripTimestamp(text: string): string {
  return text.replace(/^\[\d{4}-\d{2}-\d{2} \d{2}:\d{2} UTC\] /, "");
}

interface Props {
  bubble: ChatBubble;
  onAnswer?: (text: string) => void;
  conversationId?: string | null;
  onBranch?: (msgId: number, bubbleKey: string, newText: string) => void;
  onSwitchSibling?: (targetMsgId: number) => void;
  /// When set, this bubble is the last assistant text bubble of a
  /// completed reply; the copy button on this bubble copies the full
  /// reply (concatenation of all text fragments in the reply) rather
  /// than this fragment alone. Unset for user bubbles, mid-reply
  /// fragments, and mid-stream assistant bubbles.
  fullReplyContent?: string;
}

export const MessageBubble = memo(function MessageBubble({
  bubble,
  onAnswer,
  conversationId,
  onBranch,
  onSwitchSibling,
  fullReplyContent,
}: Props) {
  if (bubble.kind === "tool") {
    return (
      <ToolCallBubble
        bubble={bubble}
        onAnswer={onAnswer}
        conversationId={conversationId}
      />
    );
  }
  if (bubble.kind === "upload") {
    return <UploadChatBubble bubble={bubble} />;
  }
  return (
    <TextChatBubble
      bubble={bubble}
      onBranch={onBranch}
      onSwitchSibling={onSwitchSibling}
      fullReplyContent={fullReplyContent}
    />
  );
});

// ─── Widget bubble ────────────────────────────────────────────────────────────

/**
 * Build a full HTML document string for the iframe srcdoc.
 * If the widget code is already a full document, inject our base styles into <head>.
 * Otherwise wrap it in a minimal HTML shell.
 */
function buildWidgetSrcdoc(code: string): string {
  const baseStyle = `
    <style>
      *, *::before, *::after { box-sizing: border-box; }
      :root {
        --bg-base: #faf9f5;
        --bg-surface: #ffffff;
        --bg-elevated: #f0ede6;
        --bg-hover: #ebe7de;
        --bg-active: #e4dfd4;
        --border: #ddd8ce;
        --border-subtle: #e8e4db;
        --text-primary: #1a1915;
        --text-secondary: #6b6651;
        --text-muted: #73726c;
        --text-link: #b85c3a;
        --accent: #c96442;
        --accent-dim: #f5ede8;
        --accent-glow: rgba(201, 100, 66, 0.12);
        --accent-hover: #b85539;
        --danger: #c0392b;
        --danger-dim: rgba(192, 57, 43, 0.08);
        --success: #2d7a4f;
        --radius-sm: 8px;
        --radius-md: 12px;
        --radius-lg: 20px;
        --radius-full: 999px;
        --font-sans: "LXGW WenKai", system-ui, -apple-system, sans-serif;
        --font-mono: "Fira Code", ui-monospace, monospace;
        --font-serif: "LXGW WenKai", Georgia, serif;
        /* visualizer compat aliases */
        --color-background-primary: #ffffff;
        --color-background-secondary: #f0ede6;
        --color-background-tertiary: #faf9f5;
        --color-background-info: #e8f0fb;
        --color-background-danger: rgba(192, 57, 43, 0.08);
        --color-background-success: rgba(45, 122, 79, 0.08);
        --color-background-warning: #fdf3e0;
        --color-text-primary: #1a1915;
        --color-text-secondary: #6b6651;
        --color-text-tertiary: #73726c;
        --color-text-info: #185fa5;
        --color-text-danger: #c0392b;
        --color-text-success: #2d7a4f;
        --color-text-warning: #854f0b;
        --color-border-tertiary: rgba(26, 25, 21, 0.15);
        --color-border-secondary: rgba(26, 25, 21, 0.3);
        --color-border-primary: rgba(26, 25, 21, 0.4);
        --border-radius-md: 8px;
        --border-radius-lg: 12px;
        --border-radius-xl: 16px;
      }
      @import url("https://cdn.jsdelivr.net/npm/lxgw-wenkai-webfont@1.7.0/style.css");
      @import url("https://cdn.jsdelivr.net/npm/@fontsource/fira-code@5/index.css");
      html, body {
        margin: 0;
        padding: 12px 16px;
        font-family: var(--font-sans);
        font-size: 15px;
        color: var(--text-primary);
        background: transparent;
        overflow-x: hidden;
      }
    </style>
  `;

  // Already a full HTML document — inject base style into <head>
  if (/<html/i.test(code)) {
    if (/<head/i.test(code)) {
      return code.replace(/(<head[^>]*>)/i, `$1\n${baseStyle}`);
    }
    return code.replace(/(<html[^>]*>)/i, `$1<head>${baseStyle}</head>`);
  }

  // Fragment — wrap in a minimal document
  return `<!DOCTYPE html>
<html><head>${baseStyle}</head><body>${code}</body></html>`;
}

function WidgetChatBubble({ bubble }: { bubble: ToolBubble }) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);
  const [visible, setVisible] = useState(false);
  const [msgIdx, setMsgIdx] = useState(0);

  const loadingMsgs = bubble.widgetLoadingMessages?.length
    ? bubble.widgetLoadingMessages
    : ["生成中…"];

  const srcdoc = useMemo(
    () => (!bubble.pending && bubble.widgetCode ? buildWidgetSrcdoc(bubble.widgetCode) : ""),
    [bubble.pending, bubble.widgetCode],
  );

  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe || !srcdoc) return;
    iframe.srcdoc = srcdoc;
    setTimeout(() => setVisible(true), 50);
  }, [srcdoc]);

  useEffect(() => {
    if (!bubble.pending) return;
    const t = setInterval(() => setMsgIdx((i) => (i + 1) % loadingMsgs.length), 2200);
    return () => clearInterval(t);
  }, [bubble.pending, loadingMsgs.length]);

  // Height tracking: postMessage from widget + RAF poll fallback
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    const onMessage = (e: MessageEvent) => {
      if (e.source !== iframe.contentWindow) return;
      if (
        e.data?.type === "familiar-widget-height" &&
        typeof e.data.height === "number"
      ) {
        setHeight(Math.min(Math.max(e.data.height, 60), 2000));
      }
    };
    window.addEventListener("message", onMessage);

    let raf: number;
    let prevH = 0;
    const poll = () => {
      try {
        const doc = iframe.contentDocument;
        if (doc?.body) {
          const h = doc.body.scrollHeight;
          if (h > 20 && h !== prevH) {
            prevH = h;
            setHeight(Math.min(Math.max(h, 60), 2000));
          }
        }
      } catch {
        // cross-origin — ignore
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);

    return () => {
      window.removeEventListener("message", onMessage);
      cancelAnimationFrame(raf);
    };
  }, []);

  if (!bubble.pending && !srcdoc) return null;

  return (
    <div className={styles.row} style={{ justifyContent: "flex-start" }}>
      <div className={styles.widgetBubble}>
        {bubble.pending && (
          <div className={styles.widgetLoading}>
            <span className={styles.widgetLoadingDot} />
            <span key={msgIdx} className={styles.widgetLoadingText}>{loadingMsgs[msgIdx]}</span>
          </div>
        )}
        <iframe
          ref={iframeRef}
          sandbox="allow-scripts allow-same-origin"
          allowTransparency={true}
          className={styles.widgetIframe}
          style={{
            height,
            opacity: visible ? 1 : 0,
            transition: "opacity 0.25s ease",
          }}
          title="widget"
        />
      </div>
    </div>
  );
}
// ─── Text bubble (user / assistant) ──────────────────────────────────────────

function TextChatBubble({
  bubble,
  onBranch,
  onSwitchSibling,
  fullReplyContent,
}: {
  bubble: Extract<ChatBubble, { kind: "text" }>;
  onBranch?: (msgId: number, bubbleKey: string, newText: string) => void;
  onSwitchSibling?: (targetMsgId: number) => void;
  fullReplyContent?: string;
}) {
  const isUser = bubble.role === "user";
  const hasReasoning = bubble.reasoning && bubble.reasoning.length > 0;
  const [hovered, setHovered] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editText, setEditText] = useState("");
  const editRef = useRef<HTMLTextAreaElement>(null);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const canEdit = isUser && !bubble.streaming && bubble.msgId != null && onBranch != null;

  // Copy target differs by role:
  //   - User bubbles are atomic: copy this bubble's content.
  //   - Assistant bubbles belong to a multi-fragment reply interleaved with
  //     tool bubbles; ChatPage passes `fullReplyContent` only to the LAST
  //     text fragment of a completed reply, and we copy that concatenation.
  //     Mid-reply fragments and mid-stream bubbles receive no content, so
  //     no copy button surfaces there.
  const [copied, setCopied] = useState(false);
  const copyText = isUser
    ? (!bubble.streaming && bubble.content.length > 0 ? stripTimestamp(bubble.content) : null)
    : (fullReplyContent ?? null);
  const canCopy = !editing && copyText != null && copyText.length > 0;
  const copyMessage = useCallback(() => {
    if (copyText == null) return;
    navigator.clipboard.writeText(copyText).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [copyText]);

  // Branch switcher: show `‹ idx/N ›` when this message has siblings.
  const siblings = bubble.siblings ?? [];
  const siblingIdx = bubble.msgId != null ? siblings.indexOf(bubble.msgId) : -1;
  const canSwitch =
    !bubble.streaming &&
    onSwitchSibling != null &&
    siblings.length > 1 &&
    siblingIdx >= 0;
  const switchTo = (offset: -1 | 1) => {
    if (!canSwitch) return;
    const next = siblings[(siblingIdx + offset + siblings.length) % siblings.length];
    onSwitchSibling!(next);
  };

  const handleTouchStart = () => {
    if (!canEdit) return;
    longPressTimer.current = setTimeout(() => { startEdit(); }, 600);
  };
  const handleTouchEnd = () => {
    if (longPressTimer.current) { clearTimeout(longPressTimer.current); longPressTimer.current = null; }
  };

  const startEdit = () => {
    setEditText(stripTimestamp(bubble.content));
    setEditing(true);
    setTimeout(() => { editRef.current?.focus(); editRef.current?.select(); }, 0);
  };
  const cancelEdit = () => setEditing(false);
  const confirmEdit = () => {
    const text = editText.trim();
    if (!text || !bubble.msgId) { cancelEdit(); return; }
    setEditing(false);
    onBranch!(bubble.msgId, bubble.key, text);
  };

  // Auto-expand while reasoning is streaming in; collapse once content arrives.
  const [reasoningOpen, setReasoningOpen] = useState(false);
  const prevReasoningLen = useRef(0);

  useLayoutEffect(() => {
    if (!hasReasoning) return;
    const len = bubble.reasoning.length;
    let t: number | undefined;
    if (len > prevReasoningLen.current) {
      t = window.setTimeout(() => setReasoningOpen(true), 0);
    }
    prevReasoningLen.current = len;
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
  }, [bubble.reasoning, hasReasoning]);

  useLayoutEffect(() => {
    let t: number | undefined;
    if (bubble.content.length > 0 && bubble.streaming) {
      t = window.setTimeout(() => setReasoningOpen(false), 0);
    }
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
  }, [bubble.content, bubble.streaming]);

  return (
    <div
      className={`${styles.row} ${isUser ? styles.rowUser : styles.rowAssistant}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div
        className={`${styles.bubble} ${isUser ? styles.bubbleUser : styles.bubbleAssistant}`}
        onTouchStart={handleTouchStart}
        onTouchEnd={handleTouchEnd}
        onTouchMove={handleTouchEnd}
      >
        {isUser ? (
          <>
            {bubble.images && bubble.images.length > 0 && (
              <div className={styles.bubbleImages}>
                {bubble.images.map((src, i) => (
                  <img key={i} src={src} className={styles.bubbleImage} alt="" />
                ))}
              </div>
            )}
            {editing ? (
              <div className={styles.editArea}>
                <textarea
                  ref={editRef}
                  className={styles.editTextarea}
                  value={editText}
                  onChange={(e) => setEditText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); confirmEdit(); }
                    if (e.key === "Escape") cancelEdit();
                  }}
                  rows={3}
                />
                <div className={styles.editActions}>
                  <span className={styles.editHint}>Enter 发送 · Shift+Enter 换行 · Esc 取消</span>
                  <button className={styles.editCancelBtn} onClick={cancelEdit}>取消</button>
                  <button className={styles.editConfirmBtn} onClick={confirmEdit}>发送</button>
                </div>
              </div>
            ) : (
              <>
                {bubble.content && <p className={styles.userText}>{stripTimestamp(bubble.content)}</p>}
              </>
            )}
          </>
        ) : (
          <>
            {hasReasoning && (
              <div className={styles.reasoningBlock}>
                <button
                  className={styles.reasoningToggle}
                  onClick={() => setReasoningOpen((o) => !o)}
                  aria-expanded={reasoningOpen}
                >
                  <span className={styles.reasoningLabel}>
                    {bubble.streaming && bubble.content.length === 0
                      ? "思绪…"
                      : "思绪"}
                  </span>
                  <span className={styles.reasoningChevron}>
                    {reasoningOpen ? "▲" : "▼"}
                  </span>
                </button>
                {reasoningOpen && (
                  <div className={styles.reasoningContent}>
                    <MarkdownRenderer content={bubble.reasoning} />
                    {bubble.streaming && bubble.content.length === 0 && (
                      <span className={styles.cursor} aria-hidden="true" />
                    )}
                  </div>
                )}
              </div>
            )}
            <MarkdownRenderer content={bubble.content} />
            {bubble.streaming &&
              bubble.content.length === 0 &&
              !hasReasoning && (
                <span className={styles.typingDots} aria-label="正在输入">
                  <span />
                  <span />
                  <span />
                </span>
              )}
            {bubble.streaming && bubble.content.length > 0 && (
              <span className={styles.cursor} aria-hidden="true" />
            )}
          </>
        )}
      </div>
      {/* In row-reverse, elements after the bubble appear to its visual left */}
      {canEdit && !editing && (
        <button
          className={`${styles.editBtn} ${hovered ? styles.editBtnVisible : ""}`}
          onClick={startEdit}
          title="编辑消息"
        >
          <EditIcon />
        </button>
      )}
      {canSwitch && !editing && (
        <div
          className={styles.branchSwitcher}
          aria-label={`分支 ${siblingIdx + 1} / ${siblings.length}`}
        >
          <button
            type="button"
            className={styles.branchArrow}
            onClick={() => switchTo(-1)}
            aria-label="上一个分支"
          >
            ‹
          </button>
          <span className={styles.branchCount}>
            {siblingIdx + 1}/{siblings.length}
          </span>
          <button
            type="button"
            className={styles.branchArrow}
            onClick={() => switchTo(1)}
            aria-label="下一个分支"
          >
            ›
          </button>
        </div>
      )}
    </div>
  );
}

// ─── Upload bubble (user-side file card) ──────────────────────────────────────

function UploadChatBubble({ bubble }: { bubble: UploadBubble }) {
  const token = localStorage.getItem("familiar_token") ?? "";

  const handleDownload = useCallback(() => {
    const params = new URLSearchParams({ path: bubble.path, token });
    if (bubble.conversationId) params.append("conversation_id", bubble.conversationId);
    const a = document.createElement("a");
    a.href = `/api/files?${params}`;
    a.download = bubble.filename;
    a.click();
  }, [bubble.path, bubble.filename, bubble.conversationId, token]);

  return (
    <div className={`${styles.row} ${styles.rowUser}`}>
      <div className={styles.uploadBubble}>
        <div className={styles.uploadBubbleInner}>
          <span className={styles.uploadBubbleIcon} aria-hidden="true">
            <FileIcon />
          </span>
          <div className={styles.uploadBubbleMeta}>
            <span className={styles.uploadBubbleName}>{bubble.filename}</span>
            <span className={styles.uploadBubbleSize}>
              {formatBytes(bubble.size)}
            </span>
          </div>
          <button
            className={styles.uploadBubbleDownload}
            onClick={handleDownload}
            aria-label={`下载 ${bubble.filename}`}
            title="下载"
          >
            <DownloadIcon />
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Tool call bubble ─────────────────────────────────────────────────────────

// ─── Object fields view (for structured JSON display) ──────────────────────────

function ObjectFieldsView({
  data,
  label,
  streaming = false,
}: {
  data: unknown;
  label: string;
  streaming?: boolean;
}) {
  if (!data) return null;

  // If it's a primitive, just show it.
  if (typeof data !== "object" || data === null) {
    return (
      <div className={styles.toolSection}>
        <p className={styles.toolSectionLabel}>{label}</p>
        <pre className={styles.toolCode}>{String(data)}</pre>
      </div>
    );
  }

  const obj = data as Record<string, unknown>;
  const keys = Object.keys(obj);
  if (keys.length === 0) return null;

  return (
    <>
      {keys.map((key) => {
        const val = obj[key];
        let content: React.ReactNode;

        if (typeof val === "string") {
          // If string contains newlines (actual or escaped), render with whitespace preservation.
          // We also check for common large-text field names.
          const isLongText =
            val.includes("\n") ||
            val.includes("\\n") ||
            key === "content" ||
            key === "command" ||
            key === "script" ||
            key === "text" ||
            key === "goal" ||
            key === "prompt";

          if (isLongText) {
            // Unescape \n if they are literal characters in the string
            const unescaped = val.replace(/\\n/g, "\n");
            content = (
              <pre
                className={styles.toolCode}
                style={{ whiteSpace: "pre-wrap" }}
              >
                {unescaped}
                {streaming && key === keys[keys.length - 1] && (
                  <span className={styles.cursor} aria-hidden="true" />
                )}
              </pre>
            );
          } else {
            content = <span className={styles.toolCode}>{val}</span>;
          }
        } else if (typeof val === "object" && val !== null) {
          content = (
            <pre className={styles.toolCode}>
              {JSON.stringify(val, null, 2)}
            </pre>
          );
        } else {
          content = <span className={styles.toolCode}>{String(val)}</span>;
        }

        return (
          <div key={key} className={styles.toolSection}>
            <p className={styles.toolSectionLabel}>{key}</p>
            <div className={styles.toolFieldContent}>{content}</div>
          </div>
        );
      })}
    </>
  );
}

const TOOL_PLACEHOLDERS = [
  "鼓捣鼓捣中",
  "捯饬捯饬中",
  "倒腾倒腾中",
  "琢磨琢磨中",
  "摆弄摆弄中",
  "折腾折腾中",
  "捣鼓捣鼓中",
  "拾掇拾掇中",
  "张罗张罗中",
  "腾挪腾挪中",
];

function randomPlaceholder() {
  return TOOL_PLACEHOLDERS[
    Math.floor(Math.random() * TOOL_PLACEHOLDERS.length)
  ];
}

function ToolCallBubble({
  bubble,
  onAnswer,
  nested = false,
  conversationId,
}: {
  bubble: Extract<ChatBubble, { kind: "tool" }>;
  onAnswer?: (text: string) => void;
  nested?: boolean;
  conversationId?: string | null;
}) {
  const [expanded, setExpanded] = useState(false);

  // Auto-expand while pending (args streaming in), auto-collapse when done
  // (unless there are images to show — keep expanded so result is visible).
  useEffect(() => {
    let t: number | undefined;
    if (bubble.pending && bubble.argsRaw.length > 0 && !expanded) {
      t = window.setTimeout(() => setExpanded(true), 0);
    } else if (!bubble.pending && expanded && !(bubble.images?.length)) {
      t = window.setTimeout(() => setExpanded(false), 0);
    }
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bubble.pending, bubble.argsRaw.length > 0, bubble.images?.length]);

  // ── All declarations and hooks must come before any early returns ──────────

  const argsView = useMemo(() => buildToolArgsView(bubble), [bubble]);
  const args = argsView.parsed;

  // Detect present result
  // The backend may return either a direct object like { display: "file", filename, path, size }
  // or a wrapper like { text: "{\"display\":\"file\",...}", type: "text" } (see present_file).
  let fileResult: {
    display: "file";
    filename: string;
    path: string;
    size: number;
    description?: string;
  } | null = null;
  if (bubble.result && typeof bubble.result === "object") {
    const maybe = bubble.result as Record<string, unknown>;
    // direct object case
    if (maybe["display"] === "file") {
      fileResult = maybe as {
        display: "file";
        filename: string;
        path: string;
        size: number;
        description?: string;
      };
    } else if (typeof maybe["text"] === "string") {
      // wrapped string case: try to parse the text field as JSON
      try {
        const parsed = JSON.parse(maybe["text"] as string) as Record<
          string,
          unknown
        >;
        if (parsed && parsed["display"] === "file") {
          fileResult = parsed as {
            display: "file";
            filename: string;
            path: string;
            size: number;
            description?: string;
          };
        }
      } catch {
        // ignore parse errors and treat as non-file result
      }
    }
  }

  const isSpawn = bubble.name === "spawn";

  // Streaming args fallback
  const argsStreaming = !args && argsView.raw.length > 0;

  const spawnEvents = bubble.name === "spawn" ? (bubble.spawnEvents ?? []) : [];
  const spawnGoal =
    isSpawn && args && typeof args.goal === "string" ? args.goal : null;

  const streamingAskQuestion = argsView.question;
  const askOptions = argsView.options;

  // Header label: prefer the model-written description (arrives early in the
  // stream because description is always the first parameter).  Fall back to
  // a per-tool heuristic derived from the args when description is absent.
  const fallbackLabel = useRef(randomPlaceholder());
  const toolLabel = bubble.description || fallbackLabel.current;

  // ── visualize → widget 渲染 ──────────────────────────────────────────────
  if (bubble.widgetCode) {
    return <WidgetChatBubble bubble={bubble} />;
  }

  // ── diagram → Mermaid 渲染 ─────────────────────────────────────
  if (bubble.name === "diagram") {
    return <DiagramBubble bubble={bubble} />;
  }

  // ── bash / write → 专用渲染 ──────────────────────────────────────────────
  const BASH_TOOLS = new Set(["bash", "execute", "execute_command"]);
  const WRITE_TOOLS = new Set([
    "write",
    "write_file",
    "str_replace",
    "edit_block",
  ]);

  if (BASH_TOOLS.has(bubble.name)) {
    return (
      <div className={nested ? styles.toolRowNested : styles.toolRow}>
        <div className={styles.toolBubble}>
          <BashTool bubble={bubble} />
        </div>
      </div>
    );
  }

  if (bubble.name === "multiwrite") {
    return (
      <div className={nested ? styles.toolRowNested : styles.toolRow}>
        <div className={styles.toolBubble}>
          <MultiWriteTool bubble={bubble} />
        </div>
      </div>
    );
  }

  if (bubble.name === "todo_list") {
    return (
      <div className={nested ? styles.toolRowNested : styles.toolRow}>
        <div className={styles.toolBubble}>
          <PlanTool bubble={bubble} />
        </div>
      </div>
    );
  }

  if (WRITE_TOOLS.has(bubble.name)) {
    return (
      <div className={nested ? styles.toolRowNested : styles.toolRow}>
        <div className={styles.toolBubble}>
          <WriteTool bubble={bubble} />
        </div>
      </div>
    );
  }

  // ── Early return: ask → question card ────────────────────────────────────
  if (bubble.name === "ask") {
    const answeredText =
      !bubble.pending && bubble.result
        ? ((bubble.result as Record<string, unknown>)["answer"] as
            | string
            | undefined)
        : undefined;

    // While args are still streaming in (can't parse JSON yet), show a
    // generic loading header instead of a card with "…" as the question.
    if (bubble.pending && args === null) {
      return (
        <div className={styles.toolRow}>
          <div className={styles.toolBubbleInline}>
            <div className={styles.toolHeaderInline}>
              <span className={styles.toolIcon} aria-hidden="true">
                <ToolRunningIcon />
              </span>
              <span className={`${styles.toolName} ${styles.toolNamePending}`}>
                {toolLabel}
              </span>
            </div>
          </div>
        </div>
      );
    }

    return (
      <div className={styles.toolRow}>
        <AskUserCard
          question={streamingAskQuestion ?? "…"}
          options={askOptions}
          onAnswer={onAnswer}
          answered={answeredText}
        />
      </div>
    );
  }

  // ── Early return: present → file card ─────────────────────────────────────
  if (
    (bubble.name === "present" || bubble.name === "present_file") &&
    fileResult
  ) {
    return (
      <FileCard
        file={fileResult}
        pending={bubble.pending}
        conversationId={conversationId}
      />
    );
  }

  if (isSpawn) {
    return (
      <div className={styles.toolRow}>
        <div
          className={`${styles.spawnWrap} ${!bubble.pending ? styles.spawnWrapDone : ""}`}
        >
          <button
            className={styles.spawnHeader}
            onClick={() => setExpanded((v) => !v)}
            aria-expanded={expanded}
          >
            <span
              className={`${styles.spawnBadge} ${!bubble.pending ? styles.spawnBadgeDone : ""}`}
            >
              {bubble.pending ? (
                <span className={styles.spawnBadgeDot} aria-hidden="true" />
              ) : (
                <ToolDoneIcon />
              )}
              子任务
            </span>
            <span
              className={`${styles.spawnTitle} ${bubble.pending ? styles.toolNamePending : ""}`}
            >
              {toolLabel}
            </span>
            {!bubble.pending && (
              <span className={styles.toolChevron} aria-hidden="true">
                <ChevronIcon expanded={expanded} />
              </span>
            )}
          </button>

          {expanded && (
            <div className={styles.spawnBody}>
              {spawnGoal && (
                <div
                  className={styles.spawnTextBlock}
                  style={{ marginBottom: 6 }}
                >
                  <strong style={{ opacity: 0.8 }}>任务目标：</strong>
                  <MarkdownRenderer content={spawnGoal} />
                </div>
              )}

              {/* Final summary/result for spawn */}
              {!bubble.pending && !!bubble.result && (
                <div
                  className={styles.spawnTextBlock}
                  style={{
                    marginBottom: spawnEvents.length > 0 ? 12 : 0,
                    paddingBottom: spawnEvents.length > 0 ? 12 : 0,
                    borderBottom:
                      spawnEvents.length > 0
                        ? "1px dashed var(--border-subtle)"
                        : "none",
                  }}
                >
                  <strong style={{ opacity: 0.8 }}>任务总结：</strong>
                  <ObjectFieldsView data={bubble.result} label="结果" />
                </div>
              )}

              {spawnEvents.map((ev, i) =>
                ev.kind === "tool" ? (
                  <ToolCallBubble
                    key={ev.bubble.key}
                    bubble={ev.bubble}
                    nested
                  />
                ) : (
                  <div key={ev.key} className={styles.spawnTextBlock}>
                    <MarkdownRenderer
                      content={
                        ev.content +
                        (bubble.pending && i === spawnEvents.length - 1
                          ? "█"
                          : "")
                      }
                    />
                  </div>
                ),
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={nested ? styles.toolRowNested : styles.toolRow}>
      <div className={styles.toolBubble}>
        <button
          className={styles.toolHeader}
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          <span className={styles.toolIcon} aria-hidden="true">
            {bubble.pending ? <ToolRunningIcon /> : <ToolDoneIcon />}
          </span>
          <span
            className={`${styles.toolName} ${bubble.pending ? styles.toolNamePending : ""}`}
          >
            {toolLabel}
          </span>
          {!bubble.pending && (
            <span className={styles.toolChevron} aria-hidden="true">
              <ChevronIcon expanded={expanded} />
            </span>
          )}
        </button>

        {(bubble.images?.length ?? 0) > 0 && (
          <div className={styles.toolImages}>
            {bubble.images!.map((src, i) => (
              <img key={i} src={src} className={styles.toolImage} alt="" />
            ))}
          </div>
        )}

        {expanded && (
          <div className={styles.toolBody}>
            <div className={styles.toolSection}>
              <p className={styles.toolSectionLabel}>工具: {bubble.name}</p>
            </div>

            {/* If streaming and not yet parsed, show raw string */}
            {argsStreaming && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>参数</p>
                <pre className={styles.toolCode}>
                  {argsView.raw}
                  <span className={styles.cursor} aria-hidden="true" />
                </pre>
              </div>
            )}

            {/* Parsed args display */}
            {!!args && (
              <ObjectFieldsView
                data={args}
                label="参数"
                streaming={bubble.pending}
              />
            )}

            {/* Streaming progress lines */}
            {bubble.pending && (bubble.progressLines?.length ?? 0) > 0 && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>进度</p>
                <pre className={styles.toolCode}>
                  {bubble.progressLines!.join("\n")}
                </pre>
              </div>
            )}

            {/* Result display */}
            {!!bubble.result && !fileResult && (
              <ObjectFieldsView data={bubble.result} label="结果" />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Diagram bubble (Mermaid) ────────────────────────────────────────────────────────────

function DiagramBubble({ bubble }: { bubble: ToolBubble }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [svg, setSvg] = useState<string>("");
  const [error, setError] = useState<string>("");

  useEffect(() => {
    if (!bubble.diagramCode || bubble.pending) return;
    let cancelled = false;
    (async () => {
      try {
        // Dynamically import mermaid to avoid bloating the initial bundle
        const mermaid = (await import("mermaid")).default;
        mermaid.initialize({
          startOnLoad: false,
          theme: "base",
          themeVariables: {
            primaryColor: "#f0ede6",
            primaryTextColor: "#1a1915",
            primaryBorderColor: "transparent",
            lineColor: "#b8a99a",
            secondaryColor: "#f5ede8",
            tertiaryColor: "#faf9f5",
            edgeLabelBackground: "transparent",
            background: "#faf9f5",
            mainBkg: "#f0ede6",
            nodeBorder: "transparent",
            clusterBkg: "#f0ede6",
            titleColor: "#1a1915",
            fontFamily: "LXGW WenKai, system-ui, sans-serif",
          },
        });
        const id = `mermaid-${bubble.key.replace(/[^a-z0-9]/gi, "-")}`;
        const { svg: rendered } = await mermaid.render(id, bubble.diagramCode!);
        if (!cancelled) setSvg(rendered);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => { cancelled = true; };
  }, [bubble.diagramCode, bubble.pending, bubble.key]);

  return (
    <div className={styles.row} style={{ justifyContent: "flex-start" }}>
      <div className={styles.widgetBubble} style={{ padding: "16px 20px" }}>
        {bubble.pending && (
          <div className={styles.widgetLoading}>
            <span className={styles.widgetLoadingDot} />
            <span className={styles.widgetLoadingText}>生成图表…</span>
          </div>
        )}
        {!bubble.pending && error && (
          <pre style={{ fontSize: "0.8em", color: "var(--danger)", margin: 0 }}>{error}</pre>
        )}
        {!bubble.pending && svg && (
          <div
            ref={containerRef}
            style={{ animation: "fadeInUp 0.25s ease both" }}
            dangerouslySetInnerHTML={{ __html: svg }}
          />
        )}
      </div>
    </div>
  );
}

// ─── Ask-user card ────────────────────────────────────────────────────────────

function AskUserCard({
  question,
  options,
  onAnswer,
  answered,
}: {
  question: string;
  options?: string[];
  onAnswer?: (text: string) => void;
  answered?: string;
}) {
  const [custom, setCustom] = useState("");
  const [submittedText, setSubmittedText] = useState<string | null>(null);

  const handleAnswer = useCallback(
    (text: string) => {
      if (!onAnswer) return;
      setSubmittedText(text);
      onAnswer(text);
    },
    [onAnswer],
  );

  const displayAnswered = answered ?? submittedText ?? undefined;

  if (displayAnswered !== undefined) {
    return (
      <div className={styles.askUserCard}>
        <p className={styles.askUserQuestion}>{question}</p>
        <div className={styles.askUserAnswered}>
          <span className={styles.askUserAnsweredLabel}>已回答：</span>
          <span className={styles.askUserAnsweredText}>{displayAnswered}</span>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.askUserCard}>
      <p className={styles.askUserQuestion}>{question}</p>
      {options && options.length > 0 && (
        <div className={styles.askUserOptions}>
          {options.map((opt, i) => (
            <button
              key={i}
              className={styles.askUserOption}
              onClick={() => handleAnswer(opt)}
            >
              {opt}
            </button>
          ))}
        </div>
      )}
      <form
        className={styles.askUserForm}
        onSubmit={(e) => {
          e.preventDefault();
          const val = custom.trim();
          if (val) {
            handleAnswer(val);
          }
        }}
      >
        <input
          className={styles.askUserInput}
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          placeholder="自定义回答…"
          autoFocus
        />
        <button
          type="submit"
          className={styles.askUserSubmit}
          disabled={!custom.trim()}
        >
          发送
        </button>
      </form>
    </div>
  );
}

// ─── File card (Claude-style) ─────────────────────────────────────────────────

interface FileInfo {
  display: "file";
  filename: string;
  path: string;
  size: number;
  description?: string;
}

type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | {
      status: "ready";
      content: string;
      lang: string;
      lineCount: number;
      truncated: boolean;
    }
  | { status: "error"; message: string }
  | { status: "binary" };

function FileCard({
  file,
  pending,
  conversationId,
}: {
  file: FileInfo;
  pending: boolean;
  conversationId?: string | null;
}) {
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [expanded, setExpanded] = useState(false);

  const token = localStorage.getItem("familiar_token") ?? "";
  const fileUrl = useMemo(() => {
    const params = new URLSearchParams({ path: file.path, token });
    if (conversationId) params.append("conversation_id", conversationId);
    return `/api/files?${params.toString()}`;
  }, [file.path, token, conversationId]);

  const isImageFile = useMemo(() => {
    const target = `${file.filename} ${file.path}`;
    return /\.(png|jpe?g|gif|webp|bmp|svg|tiff?|avif|heic|heif)$/i.test(target);
  }, [file.filename, file.path]);

  const loadPreview = useCallback(async () => {
    if (isImageFile || preview.status !== "idle") return;
    setPreview({ status: "loading" });
    try {
      const params = new URLSearchParams({ path: file.path, token });
      if (conversationId) params.append("conversation_id", conversationId);

      const res = await fetch(`${BASE()}/api/files/preview?${params}`);
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: "无法预览" }));
        if (res.status === 400) {
          setPreview({ status: "binary" });
        } else {
          setPreview({ status: "error", message: err.error ?? "加载失败" });
        }
        return;
      }
      const data = await res.json();
      setPreview({
        status: "ready",
        content: data.content,
        lang: data.lang,
        lineCount: data.line_count,
        truncated: data.truncated,
      });
    } catch {
      setPreview({ status: "error", message: "网络错误" });
    }
  }, [file.path, token, preview.status, isImageFile, conversationId]);

  function toggleExpand() {
    if (!expanded && preview.status === "idle" && !isImageFile) {
      loadPreview();
    }
    setExpanded((v) => !v);
  }

  const handleDownload = useCallback(() => {
    const a = document.createElement("a");
    a.href = fileUrl;
    a.download = file.filename;
    a.click();
  }, [fileUrl, file.filename]);

  return (
    <div className={styles.toolRow}>
      <div
        className={`${styles.fileCard} ${pending ? styles.fileCardPending : ""}`}
      >
        {/* ── Card header ── */}
        <div className={styles.fileCardHeader}>
          <div className={styles.fileCardLeft}>
            <span className={styles.fileCardIcon} aria-hidden="true">
              <FileIcon />
            </span>
            <div className={styles.fileCardMeta}>
              <span className={styles.fileCardName}>{file.filename}</span>
              {file.description && (
                <span className={styles.fileCardDesc}>{file.description}</span>
              )}
              {!pending && (
                <span className={styles.fileCardSize}>
                  {formatBytes(file.size)}
                </span>
              )}
              {pending && (
                <span className={styles.fileCardPendingLabel}>准备中…</span>
              )}
            </div>
          </div>

          <div className={styles.fileCardActions}>
            {!pending && (
              <>
                <button
                  className={styles.fileCardBtn}
                  onClick={toggleExpand}
                  aria-label={expanded ? "收起预览" : "展开预览"}
                  title={expanded ? "收起" : "预览"}
                >
                  <EyeIcon />
                  <span>{expanded ? "收起" : "预览"}</span>
                </button>
                <button
                  className={`${styles.fileCardBtn} ${styles.fileCardBtnPrimary}`}
                  onClick={handleDownload}
                  aria-label={`下载 ${file.filename}`}
                  title="下载"
                >
                  <DownloadIcon />
                  <span>下载</span>
                </button>
              </>
            )}
          </div>
        </div>

        {/* ── Preview area ── */}
        {expanded && (
          <div className={styles.filePreview}>
            {isImageFile ? (
              <div className={styles.filePreviewImageWrap}>
                <img
                  src={fileUrl}
                  alt={file.filename}
                  className={styles.filePreviewImage}
                />
              </div>
            ) : (
              <>
                {preview.status === "loading" && (
                  <div className={styles.filePreviewLoading}>加载中…</div>
                )}
                {preview.status === "binary" && (
                  <div className={styles.filePreviewBinary}>
                    <span aria-hidden="true">📦</span>
                    <span>二进制文件，请下载后查看</span>
                  </div>
                )}
                {preview.status === "error" && (
                  <div className={styles.filePreviewError}>
                    ⚠️ {preview.message}
                  </div>
                )}
                {preview.status === "ready" && (
                  <>
                    <FilePreviewContent
                      content={preview.content}
                      lang={preview.lang}
                      lineCount={preview.lineCount}
                    />
                    {preview.truncated && (
                      <div className={styles.filePreviewTruncated}>
                        文件过大，仅显示前 100 KB
                      </div>
                    )}
                  </>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// ─── Icons ────────────────────────────────────────────────────────────────────

function EditIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" />
      <polyline points="13 2 13 9 20 9" />
    </svg>
  );
}

function EyeIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

// ─── Tool call icons ──────────────────────────────────────────────────────────

function ToolRunningIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
    </svg>
  );
}

function ToolDoneIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function ChevronIcon({ expanded }: { expanded: boolean }) {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{
        transform: expanded ? "rotate(180deg)" : "rotate(0deg)",
        transition: "transform 0.15s ease",
      }}
    >
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}
