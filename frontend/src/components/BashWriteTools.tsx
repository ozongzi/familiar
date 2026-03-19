import { useState, useEffect } from "react";
import type { ToolBubble } from "../api/types";
import { buildToolArgsView } from "./messageBubble.toolParsing";
import {
  ToolHeader,
  ToolBodyWrap,
  ExitBadge,
  CheckBadge,
  AutocheckDetails,
  parseAutocheckResult,
  TerminalIcon,
  FileEditIcon,
} from "./ToolShared";
import sharedStyles from "./ToolShared.module.css";
import { FilePreviewContent } from "./FilePreviewContent";

function unwrapResult(result: unknown): Record<string, unknown> | null {
  if (!result || typeof result !== "object") return null;
  const r = result as Record<string, unknown>;
  if (typeof r.text === "string") {
    try {
      const parsed = JSON.parse(r.text) as Record<string, unknown>;
      return parsed;
    } catch {
      return r;
    }
  }
  return r;
}

export function BashTool({ bubble }: { bubble: ToolBubble }) {
  const [expanded, setExpanded] = useState(false);
  const argsView = buildToolArgsView(bubble);

  const command = argsView.command ?? argsView.raw;

  const result = unwrapResult(bubble.result);
  const exitCode =
    typeof result?.exit_code === "number" ? result.exit_code : null;
  const output = typeof result?.output === "string" ? result.output : null;
  const timedOut = result?.timed_out === true;

  useEffect(() => {
    if (bubble.pending) setExpanded(true);
  }, [bubble.pending]);

  const badge = bubble.pending ? null : timedOut ? (
    <span className={sharedStyles.badgeWarn}>timed out</span>
  ) : exitCode !== null ? (
    <ExitBadge code={exitCode} />
  ) : null;

  const label = bubble.description || "运行命令";

  return (
    <div>
      <ToolHeader
        pending={bubble.pending}
        label={label}
        badges={badge}
        expanded={expanded}
        onToggle={() => setExpanded((v) => !v)}
        icon={<TerminalIcon />}
      />
      {expanded && (
        <ToolBodyWrap>
          {command && (
            <div className={sharedStyles.cmdBar}>
              <span className={sharedStyles.cmdPrompt}>$</span>
              <span className={sharedStyles.cmdText}>{command}</span>
            </div>
          )}
          {output !== null ? (
            <div className={sharedStyles.output}>
              {output.length > 0 ? (
                output
              ) : (
                <span className={sharedStyles.outputEmpty}>(no output)</span>
              )}
            </div>
          ) : (
            bubble.pending && (
              <div className={sharedStyles.output}>
                <span className={sharedStyles.outputEmpty}>等待输出…</span>
              </div>
            )
          )}
        </ToolBodyWrap>
      )}
    </div>
  );
}

// ─── WriteTool ─────────────────────────────────────────────────────────────────

export function WriteTool({ bubble }: { bubble: ToolBubble }) {
  const [expanded, setExpanded] = useState(false);
  const argsView = buildToolArgsView(bubble);

  const path = argsView.path ?? "";
  const oldStr = argsView.oldStr;
  const newContent = argsView.editContent ?? "";
  const isDiff = oldStr !== null;

  const autocheck = parseAutocheckResult(bubble.result);
  const hasIssues =
    autocheck && (autocheck.errors.length > 0 || autocheck.warnings.length > 0);

  useEffect(() => {
    if (bubble.pending) setExpanded(true);
  }, [bubble.pending]);

  const badge = bubble.pending ? null : autocheck ? (
    <CheckBadge
      success={autocheck.success}
      warnings={autocheck.warnings.length}
      errors={autocheck.errors.length}
    />
  ) : null;

  const ext = path.split(".").pop() ?? "";
  const addedLines = newContent.split("\n").length;

  const label = bubble.description || (isDiff ? "编辑文件" : "写入文件");

  return (
    <div>
      <ToolHeader
        pending={bubble.pending}
        label={label}
        badges={badge}
        expanded={expanded}
        onToggle={() => setExpanded((v) => !v)}
        icon={<FileEditIcon />}
      />
      {expanded && (
        <ToolBodyWrap>
          <div className={sharedStyles.diffPath}>
            <span>{path}</span>
            {!isDiff && !bubble.pending && (
              <span className={sharedStyles.diffLines}>
                +{addedLines} lines
              </span>
            )}
          </div>
          {isDiff ? (
            <DiffView oldStr={oldStr!} newStr={newContent} />
          ) : (
            <div className={sharedStyles.codePreview}>
              <FilePreviewContent
                content={newContent}
                lang={ext}
                lineCount={addedLines}
                compact
              />
            </div>
          )}
          {!bubble.pending && hasIssues && autocheck && (
            <AutocheckDetails
              warnings={autocheck.warnings}
              errors={autocheck.errors}
            />
          )}
        </ToolBodyWrap>
      )}
    </div>
  );
}

