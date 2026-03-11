import styles from "./TerminalView.module.css";

interface Props {
  command?: string; // the command / script that was run
  stdout?: string;
  stderr?: string;
  exitCode?: number | null;
  toolName: string; // "execute" | "run_ts" | "run_py"
}

function toolLabel(toolName: string, command?: string): string {
  if (toolName === "execute" && command) return command;
  if (toolName === "run_ts") return "bun script.ts";
  if (toolName === "run_py") return "uv run script.py";
  return toolName;
}

function ExitBadge({ code }: { code: number | null | undefined }) {
  if (code === undefined || code === null) return null;
  const ok = code === 0;
  return (
    <span className={ok ? styles.exitOk : styles.exitFail}>
      exit {code}
    </span>
  );
}

export function TerminalView({ command, stdout, stderr, exitCode, toolName }: Props) {
  const hasStdout = stdout && stdout.trim().length > 0;
  const hasStderr = stderr && stderr.trim().length > 0;
  const isEmpty = !hasStdout && !hasStderr;
  const label = toolLabel(toolName, command);

  return (
    <div className={styles.container}>
      {/* ── Prompt bar ── */}
      <div className={styles.header}>
        <div className={styles.dots} aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <span className={styles.prompt}>
          <span className={styles.promptSigil}>$</span>
          <span className={styles.promptCmd}>{label}</span>
        </span>
        <ExitBadge code={exitCode} />
      </div>

      {/* ── Output body ── */}
      <div className={styles.body}>
        {isEmpty && (
          <span className={styles.empty}>(no output)</span>
        )}

        {hasStdout && (
          <pre className={styles.stdout}>{stdout!.trimEnd()}</pre>
        )}

        {hasStderr && (
          <>
            {hasStdout && <div className={styles.divider} />}
            <div className={styles.stderrHeader}>stderr</div>
            <pre className={styles.stderr}>{stderr!.trimEnd()}</pre>
          </>
        )}
      </div>
    </div>
  );
}
