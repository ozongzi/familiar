import {
  memo,
  useState,
  useCallback,
  useEffect,
  useRef,
  useMemo,
  useLayoutEffect,
} from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { DiffView } from "./DiffView";
import { TerminalView } from "./TerminalView";
import type { ChatBubble, UploadBubble } from "../api/types";
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

// ── Helper: extract a string field from a partial streaming JSON args string ─
function extractArgsField(raw: string, key: string): string | null {
  const keyPattern = new RegExp(`"${key}"\\s*:\\s*"`);
  const keyMatch = raw.match(keyPattern);
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
      } else {
        break;
      }
    } else if (ch === '"') {
      return value;
    } else {
      value += ch;
      i++;
    }
  }
  return value.length > 0 ? value : null;
}
function ToolCallBubble({
  bubble,
  onAnswer,
}: {
  bubble: Extract<ChatBubble, { kind: "tool" }>;
  onAnswer?: (text: string) => void;
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

  const args = useMemo(() => {
    if (!bubble.argsRaw) return null;
    try {
      return JSON.parse(bubble.argsRaw) as Record<string, unknown>;
    } catch {
      return null;
    }
  }, [bubble.argsRaw]);
  const result = bubble.result as Record<string, unknown> | null;

  // Detect present result
  const fileResult =
    bubble.result &&
    typeof bubble.result === "object" &&
    (bubble.result as Record<string, unknown>)["display"] === "file"
      ? (bubble.result as {
          display: "file";
          filename: string;
          path: string;
          size: number;
        })
      : null;

  // Terminal tools: bash, run_ts, run_py
  const isTerminal =
    bubble.name === "bash" ||
    bubble.name === "run_ts" ||
    bubble.name === "run_py";

  // Edit tools: edit / write
  const isReplaceTool = bubble.name === "edit";
  const isEditTool = isReplaceTool || bubble.name === "write";

  // Diff view only shown when edit completed successfully with parsed args
  const isDiff =
    !bubble.pending &&
    isEditTool &&
    result?.status === "success" &&
    ((isReplaceTool &&
      args?.old_str !== undefined &&
      args?.new_str !== undefined) ||
      (bubble.name === "write" &&
        args?.path !== undefined &&
        args?.content !== undefined));

  const isSpawn = bubble.name === "spawn";
  const isInline = isTerminal || isEditTool || isSpawn;

  // Streaming args display (generic view only)
  const argsStr = args ? JSON.stringify(args, null, 2) : bubble.argsRaw || "";
  const argsStreaming = !args && bubble.argsRaw.length > 0;
  const resultStr =
    !isInline && bubble.result && !fileResult
      ? JSON.stringify(bubble.result, null, 2)
      : null;

  const spawnOutput = bubble.name === "spawn" ? (bubble.spawnOutput ?? "") : "";
  const spawnResultText =
    bubble.name === "spawn" && result && typeof result.result === "string"
      ? String(result.result)
      : "";

  // ── Extract script content from streaming argsRaw (run_py / run_ts) ───────
  const streamingScript = useMemo(() => {
    if (bubble.name !== "run_py" && bubble.name !== "run_ts") return null;
    // Show during streaming (pending) or after completion (args parsed).
    if (bubble.pending) {
      if (!bubble.argsRaw) return null;
      return extractArgsField(bubble.argsRaw, "script");
    }
    // After completion, prefer the parsed args value.
    return args?.script ? String(args.script) : null;
  }, [bubble.pending, bubble.name, bubble.argsRaw, args]);

  // ── Extract command content from streaming argsRaw (bash) ─────────────────
  const streamingCommand = useMemo(() => {
    if (bubble.name !== "bash") return null;
    if (!bubble.argsRaw) return null;
    // extractArgsField works even with incomplete/corrupt JSON (streaming or damaged)
    const extracted = extractArgsField(bubble.argsRaw, "command");
    if (extracted) return extracted;
    // Fallback to parsed args
    return args?.command ? String(args.command) : null;
  }, [bubble.name, bubble.argsRaw, args]);

  // ── Extract fields for edit tools (edit / write) ───────────────────────────
  const streamingEditPath = useMemo(() => {
    if (!isEditTool) return null;
    if (!bubble.pending) return args?.path ? String(args.path) : null;
    if (!bubble.argsRaw) return null;
    return extractArgsField(bubble.argsRaw, "path");
  }, [isEditTool, bubble.pending, bubble.argsRaw, args]);

  // old_str (edit) — used for diff preview as soon as it arrives
  const streamingOldStr = useMemo(() => {
    if (!isReplaceTool) return null;
    if (!bubble.pending) return args?.old_str ? String(args.old_str) : null;
    if (!bubble.argsRaw) return null;
    return extractArgsField(bubble.argsRaw, "old_str");
  }, [isReplaceTool, bubble.pending, bubble.argsRaw, args]);

  // new_str / content — arrives after old_str; null means not yet streamed
  const streamingEditContent = useMemo(() => {
    if (!isEditTool) return null;
    if (!bubble.pending) {
      if (isReplaceTool)
        return args?.new_str !== undefined ? String(args.new_str) : null;
      if (bubble.name === "write")
        return args?.content !== undefined ? String(args.content) : null;
      return null;
    }
    if (!bubble.argsRaw) return null;
    const field = isReplaceTool ? "new_str" : "content";
    return extractArgsField(bubble.argsRaw, field);
  }, [isEditTool, isReplaceTool, bubble.pending, bubble.argsRaw, args]);

  // ── Extract question text from streaming argsRaw (ask) ───────────────────
  const streamingAskQuestion = useMemo(() => {
    if (bubble.name !== "ask") return null;
    if (args?.question) return String(args.question);
    if (!bubble.argsRaw) return null;
    return extractArgsField(bubble.argsRaw, "question");
  }, [bubble.name, bubble.argsRaw, args]);

  // ── Extract options with runtime array check (ask) ─────────────────────
  const askOptions = useMemo(() => {
    if (bubble.name !== "ask") return undefined;
    const opts = args?.options;
    return Array.isArray(opts) ? (opts as string[]) : undefined;
  }, [bubble.name, args]);

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
              <span className={styles.toolName}>{toolLabel}</span>
              <span className={styles.toolSpinner} aria-hidden="true" />
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
  if (bubble.name === "present" && fileResult) {
    return <FileCard file={fileResult} pending={bubble.pending} />;
  }

  const scriptLang =
    bubble.name === "run_py"
      ? "python"
      : bubble.name === "run_ts"
        ? "typescript"
        : "";

  if (isInline) {
    return (
      <div className={styles.toolRow}>
        <div className={styles.toolBubbleInline}>
          {/* Header — always clickable to toggle detail */}
          <button
            className={styles.toolHeaderInline}
            onClick={() => setExpanded((v) => !v)}
            aria-expanded={expanded}
          >
            <span className={styles.toolIcon} aria-hidden="true">
              {bubble.pending ? <ToolRunningIcon /> : <ToolDoneIcon />}
            </span>
            <span className={styles.toolName}>{toolLabel}</span>
            {bubble.pending ? (
              <span className={styles.toolSpinner} aria-hidden="true" />
            ) : (
              <span className={styles.toolChevron} aria-hidden="true">
                <ChevronIcon expanded={expanded} />
              </span>
            )}
          </button>

          {expanded && (
            <>
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>工具: {bubble.name}</p>
              </div>
              {/* edit: 流式期间 — old_str 到了就渲染 diff（new_str 未到时显示纯删除行） */}
              {isEditTool &&
                bubble.pending &&
                isReplaceTool &&
                streamingOldStr !== null && (
                  <DiffView
                    mode="str_replace"
                    path={streamingEditPath ?? ""}
                    oldStr={streamingOldStr}
                    newStr={streamingEditContent ?? ""}
                    streaming
                  />
                )}

              {/* write: 流式期间 — content 开始到达就渲染 DiffView（全部为新增行） */}
              {isEditTool &&
                bubble.pending &&
                bubble.name === "write" &&
                streamingEditContent !== null && (
                  <DiffView
                    mode="write"
                    path={streamingEditPath ?? ""}
                    newStr={streamingEditContent}
                    streaming
                  />
                )}

              {/* edit tools: 完成后显示最终 DiffView（成功且 args 解析完整） */}
              {isEditTool && !bubble.pending && isDiff && isReplaceTool && (
                <DiffView
                  mode="str_replace"
                  path={String(args!.path)}
                  oldStr={String(args!.old_str)}
                  newStr={String(args!.new_str)}
                />
              )}
              {isEditTool &&
                !bubble.pending &&
                isDiff &&
                bubble.name === "write" && (
                  <DiffView
                    mode="write"
                    path={String(args!.path)}
                    newStr={String(args!.content)}
                  />
                )}
              {/* edit tools: fallback — 失败（error 字段）或 args 解析不完整时显示原始结果 */}
              {isEditTool && !bubble.pending && !isDiff && result && (
                <div className={styles.toolSection}>
                  <p className={styles.toolSectionLabel}>
                    {(result as Record<string, unknown>)["error"]
                      ? "错误"
                      : "结果"}
                  </p>
                  <pre className={styles.toolCode}>
                    {JSON.stringify(result, null, 2)}
                  </pre>
                </div>
              )}

              {/* spawn: 子 Agent 流式输出 */}
              {isSpawn &&
                (spawnOutput.length > 0 || spawnResultText.length > 0) && (
                  <div className={styles.toolSection}>
                    <p className={styles.toolSectionLabel}>子 Agent 输出</p>
                    <MarkdownRenderer
                      content={`${spawnOutput || spawnResultText}${bubble.pending ? "\n\n█" : ""}`}
                    />
                  </div>
                )}
              {/* run_py / run_ts: syntax-highlighted script preview (streaming or done) */}
              {isTerminal && streamingScript !== null && (
                <div className={styles.scriptPreview}>
                  <MarkdownRenderer
                    content={`\`\`\`${scriptLang}\n${streamingScript}${argsStreaming ? "█" : ""}\n\`\`\``}
                  />
                </div>
              )}

              {/* bash: sh-highlighted command preview (streaming and done) */}
              {bubble.name === "bash" && streamingCommand !== null && (
                <div className={styles.scriptPreview}>
                  <MarkdownRenderer
                    content={`\`\`\`sh\n${streamingCommand}${argsStreaming ? "█" : ""}\n\`\`\``}
                  />
                </div>
              )}

              {/* bash: fallback raw args while streaming if command field not yet present */}
              {bubble.pending &&
                bubble.name === "bash" &&
                streamingCommand === null &&
                argsStr && (
                  <div className={styles.toolSection}>
                    <pre className={styles.toolCode}>
                      {argsStr}
                      {argsStreaming && (
                        <span className={styles.cursor} aria-hidden="true" />
                      )}
                    </pre>
                  </div>
                )}

              {/* Terminal result — shown below the code preview once complete */}
              {!bubble.pending && isTerminal && (
                <TerminalView
                  toolName={bubble.name}
                  command={args?.command ? String(args.command) : undefined}
                  stdout={result?.stdout ? String(result.stdout) : undefined}
                  stderr={result?.stderr ? String(result.stderr) : undefined}
                  exitCode={
                    result?.exit_code !== undefined
                      ? (result.exit_code as number | null)
                      : undefined
                  }
                />
              )}
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={styles.toolRow}>
      <div className={styles.toolBubble}>
        <button
          className={styles.toolHeader}
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          <span className={styles.toolIcon} aria-hidden="true">
            {bubble.pending ? <ToolRunningIcon /> : <ToolDoneIcon />}
          </span>
          <span className={styles.toolName}>{toolLabel}</span>
          {bubble.pending ? (
            <span className={styles.toolSpinner} aria-hidden="true" />
          ) : (
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
            {argsStr && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>参数</p>
                <pre className={styles.toolCode}>
                  {argsStr}
                  {argsStreaming && (
                    <span className={styles.cursor} aria-hidden="true" />
                  )}
                </pre>
              </div>
            )}
            {resultStr !== null && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>结果</p>
                <pre className={styles.toolCode}>{resultStr}</pre>
              </div>
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

// ─── File preview content (with highlight.js) ─────────────────────────────────

function FilePreviewContent({
  content,
  lang,
  lineCount,
}: {
  content: string;
  lang: string;
  lineCount: number;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Import hljs dynamically to keep the main bundle lean — it's already loaded
  // by MarkdownRenderer so this will hit the module cache.
  useEffect(() => {
    import("highlight.js").then((hljs) => {
      const el = containerRef.current?.querySelector("code");
      if (!el) return;
      if (lang && hljs.default.getLanguage(lang)) {
        el.innerHTML = hljs.default.highlight(content, {
          language: lang,
        }).value;
      } else {
        el.innerHTML = hljs.default.highlightAuto(content).value;
      }
    });
  }, [content, lang]);

  return (
    <div ref={containerRef} className={styles.filePreviewCode}>
      <div className={styles.filePreviewCodeHeader}>
        {lang && <span className={styles.filePreviewLang}>{lang}</span>}
        <span className={styles.filePreviewLines}>{lineCount} 行</span>
      </div>
      <pre className={styles.filePreviewPre}>
        <code className={`hljs ${lang ? `language-${lang}` : ""}`}>
          {content}
        </code>
      </pre>
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
