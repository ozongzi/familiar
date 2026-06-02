import { useState } from "react";
import JsonView from "@uiw/react-json-view";
import { useAuth } from "../store/auth.shared";
import { CodeEditor } from "./CodeEditor";
import styles from "./SqlPanel.module.css";
import { getServerBase } from "../utils/tauri";

interface SqlResult {
  columns: string[];
  rows: Record<string, unknown>[];
}

export function SqlPanel() {
  const { token } = useAuth();
  const [sql, setSql] = useState("SELECT * FROM users LIMIT 20;");
  const [result, setResult] = useState<SqlResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [inspect, setInspect] = useState<{ col: string; value: unknown } | null>(null);

  async function runQuery() {
    if (!sql.trim()) return;
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const res = await fetch(`${getServerBase()}/api/admin/sql`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ sql }),
      });
      const data = await res.json();
      if (!res.ok) {
        setError(data.error ?? JSON.stringify(data));
      } else {
        setResult(data);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.container}>
      <div className={styles.editorRow}>
        <div style={{ flex: 1 }}>
          <CodeEditor
            value={sql}
            onChange={setSql}
            language="sql"
            height={160}
            onSubmit={runQuery}
          />
        </div>
        <button className={styles.runBtn} onClick={runQuery} disabled={loading}>
          {loading ? "执行中…" : "执行"}
          {!loading && <span className={styles.hint}>⌘↵</span>}
        </button>
      </div>

      {error && <div className={styles.error}>{error}</div>}

      {result && (
        <div className={styles.resultWrap}>
          <div className={styles.meta}>{result.rows.length} 行</div>
          {result.rows.length === 0 ? (
            <div className={styles.empty}>无结果</div>
          ) : (
            <div className={styles.tableWrap}>
              <table className={styles.table}>
                <thead>
                  <tr>
                    {result.columns.map((col) => (
                      <th key={col}>{col}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {result.rows.map((row, i) => (
                    <tr key={i}>
                      {result.columns.map((col) => (
                        <Cell
                          key={col}
                          value={row[col]}
                          onInspect={() => setInspect({ col, value: row[col] })}
                        />
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {inspect && (
        <InspectModal
          col={inspect.col}
          value={inspect.value}
          onClose={() => setInspect(null)}
        />
      )}
    </div>
  );
}

// ─── Cell ────────────────────────────────────────────────────────────────────

function Cell({ value, onInspect }: { value: unknown; onInspect: () => void }) {
  const [copied, setCopied] = useState(false);

  if (value === null || value === undefined) {
    return <td><span className={styles.null}>NULL</span></td>;
  }

  const isObject = typeof value === "object";
  const text = isObject ? JSON.stringify(value) : String(value);
  const display = isObject ? previewJson(value) : text;

  async function copy(e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(
        isObject ? JSON.stringify(value, null, 2) : text,
      );
      setCopied(true);
      setTimeout(() => setCopied(false), 900);
    } catch {
      // ignore
    }
  }

  return (
    <td
      className={styles.cell}
      onClick={isObject ? onInspect : undefined}
      style={isObject ? { cursor: "pointer" } : undefined}
    >
      <span className={styles.cellText} title={isObject ? "点击查看" : text}>
        {display}
      </span>
      <button
        className={styles.copyBtn}
        onClick={copy}
        title={copied ? "已复制" : "复制"}
      >
        {copied ? "✓" : "⧉"}
      </button>
    </td>
  );
}

function previewJson(v: unknown): string {
  if (Array.isArray(v)) {
    return `[…] (${v.length})`;
  }
  if (v && typeof v === "object") {
    const keys = Object.keys(v as object);
    return `{…} (${keys.length} keys)`;
  }
  return String(v);
}

// ─── Inspect modal ───────────────────────────────────────────────────────────

function InspectModal({
  col,
  value,
  onClose,
}: {
  col: string;
  value: unknown;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const isObject = value !== null && typeof value === "object";
  const text = isObject
    ? JSON.stringify(value, null, 2)
    : String(value ?? "");

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 900);
    } catch {
      // ignore
    }
  }

  return (
    <div className={styles.modalOverlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.modalHeader}>
          <span className={styles.modalTitle}>{col}</span>
          <div className={styles.modalActions}>
            <button className={styles.modalBtn} onClick={copy}>
              {copied ? "已复制" : "复制"}
            </button>
            <button className={styles.modalBtn} onClick={onClose}>关闭</button>
          </div>
        </div>
        <div className={styles.modalBody}>
          {isObject ? (
            <JsonView
              value={value as object}
              collapsed={2}
              displayDataTypes={false}
              style={{ background: "transparent", fontSize: 13 }}
            />
          ) : (
            <pre className={styles.modalPre}>{text}</pre>
          )}
        </div>
      </div>
    </div>
  );
}
