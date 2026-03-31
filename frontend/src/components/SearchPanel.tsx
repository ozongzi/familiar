import { useState, useEffect, useRef, useCallback } from "react";
import { api } from "../api/client";
import type { SearchResult } from "../api/types";
import styles from "./SearchPanel.module.css";

interface Props {
  token: string;
  onSelectConversation: (id: string) => void;
  onClose: () => void;
}

export function SearchPanel({ token, onSelectConversation, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Close on Escape
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const search = useCallback(
    (q: string) => {
      if (!q.trim()) {
        setResults([]);
        return;
      }
      setLoading(true);
      api
        .searchMessages(token, q.trim())
        .then((r) => setResults(r.results))
        .catch(() => setResults([]))
        .finally(() => setLoading(false));
    },
    [token],
  );

  const handleChange = (value: string) => {
    setQuery(value);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => search(value), 300);
  };

  const handleSelect = (r: SearchResult) => {
    onSelectConversation(r.conversation_id);
    onClose();
  };

  const highlight = (text: string | null, q: string): string => {
    if (!text || !q.trim()) return text ?? "";
    const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return text.replace(
      new RegExp(`(${escaped})`, "gi"),
      "<mark>$1</mark>",
    );
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        <div className={styles.inputRow}>
          <SearchIcon />
          <input
            ref={inputRef}
            className={styles.input}
            placeholder="搜索历史消息…"
            value={query}
            onChange={(e) => handleChange(e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
          {query && (
            <button className={styles.clearBtn} onClick={() => { setQuery(""); setResults([]); inputRef.current?.focus(); }}>
              ×
            </button>
          )}
        </div>

        <div className={styles.results}>
          {loading && <p className={styles.hint}>搜索中…</p>}
          {!loading && query && results.length === 0 && (
            <p className={styles.hint}>无结果</p>
          )}
          {!loading && !query && (
            <p className={styles.hint}>输入关键词搜索所有对话历史</p>
          )}
          {results.map((r) => (
            <button
              key={r.id}
              className={styles.resultItem}
              onClick={() => handleSelect(r)}
            >
              <div className={styles.resultMeta}>
                <span className={styles.convName}>{r.conversation_name}</span>
                <span className={styles.role}>{r.role === "user" ? "你" : "助手"}</span>
              </div>
              <p
                className={styles.snippet}
                dangerouslySetInnerHTML={{
                  __html: highlight(r.content?.slice(0, 120) ?? "", query),
                }}
              />
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function SearchIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}
