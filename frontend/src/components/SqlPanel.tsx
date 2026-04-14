import { useState, useRef } from "react";
import { useAuth } from "../store/auth.shared";
import styles from "./SqlPanel.module.css";

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
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  async function runQuery() {
    if (!sql.trim()) return;
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const res = await fetch("/api/admin/sql", {
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

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      runQuery();
    }
  }

  return (
    <div className={styles.container}>
      <div className={styles.editorRow}>
        <textarea
          ref={textareaRef}
          className={styles.editor}
          value={sql}
          onChange={(e) => setSql(e.target.value)}
          onKeyDown={handleKeyDown}
          spellCheck={false}
          rows={6}
        />
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
                      {result.columns.map((col) => {
                        const val = row[col];
                        const display =
                          val === null || val === undefined
                            ? <span className={styles.null}>NULL</span>
                            : typeof val === "object"
                            ? JSON.stringify(val)
                            : String(val);
                        return <td key={col}>{display}</td>;
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
