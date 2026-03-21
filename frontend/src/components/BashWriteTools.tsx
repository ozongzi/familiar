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

  const runResult = (() => {
    if (!bubble.result || typeof bubble.result !== "object") return null;
    const r = bubble.result as Record<string, unknown>;
    const run = r.run as Record<string, unknown> | undefined;
    if (!run || typeof run !== "object") return null;
    return {
      output: typeof run.output === "string" ? run.output : null,
      exitCode: typeof run.exit_code === "number" ? run.exit_code : null,
      timedOut: run.timed_out === true,
    };
  })();

  useEffect(() => {
    if (bubble.pending) setExpanded(true);
  }, [bubble.pending]);

  const badge = bubble.pending ? null : (
    <>
      {runResult && runResult.exitCode !== null && (
        <ExitBadge code={runResult.exitCode} />
      )}
      {autocheck && (
        <CheckBadge
          success={autocheck.success}
          warnings={autocheck.warnings.length}
          errors={autocheck.errors.length}
        />
      )}
    </>
  );

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
          {!bubble.pending && runResult && (
            <div className={sharedStyles.output}>
              {runResult.timedOut ? (
                <span className={sharedStyles.outputEmpty}>timed out</span>
              ) : runResult.output && runResult.output.length > 0 ? (
                runResult.output
              ) : (
                <span className={sharedStyles.outputEmpty}>(no output)</span>
              )}
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

// ─── MultiWriteTool ────────────────────────────────────────────────────────────

interface WriteEntry {
  path: string;
  new_content?: string;
  old_string?: string;
  shebang?: string;
}

interface WriteEntryResult {
  error?: string;
  run?: { output?: string; exit_code?: number; timed_out?: boolean };
  diff?: string;
}

export function MultiWriteTool({ bubble }: { bubble: ToolBubble }) {
  const [expanded, setExpanded] = useState(false);
  const argsView = buildToolArgsView(bubble);

  const entries: WriteEntry[] = (() => {
    const parsed = argsView.parsed;
    const raw = typeof parsed?.writes_json === "string" ? parsed.writes_json : null;
    if (!raw) return [];
    try {
      const arr = JSON.parse(raw);
      return Array.isArray(arr) ? arr : [];
    } catch {
      return [];
    }
  })();

  const resultObj = (() => {
    if (!bubble.result || typeof bubble.result !== "object") return null;
    const r = bubble.result as Record<string, unknown>;
    if (typeof r.text === "string") {
      try { return JSON.parse(r.text) as Record<string, unknown>; } catch { return r; }
    }
    return r;
  })();

  const results: WriteEntryResult[] = Array.isArray(resultObj?.results)
    ? (resultObj!.results as WriteEntryResult[])
    : [];

  const autochecks: unknown[] = Array.isArray(resultObj?.autochecks)
    ? (resultObj!.autochecks as unknown[])
    : [];

  const failedPaths: string[] = Array.isArray(resultObj?.failed_paths)
    ? (resultObj!.failed_paths as string[])
    : [];

  const hasErrors = failedPaths.length > 0;
  const allAutochecks = autochecks
    .map((ac) => parseAutocheckResult(ac))
    .filter(Boolean) as ReturnType<typeof parseAutocheckResult>[];
  const hasIssues = allAutochecks.some(
    (ac) => ac && (ac.errors.length > 0 || ac.warnings.length > 0),
  );

  useEffect(() => {
    if (bubble.pending) setExpanded(true);
  }, [bubble.pending]);

  const label = bubble.description || "批量写入文件";

  const badge = bubble.pending ? null : (
    <>
      {!bubble.pending && entries.length > 0 && (
        <span className={sharedStyles.badgeOk} style={{ opacity: 0.7 }}>
          {entries.length} 文件
        </span>
      )}
      {hasErrors && (
        <span className={sharedStyles.badgeErr}>{failedPaths.length} 失败</span>
      )}
      {allAutochecks.map((ac, i) =>
        ac ? (
          <CheckBadge
            key={i}
            success={ac.success}
            warnings={ac.warnings.length}
            errors={ac.errors.length}
          />
        ) : null,
      )}
    </>
  );

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
          {entries.map((entry, i) => {
            const res = results[i] as WriteEntryResult | undefined;
            const isDiff = !!entry.old_string;
            const newContent = entry.new_content ?? "";
            const ext = entry.path.split(".").pop() ?? "";
            const addedLines = newContent.split("\n").length;
            const run = res?.run;
            const runResult = run
              ? {
                  output: typeof run.output === "string" ? run.output : null,
                  exitCode: typeof run.exit_code === "number" ? run.exit_code : null,
                  timedOut: run.timed_out === true,
                }
              : null;
            const isError = !!res?.error;

            return (
              <div key={i} className={sharedStyles.multiWriteEntry}>
                <div className={sharedStyles.diffPath}>
                  <span>{entry.path}</span>
                  {!isDiff && !bubble.pending && (
                    <span className={sharedStyles.diffLines}>+{addedLines} lines</span>
                  )}
                  {isError && (
                    <span className={sharedStyles.badgeErr} style={{ marginLeft: "auto" }}>
                      error
                    </span>
                  )}
                </div>
                {isError ? (
                  <div className={sharedStyles.output}>
                    <span className={sharedStyles.outputEmpty}>{res?.error}</span>
                  </div>
                ) : isDiff ? (
                  <DiffView oldStr={entry.old_string!} newStr={newContent} />
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
                {!bubble.pending && runResult && (
                  <div className={sharedStyles.output}>
                    {runResult.timedOut ? (
                      <span className={sharedStyles.outputEmpty}>timed out</span>
                    ) : runResult.output && runResult.output.length > 0 ? (
                      runResult.output
                    ) : (
                      <span className={sharedStyles.outputEmpty}>(no output)</span>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {!bubble.pending && hasIssues &&
            allAutochecks.map((ac, i) =>
              ac && (ac.errors.length > 0 || ac.warnings.length > 0) ? (
                <AutocheckDetails key={i} warnings={ac.warnings} errors={ac.errors} />
              ) : null,
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
