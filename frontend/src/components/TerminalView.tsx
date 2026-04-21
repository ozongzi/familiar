import { useMemo, useEffect, useRef } from "react";
import AnsiToHtml from "ansi-to-html";
import styles from "./TerminalView.module.css";

// One Light palette — readable on the warm light backgrounds familiar uses.
const converter = new AnsiToHtml({
  fg: "#1a1915",
  bg: "#f0ede6",
  colors: {
    0: "#383a42", 1: "#e45649", 2: "#50a14f", 3: "#c18401",
    4: "#4078f2", 5: "#a626a4", 6: "#0184bc", 7: "#a0a1a7",
    8: "#4f525e", 9: "#cb4e42", 10: "#43a047", 11: "#c18401",
    12: "#6188d8", 13: "#9c4f99", 14: "#0f86ab", 15: "#383a42",
  },
  escapeXML: true,
});

// Simulate terminal line-overwrite behaviour so the caller doesn't have to.
// Strips raw \r noise — LLM already sees clean output via server-side TerminalBuffer;
// this is purely for the live streaming display in the UI.
function applyCarriageReturns(text: string): string {
  const lines: string[] = [""];
  for (const ch of text) {
    if (ch === "\r") lines[lines.length - 1] = "";
    else if (ch === "\n") lines.push("");
    else lines[lines.length - 1] += ch;
  }
  return lines.join("\n");
}

function toHtml(text: string): string {
  return converter.toHtml(applyCarriageReturns(text));
}

interface Props {
  toolName: string;
  command?: string;
  stdout?: string;
  stderr?: string;
  exitCode?: number | null;
  /** Raw streaming chunks from tool_progress events. When set, renders in streaming mode. */
  progressLines?: string[];
  /** Drop the traffic-lights header — use when embedded inside a ToolBodyWrap. */
  headless?: boolean;
}

function toolLabel(toolName: string, command?: string): string {
  if (["execute", "execute_command", "start_process", "bash"].includes(toolName) && command) {
    return command;
  }
  if (toolName === "run_ts") return "bun script.ts";
  if (toolName === "run_py") return "uv run script.py";
  return toolName;
}

function ExitBadge({ code }: { code: number | null | undefined }) {
  if (code == null) return null;
  return (
    <span className={code === 0 ? styles.exitOk : styles.exitFail}>
      exit {code}
    </span>
  );
}

export function TerminalView({
  toolName,
  command,
  stdout,
  stderr,
  exitCode,
  progressLines,
  headless = false,
}: Props) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const streaming = progressLines != null;

  // Auto-scroll to bottom while streaming.
  useEffect(() => {
    if (streaming && bodyRef.current) {
      bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
    }
  }, [streaming, progressLines]);

  const renderedProgress = useMemo(() => {
    if (!progressLines?.length) return "";
    return toHtml(progressLines.join(""));
  }, [progressLines]);

  const renderedStdout = useMemo(() => (stdout ? toHtml(stdout.trimEnd()) : ""), [stdout]);
  const renderedStderr = useMemo(() => (stderr ? toHtml(stderr.trimEnd()) : ""), [stderr]);

  const hasStdout = !!stdout?.trim();
  const hasStderr = !!stderr?.trim();
  const hasProgress = !!renderedProgress;
  const isEmpty = streaming ? !hasProgress : !hasStdout && !hasStderr;
  const label = toolLabel(toolName, command);

  return (
    <div className={headless ? styles.containerHeadless : styles.container}>
      {!headless && (
        <div className={styles.header}>
          <div className={styles.dots} aria-hidden="true">
            <span /><span /><span />
          </div>
          <span className={styles.prompt}>
            <span className={styles.promptSigil}>$</span>
            <span className={styles.promptCmd}>{label}</span>
          </span>
          <ExitBadge code={exitCode} />
        </div>
      )}

      <div ref={bodyRef} className={styles.body}>
        {isEmpty && <span className={styles.empty}>(no output)</span>}

        {streaming ? (
          hasProgress && (
            <pre
              className={styles.stdout}
              dangerouslySetInnerHTML={{ __html: renderedProgress }}
            />
          )
        ) : (
          <>
            {hasStdout && (
              <pre
                className={styles.stdout}
                dangerouslySetInnerHTML={{ __html: renderedStdout }}
              />
            )}
            {hasStderr && (
              <>
                {hasStdout && <div className={styles.divider} />}
                <div className={styles.stderrHeader}>stderr</div>
                <pre
                  className={styles.stderr}
                  dangerouslySetInnerHTML={{ __html: renderedStderr }}
                />
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
