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
import { BashTool, WriteTool } from "./BashWriteTools";
import styles from "./MessageBubble.module.css";

// ─── Helpers ──────────────────────────────────────────────────────────────────

interface Props {
  bubble: ChatBubble;
  onAnswer?: (text: string) => void;
}

export const MessageBubble = memo(function MessageBubble({
  bubble,
  onAnswer,
}: Props) {
  if (bubble.kind === "tool") {
    return <ToolCallBubble bubble={bubble} onAnswer={onAnswer} />;
  }
  if (bubble.kind === "upload") {
    return <UploadChatBubble bubble={bubble} />;
  }
  return <TextChatBubble bubble={bubble} />;
});

// ─── Widget bubble ────────────────────────────────────────────────────────────

const WIDGET_CSS_VARS = `
  :host {
    --bg-base: #faf9f5;
    --bg-surface: #ffffff;
    --bg-elevated: #f0ede6;
    --bg-hover: #ebe7de;
    --border: #ddd8ce;
    --border-subtle: #e8e4db;
    --text-primary: #1a1915;
    --text-secondary: #6b6651;
    --text-muted: #73726c;
    --accent: #c96442;
    --accent-dim: #f5ede8;
    --radius-sm: 8px;
    --radius-md: 12px;
    --radius-lg: 20px;
    --font-sans: system-ui, -apple-system, sans-serif;
    --font-mono: "Cascadia Code", "JetBrains Mono", monospace;
    --color-background-primary: #ffffff;
    --color-background-secondary: #f0ede6;
    --color-text-primary: #1a1915;
    --color-text-secondary: #6b6651;
    --color-border-tertiary: rgba(29,25,21,0.15);
    --color-border-secondary: rgba(29,25,21,0.3);
    --border-radius-md: 8px;
    --border-radius-lg: 12px;
    display: block;
  }
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
  :host > div {
    font-family: var(--font-sans);
    font-size: 15px;
    color: var(--text-primary);
    background: transparent;
    overflow-x: hidden;
    padding: 12px;
  }
`;

function extractBodyContent(code: string): string {
  const bodyMatch = code.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  if (bodyMatch) return bodyMatch[1];
  const htmlMatch = code.match(/<html[^>]*>([\s\S]*)<\/html>/i);
  if (htmlMatch) return htmlMatch[1];
  return code;
}

function extractHeadAssets(code: string): {
  styles: string;
  scriptSrcs: string[];
} {
  const headMatch = code.match(/<head[^>]*>([\s\S]*?)<\/head>/i);
  if (!headMatch) return { styles: "", scriptSrcs: [] };
  const head = headMatch[1];
  const styleMatches = head.match(/<style[^>]*>[\s\S]*?<\/style>/gi) ?? [];
  const styles = styleMatches
    .map((s) => s.replace(/<\/?style[^>]*>/gi, ""))
    .join("\n");
  const scriptSrcMatches = [
    ...head.matchAll(/<script[^>]+src=["']([^"']+)["'][^>]*>/gi),
  ];
  const scriptSrcs = scriptSrcMatches.map((m) => m[1]);
  return { styles, scriptSrcs };
}

function injectShadow(host: HTMLElement, code: string) {
  let shadow = host.shadowRoot;
  if (!shadow) {
    shadow = host.attachShadow({ mode: "open" });
  }

  while (shadow.firstChild) shadow.removeChild(shadow.firstChild);

  const isFullDocument = /<html/i.test(code);
  const bodyContent = isFullDocument ? extractBodyContent(code) : code;
  const { styles: headStyles, scriptSrcs: externalScripts } = isFullDocument
    ? extractHeadAssets(code)
    : { styles: "", scriptSrcs: [] };

  const varStyle = document.createElement("style");
  varStyle.textContent = WIDGET_CSS_VARS;
  shadow.appendChild(varStyle);

  if (headStyles) {
    const headStyle = document.createElement("style");
    headStyle.textContent = headStyles;
    shadow.appendChild(headStyle);
  }

  // 注入 head 里的外部 script（如 Chart.js），等它们加载完再执行 inline script
  const loadPromises = externalScripts.map(
    (src) =>
      new Promise<void>((resolve) => {
        const s = document.createElement("script");
        s.src = src;
        s.onload = () => resolve();
        s.onerror = () => resolve();
        shadow.appendChild(s);
      }),
  );

  const wrapper = document.createElement("div");
  wrapper.innerHTML = bodyContent;
  shadow.appendChild(wrapper);

  // 等外部 script 加载完再执行 inline scripts
  Promise.all(loadPromises).then(() => {
    wrapper.querySelectorAll("script").forEach((oldScript) => {
      const newScript = document.createElement("script");
      if (oldScript.src) {
        newScript.src = oldScript.src;
      } else {
        newScript.textContent = oldScript.textContent;
      }
      oldScript.replaceWith(newScript);
    });
  });
}