// ─── DiffView ──────────────────────────────────────────────────────────────────

function computeDiff(
  oldStr: string,
  newStr: string,
): Array<{ kind: "del" | "add" | "ctx"; text: string; ln: number }> {
  const oldLines = oldStr.split("\n");
  const newLines = newStr.split("\n");
  const result: Array<{
    kind: "del" | "add" | "ctx";
    text: string;
    ln: number;
  }> = [];

  // Simple LCS-based diff for display purposes
  // For short diffs (most tool calls), this is fast enough
  const m = oldLines.length;
  const n = newLines.length;
  const LIMIT = 60;

  if (m > LIMIT || n > LIMIT) {
    oldLines.forEach((t, i) =>
      result.push({ kind: "del", text: t, ln: i + 1 }),
    );
    newLines.forEach((t, i) =>
      result.push({ kind: "add", text: t, ln: i + 1 }),
    );
    return result;
  }

  // DP LCS
  const dp: number[][] = Array.from({ length: m + 1 }, () =>
    new Array(n + 1).fill(0),
  );
  for (let i = m - 1; i >= 0; i--)
    for (let j = n - 1; j >= 0; j--)
      dp[i][j] =
        oldLines[i] === newLines[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);

  let i = 0,
    j = 0,
    oldLn = 1,
    newLn = 1;
  while (i < m || j < n) {
    if (i < m && j < n && oldLines[i] === newLines[j]) {
      result.push({ kind: "ctx", text: oldLines[i], ln: oldLn++ });
      i++;
      j++;
      newLn++;
    } else if (j < n && (i >= m || dp[i + 1][j] <= dp[i][j + 1])) {
      result.push({ kind: "add", text: newLines[j], ln: newLn++ });
      j++;
    } else {
      result.push({ kind: "del", text: oldLines[i], ln: oldLn++ });
      i++;
    }
  }

  // Keep only context around changes (±3 lines)
  const changed = new Set(
    result.map((r, idx) => (r.kind !== "ctx" ? idx : -1)).filter((x) => x >= 0),
  );
  const keep = new Set<number>();
  changed.forEach((idx) => {
    for (let k = idx - 3; k <= idx + 3; k++)
      if (k >= 0 && k < result.length) keep.add(k);
  });

  return result
    .map((r, idx) => (keep.has(idx) ? r : null))
    .filter((r): r is NonNullable<typeof r> => r !== null);
}

function DiffView({ oldStr, newStr }: { oldStr: string; newStr: string }) {
  const lines = computeDiff(oldStr, newStr);

  return (
    <div className={sharedStyles.diffArea}>
      {lines.map((line, i) => (
        <span
          key={i}
          className={`${sharedStyles.diffLine} ${
            line.kind === "del"
              ? sharedStyles.diffDel
              : line.kind === "add"
                ? sharedStyles.diffAdd
                : sharedStyles.diffCtx
          }`}
        >
          <span className={sharedStyles.diffLn}>{line.ln}</span>
          {line.kind === "del" ? "−" : line.kind === "add" ? "+" : " "}
          {line.text}
        </span>
      ))}
    </div>
  );
}
