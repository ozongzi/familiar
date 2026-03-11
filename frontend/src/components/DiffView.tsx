import { useMemo } from "react";
import * as Diff from "diff";
import hljs from "highlight.js";
import DOMPurify from "dompurify";
import styles from "./DiffView.module.css";

interface Props {
  path: string;
  oldStr?: string; // str_replace: old_str
  newStr: string; // str_replace: new_str, or write: content
  mode: "str_replace" | "write";
  streaming?: boolean; // 流式期间禁用 unchanged 行折叠
}

interface DiffLine {
  type: "added" | "removed" | "unchanged";
  content: string;
  oldLineNo: number | null;
  newLineNo: number | null;
}

function computeDiffLines(oldStr: string, newStr: string): DiffLine[] {
  const changes = Diff.diffLines(oldStr, newStr, { newlineIsToken: false });
  const lines: DiffLine[] = [];
  let oldNo = 1;
  let newNo = 1;

  for (const change of changes) {
    const rawLines = change.value.split("\n");
    // diffLines includes a trailing empty string if value ends with \n
    const lineTexts =
      rawLines[rawLines.length - 1] === "" ? rawLines.slice(0, -1) : rawLines;

    if (change.added) {
      for (const text of lineTexts) {
        lines.push({
          type: "added",
          content: text,
          oldLineNo: null,
          newLineNo: newNo++,
        });
      }
    } else if (change.removed) {
      for (const text of lineTexts) {
        lines.push({
          type: "removed",
          content: text,
          oldLineNo: oldNo++,
          newLineNo: null,
        });
      }
    } else {
      for (const text of lineTexts) {
        lines.push({
          type: "unchanged",
          content: text,
          oldLineNo: oldNo++,
          newLineNo: newNo++,
        });
      }
    }
  }

  return lines;
}

// For "write" mode with no old content, treat all lines as added.
function writeLines(newStr: string): DiffLine[] {
  return newStr.split("\n").map((content, i) => ({
    type: "added" as const,
    content,
    oldLineNo: null,
    newLineNo: i + 1,
  }));
}

// Highlight a single line of code for the given file path.
// Returns sanitized HTML safe to inject into the DOM.
function highlightLineForPath(line: string, path: string): string {
  if (!line) return ""; // keep empty lines empty

  // Derive a language hint from the file extension if possible.
  const ext = path.includes(".") ? (path.split(".").pop() ?? "") : "";
  const language = ext && hljs.getLanguage(ext) ? ext : "";

  // Use explicit language if available, otherwise auto-detect.
  let rawHighlighted: string;
  try {
    rawHighlighted = language
      ? hljs.highlight(line, { language }).value
      : hljs.highlightAuto(line).value;
  } catch {
    rawHighlighted = line
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // Sanitize the highlighted HTML to avoid XSS.
  const sanitized = DOMPurify.sanitize(rawHighlighted, {
    ALLOWED_TAGS: ["span", "div", "em", "strong", "code"],
    ALLOWED_ATTR: ["class"],
  });

  return sanitized;
}

export function DiffView({
  path,
  oldStr,
  newStr,
  mode,
  streaming = false,
}: Props) {
  const lines = useMemo<DiffLine[]>(() => {
    if (mode === "write" || oldStr === undefined) {
      return writeLines(newStr);
    }
    return computeDiffLines(oldStr, newStr);
  }, [mode, oldStr, newStr]);

  const added = lines.filter((l) => l.type === "added").length;
  const removed = lines.filter((l) => l.type === "removed").length;

  // Collapse long unchanged runs — show at most CONTEXT lines around changes.
  const CONTEXT = 3;
  const visible = useMemo(() => {
    if (mode === "write") return lines; // all lines are added, show all
    if (streaming)
      return lines.map((line, idx) => ({ kind: "line" as const, line, idx })); // 流式期间不折叠，避免 new_str 未完整时 unchanged 行被隐藏
    const changed = new Set<number>();
    lines.forEach((l, i) => {
      if (l.type !== "unchanged") changed.add(i);
    });

    const show = new Set<number>();
    changed.forEach((idx) => {
      for (let d = -CONTEXT; d <= CONTEXT; d++) {
        const j = idx + d;
        if (j >= 0 && j < lines.length) show.add(j);
      }
    });

    // Build runs with optional collapse markers between them.
    type Row =
      | { kind: "line"; line: DiffLine; idx: number }
      | { kind: "collapse"; count: number };

    const rows: Row[] = [];
    let i = 0;
    while (i < lines.length) {
      if (show.has(i)) {
        rows.push({ kind: "line", line: lines[i], idx: i });
        i++;
      } else {
        // Count consecutive hidden lines.
        const start = i;
        while (i < lines.length && !show.has(i)) i++;
        rows.push({ kind: "collapse", count: i - start });
      }
    }
    return rows;
  }, [lines, mode, streaming]);

  const filename = path.split("/").pop() ?? path;

  return (
    <div className={styles.container}>
      {/* ── Header ── */}
      <div className={styles.header}>
        <span className={styles.filepath}>
          <span className={styles.filepathDir}>
            {path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : ""}
          </span>
          <span className={styles.filepathName}>{filename}</span>
        </span>
        <div className={styles.stats}>
          {removed > 0 && (
            <span className={styles.statRemoved}>−{removed}</span>
          )}
          {added > 0 && <span className={styles.statAdded}>+{added}</span>}
          {mode === "write" && <span className={styles.badge}>新建</span>}
        </div>
      </div>

      {/* ── Diff table ── */}
      <div className={styles.body}>
        <table className={styles.table}>
          <tbody>
            {(mode === "write"
              ? lines.map((line, idx) => ({ kind: "line" as const, line, idx }))
              : visible
            ).map((row, ri) => {
              if ((row as { kind: string }).kind === "collapse") {
                const c = row as { kind: "collapse"; count: number };
                return (
                  <tr key={`collapse-${ri}`} className={styles.collapseRow}>
                    <td className={styles.lineNo} />
                    <td className={styles.lineNo} />
                    <td className={styles.collapseCell}>
                      ⋯ {c.count} 行未改动
                    </td>
                  </tr>
                );
              }
              const { line } = row as {
                kind: "line";
                line: DiffLine;
                idx: number;
              };
              return (
                <tr
                  key={`line-${ri}`}
                  className={
                    line.type === "added"
                      ? styles.rowAdded
                      : line.type === "removed"
                        ? styles.rowRemoved
                        : styles.rowUnchanged
                  }
                >
                  <td className={styles.lineNo}>{line.oldLineNo ?? ""}</td>
                  <td className={styles.lineNo}>{line.newLineNo ?? ""}</td>
                  <td className={styles.lineContent}>
                    <span className={styles.marker}>
                      {line.type === "added"
                        ? "+"
                        : line.type === "removed"
                          ? "−"
                          : " "}
                    </span>
                    <code
                      className={`${styles.code} hljs`}
                      dangerouslySetInnerHTML={{
                        __html: highlightLineForPath(line.content, path),
                      }}
                    />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