function WidgetChatBubble({ bubble }: { bubble: ToolBubble }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(300);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !bubble.widgetCode) return;

    injectShadow(host, bubble.widgetCode);

    const shadow = host.shadowRoot;
    if (!shadow) return;

    function measure() {
      const children = Array.from(shadow!.children).filter(
        (c) => c.tagName !== "STYLE" && c.tagName !== "SCRIPT",
      );
      const wrapper = children[children.length - 1] as HTMLElement | undefined;
      if (!wrapper) return;
      // scrollHeight 包含 overflow 内容，比 getBoundingClientRect 更可靠
      const h = wrapper.scrollHeight;
      if (h > 20) setHeight(Math.min(h + 24, 1400));
    }

    const ro = new ResizeObserver(measure);
    const children = Array.from(shadow.children).filter(
      (c) => c.tagName !== "STYLE" && c.tagName !== "SCRIPT",
    );
    const wrapper = children[children.length - 1];
    if (wrapper) ro.observe(wrapper);

    const timers = [
      setTimeout(measure, 200),
      setTimeout(measure, 800),
      setTimeout(measure, 1800),
    ];

    return () => {
      ro.disconnect();
      timers.forEach(clearTimeout);
    };
  }, [bubble.widgetCode]);

  return (
    <div className={styles.row} style={{ justifyContent: "flex-start" }}>
      <div className={styles.widgetBubble}>
        <div
          ref={hostRef}
          style={{ width: "100%", height, display: "block" }}
        />
      </div>
    </div>
  );
}
// ─── Text bubble (user / assistant) ──────────────────────────────────────────

