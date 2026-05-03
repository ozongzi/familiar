import { useEffect, useState } from "react";
import { api } from "../api/client";
import { isTauri, getServerBase } from "../utils/tauri";
import styles from "./ShareModal.module.css";

interface Props {
  token: string;
  conversationId: string;
  conversationName: string;
  onClose: () => void;
}

export function ShareModal({ token, conversationId, onClose }: Props) {
  const [shareToken, setShareToken] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .getShare(token, conversationId)
      .then((info) => {
        if (!cancelled) {
          setShareToken(info.token);
          setLoading(false);
        }
      })
      .catch((e: Error) => {
        if (!cancelled) {
          setError(e.message);
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [token, conversationId]);

  // Close on Escape
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // In Tauri, window.location.origin is the internal scheme (tauri://localhost
  // / http://tauri.localhost) — useless as a public link. Fall back to the
  // configured server base so Android/desktop produce real https:// URLs.
  const linkBase = isTauri() ? getServerBase() : window.location.origin;
  const fullLink = shareToken ? `${linkBase}/share/${shareToken}` : "";

  const handleCreate = async () => {
    setError(null);
    setBusy(true);
    try {
      const info = await api.createShare(token, conversationId);
      setShareToken(info.token);
    } catch (e) {
      setError(e instanceof Error ? e.message : "创建失败");
    } finally {
      setBusy(false);
    }
  };

  const handleRevoke = async () => {
    setError(null);
    setBusy(true);
    try {
      await api.deleteShare(token, conversationId);
      setShareToken(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "撤销失败");
    } finally {
      setBusy(false);
    }
  };

  const handleCopy = async () => {
    if (!fullLink) return;
    try {
      await navigator.clipboard.writeText(fullLink);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      /* ignore */
    }
  };

  return (
    <div
      className={styles.overlay}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className={styles.modal} role="dialog" aria-modal="true">
        <div className={styles.titleRow}>
          <h2 className={styles.title}>分享对话</h2>
          <button
            className={styles.closeBtn}
            onClick={onClose}
            aria-label="关闭"
          >
            <CloseIcon />
          </button>
        </div>

        {loading ? (
          <div className={styles.empty}>加载中…</div>
        ) : shareToken ? (
          <>
            <p className={styles.subtitle}>
              拿到这个链接的人无需登录就能查看这个对话的当前快照。
              他们也可以一键导入到自己的账号继续聊。
            </p>
            <div className={styles.linkRow}>
              <input
                className={styles.linkInput}
                type="text"
                readOnly
                value={fullLink}
                onClick={(e) => (e.target as HTMLInputElement).select()}
              />
              <button
                className={styles.copyBtn}
                onClick={handleCopy}
                disabled={busy}
              >
                {copied ? "已复制" : "复制"}
              </button>
            </div>
            <div className={styles.actions}>
              <button
                className={`${styles.actionBtn} ${styles.actionBtnDanger}`}
                onClick={handleRevoke}
                disabled={busy}
              >
                撤销链接
              </button>
            </div>
          </>
        ) : (
          <>
            <p className={styles.subtitle}>
              生成一个分享链接，把这个对话的当前快照分享给任何人。
              对方无需登录就能查看，也可以一键导入到自己的账号。
            </p>
            <button
              className={styles.createBtn}
              onClick={handleCreate}
              disabled={busy}
            >
              {busy ? "生成中…" : "生成分享链接"}
            </button>
          </>
        )}

        {error && <div className={styles.error}>⚠️ {error}</div>}
      </div>
    </div>
  );
}

function CloseIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}
