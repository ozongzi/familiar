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
import { buildToolArgsView } from "./messageBubble.toolParsing";
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


function toRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function readTextPayload(value: unknown): string | undefined {
  const obj = toRecord(value);
  if (!obj) return undefined;
  if (typeof obj.text === "string") return obj.text;
  return undefined;
}

function getTerminalResultView(result: Record<string, unknown> | null): {
  stdout?: string;
  stderr?: string;
  exitCode?: number | null;
} {
  if (!result) return {};

  const stdout =
    (typeof result.stdout === "string" ? result.stdout : undefined) ??
    readTextPayload(result.output) ??
    (typeof result.output === "string" ? result.output : undefined) ??
    (typeof result.text === "string" ? result.text : undefined);

  const stderr =
    (typeof result.stderr === "string" ? result.stderr : undefined) ??
    readTextPayload(result.error) ??
    (typeof result.error === "string" ? result.error : undefined);

  const exitCodeRaw =
    result.exit_code ?? result.exitCode ?? result.code ?? result.status_code;
  const exitCode =
    typeof exitCodeRaw === "number" || exitCodeRaw === null
      ? (exitCodeRaw as number | null)
      : undefined;

  return { stdout, stderr, exitCode };
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
  const result = bubble.result as Record<string, unknown> | null;

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

  const isDesktopCommanderTerminal =
    bubble.name === "execute_command" ||
    bubble.name === "execute" ||
    bubble.name === "start_process";

  // Terminal tools: bash / execute_command / run_* / start_process
  const isTerminal =
    bubble.name === "bash" ||
    bubble.name === "run_ts" ||
    bubble.name === "run_py" ||
    isDesktopCommanderTerminal;

  // Edit tools: edit / write / write_file / write_pdf
  const isReplaceTool = bubble.name === "edit_block";
  const isWriteTool =
    bubble.name === "write" ||
    bubble.name === "write_file" ||
    bubble.name === "write_pdf";
  const isEditTool = isReplaceTool || isWriteTool;

  // Diff view rules:
  // - replace tools still require explicit success status + complete old/new args.
  // - write tools rely on parsed args (path + content), because many providers
  //   return text-shaped result payloads without `status: "success"`.
  const hasReplaceDiff =
    !bubble.pending &&
    isReplaceTool &&
    result?.status === "success" &&
    argsView.oldStr !== null &&
    argsView.editContent !== null;
  const hasWriteDiff =
    !bubble.pending &&
    isWriteTool &&
    argsView.path !== null &&
    argsView.editContent !== null;
  const isDiff = hasReplaceDiff || hasWriteDiff;

  const isSpawn = bubble.name === "spawn";
  const isInline = isTerminal || isEditTool;

  // Streaming args display (generic view only)
  const argsStr = args ? JSON.stringify(args, null, 2) : argsView.raw;
  const argsStreaming = !args && argsView.raw.length > 0;
  const resultStr =
    !isInline && bubble.result && !fileResult
      ? JSON.stringify(bubble.result, null, 2)
      : null;

  const spawnEvents = bubble.name === "spawn" ? (bubble.spawnEvents ?? []) : [];

  const streamingScript = argsView.script;
  const streamingCommand = argsView.command;
  const streamingEditPath = argsView.path;
  const streamingOldStr = argsView.oldStr;
  const streamingEditContent = argsView.editContent;
  const streamingAskQuestion = argsView.question;
  const askOptions = argsView.options;
  const terminalResult = getTerminalResultView(result);

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
              <span
                className={`${styles.toolName} ${styles.toolNamePending}`}
              >
                {toolLabel}
              </span>
              <span className={styles.toolInkPulse} aria-hidden="true" />
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

  const scriptLang =
    bubble.name === "run_py"
      ? "python"
      : bubble.name === "run_ts"
        ? "typescript"
        : "";

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
            {bubble.pending ? (
              <span className={styles.toolInkPulse} aria-hidden="true" />
            ) : (
              <span className={styles.toolChevron} aria-hidden="true">
                <ChevronIcon expanded={expanded} />
              </span>
            )}
          </button>

          {expanded && spawnEvents.length > 0 && (
            <div className={styles.spawnBody}>
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

  if (isInline) {
    return (
      <div className={nested ? styles.toolRowNested : styles.toolRow}>
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
            <span
              className={`${styles.toolName} ${bubble.pending ? styles.toolNamePending : ""}`}
            >
              {toolLabel}
            </span>
            {bubble.pending ? (
              <span className={styles.toolInkPulse} aria-hidden="true" />
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
                isWriteTool &&
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
                  path={streamingEditPath ?? ""}
                  oldStr={streamingOldStr ?? ""}
                  newStr={streamingEditContent ?? ""}
                />
              )}
              {isEditTool &&
                !bubble.pending &&
                isDiff &&
                isWriteTool && (
                  <DiffView
                    mode="write"
                    path={streamingEditPath ?? ""}
                    newStr={streamingEditContent ?? ""}
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

              {/* run_py / run_ts: syntax-highlighted script preview (streaming or done) */}
              {(bubble.name === "run_py" || bubble.name === "run_ts") &&
                streamingScript !== null && (
                <div className={styles.scriptPreview}>
                  <MarkdownRenderer
                    content={`\`\`\`${scriptLang}\n${streamingScript}${argsStreaming ? "█" : ""}\n\`\`\``}
                  />
                </div>
              )}

              {/* bash / Desktop Commander: sh-highlighted command preview */}
              {(bubble.name === "bash" || isDesktopCommanderTerminal) &&
                streamingCommand !== null && (
                <div className={styles.scriptPreview}>
                  <MarkdownRenderer
                    content={`\`\`\`sh\n${streamingCommand}${argsStreaming ? "█" : ""}\n\`\`\``}
                  />
                </div>
              )}

              {/* bash / Desktop Commander: fallback raw args while streaming */}
              {bubble.pending &&
                (bubble.name === "bash" || isDesktopCommanderTerminal) &&
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
                  command={streamingCommand ?? undefined}
                  stdout={terminalResult.stdout}
                  stderr={terminalResult.stderr}
                  exitCode={terminalResult.exitCode}
                />
              )}
            </>
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
          {bubble.pending ? (
            <span className={styles.toolInkPulse} aria-hidden="true" />
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