function TextChatBubble({
  bubble,
}: {
  bubble: Extract<ChatBubble, { kind: "text" }>;
}) {
  const isUser = bubble.role === "user";
  const hasReasoning = bubble.reasoning && bubble.reasoning.length > 0;

  // Auto-expand while reasoning is streaming in; collapse once content arrives.
  const [reasoningOpen, setReasoningOpen] = useState(false);
  const prevReasoningLen = useRef(0);

  useLayoutEffect(() => {
    if (!hasReasoning) return;
    const len = bubble.reasoning.length;
    let t: number | undefined;
    if (len > prevReasoningLen.current) {
      // New reasoning tokens arrived — ensure open.
      // Defer the state update to avoid calling setState synchronously inside the effect.
      t = window.setTimeout(() => setReasoningOpen(true), 0);
    }
    prevReasoningLen.current = len;
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
  }, [bubble.reasoning, hasReasoning]);

  // Collapse reasoning once the main content starts streaming in.
  useLayoutEffect(() => {
    let t: number | undefined;
    if (bubble.content.length > 0 && bubble.streaming) {
      // Defer the collapse to avoid calling setState synchronously inside the effect.
      t = window.setTimeout(() => setReasoningOpen(false), 0);
    }
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
  }, [bubble.content, bubble.streaming]);

  return (
    <div
      className={`${styles.row} ${isUser ? styles.rowUser : styles.rowAssistant}`}
    >
      <div
        className={`${styles.bubble} ${isUser ? styles.bubbleUser : styles.bubbleAssistant}`}
      >
        {isUser ? (
          <p className={styles.userText}>{bubble.content}</p>
        ) : (
          <>
            {hasReasoning && (
              <div className={styles.reasoningBlock}>
                <button
                  className={styles.reasoningToggle}
                  onClick={() => setReasoningOpen((o) => !o)}
                  aria-expanded={reasoningOpen}
                >
                  <span className={styles.reasoningIcon}>💭</span>
                  <span className={styles.reasoningLabel}>
                    {bubble.streaming && bubble.content.length === 0
                      ? "思考中…"
                      : "思考过程"}
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
    </div>
  );
}

// ─── Upload bubble (user-side file card) ──────────────────────────────────────

function UploadChatBubble({ bubble }: { bubble: UploadBubble }) {
  const token = localStorage.getItem("familiar_token") ?? "";

  const handleDownload = useCallback(() => {
    const params = new URLSearchParams({ path: bubble.path, token });
    const a = document.createElement("a");
    a.href = `/api/files?${params}`;
    a.download = bubble.filename;
    a.click();
  }, [bubble.path, bubble.filename, token]);

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

function ToolCallBubble({
  bubble,
  onAnswer,
  nested = false,
}: {
  bubble: Extract<ChatBubble, { kind: "tool" }>;
  onAnswer?: (text: string) => void;
  nested?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  // Auto-expand while pending (args streaming in), auto-collapse when done.
  useEffect(() => {
    let t: number | undefined;
    if (bubble.pending && bubble.argsRaw.length > 0 && !expanded) {
      t = window.setTimeout(() => setExpanded(true), 0);
    } else if (!bubble.pending && expanded) {
      t = window.setTimeout(() => setExpanded(false), 0);
    }
    return () => {
      if (t !== undefined) clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bubble.pending, bubble.argsRaw.length > 0]);

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
  const toolLabel = useMemo(() => {
    const placeholders = [
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
    return (
      bubble.description ||
      placeholders[Math.floor(Math.random() * placeholders.length)]
    );
  }, [bubble.description]);

  // ── visualize → widget 渲染 ──────────────────────────────────────────────
  if (bubble.widgetCode) {
    return <WidgetChatBubble bubble={bubble} />;
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
    return <FileCard file={fileResult} pending={bubble.pending} />;
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
  const [submitted, setSubmitted] = useState(false);

  const handleAnswer = useCallback(
    (text: string) => {
      if (!onAnswer) return;
      setSubmitted(true);
      onAnswer(text);
    },
    [onAnswer],
  );

  if (answered !== undefined) {
    return (
      <div className={styles.askUserCard}>
        <p className={styles.askUserQuestion}>{question}</p>
        <div className={styles.askUserAnswered}>
          <span className={styles.askUserAnsweredLabel}>已回答：</span>
          <span className={styles.askUserAnsweredText}>{answered}</span>
        </div>
      </div>
    );
  }

  if (submitted) {
    return (
      <div className={styles.askUserCard}>
        <p className={styles.askUserQuestion}>{question}</p>
        <div className={styles.askUserAnswered}>
          <span className={styles.askUserAnsweredLabel}>已发送，等待回应…</span>
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

function FileCard({ file, pending }: { file: FileInfo; pending: boolean }) {
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [expanded, setExpanded] = useState(false);

  const token = localStorage.getItem("familiar_token") ?? "";

  const loadPreview = useCallback(async () => {
    if (preview.status !== "idle") return;
    setPreview({ status: "loading" });
    try {
      const params = new URLSearchParams({ path: file.path, token });
      const res = await fetch(`/api/files/preview?${params}`);
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
  }, [file.path, token, preview.status]);

  function toggleExpand() {
    if (!expanded && preview.status === "idle") {
      loadPreview();
    }
    setExpanded((v) => !v);
  }

  const handleDownload = useCallback(() => {
    const params = new URLSearchParams({ path: file.path, token });
    const a = document.createElement("a");
    a.href = `/api/files?${params}`;
    a.download = file.filename;
    a.click();
  }, [file.path, file.filename, token]);

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
