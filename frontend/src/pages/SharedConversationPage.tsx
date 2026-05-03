import { useEffect, useMemo, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { api } from "../api/client";
import type { ChatBubble, SharedConversation } from "../api/types";
import { messagesToBubbles } from "../utils/messagesToBubbles";
import { MessageBubble } from "../components/MessageBubble";
import { useAuth } from "../store/auth.shared";
import styles from "./SharedConversationPage.module.css";

const PENDING_IMPORT_KEY = "familiar_pending_share_import";

/** Stash an import intent so we can resume after the visitor logs in. */
export function setPendingShareImport(token: string) {
  sessionStorage.setItem(PENDING_IMPORT_KEY, token);
}

export function consumePendingShareImport(): string | null {
  const v = sessionStorage.getItem(PENDING_IMPORT_KEY);
  if (v) sessionStorage.removeItem(PENDING_IMPORT_KEY);
  return v;
}

export function SharedConversationPage() {
  const { shareToken } = useParams<{ shareToken: string }>();
  const navigate = useNavigate();
  const { token: authToken } = useAuth();

  const [data, setData] = useState<SharedConversation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);

  useEffect(() => {
    if (!shareToken) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .getSharedConversation(shareToken)
      .then((d) => {
        if (!cancelled) {
          setData(d);
          setLoading(false);
        }
      })
      .catch((e: Error) => {
        if (!cancelled) {
          setError(e.message || "无法加载对话");
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [shareToken]);

  const bubbles: ChatBubble[] = useMemo(
    () => (data ? messagesToBubbles(data.messages) : []),
    [data],
  );

  const handleContinue = async () => {
    if (!shareToken || importing) return;
    if (!authToken) {
      // Not logged in yet — stash the token and bounce through login.
      setPendingShareImport(shareToken);
      navigate("/", { replace: false });
      return;
    }
    setImporting(true);
    try {
      const { conversation_id } = await api.importSharedConversation(
        authToken,
        shareToken,
      );
      navigate(`/${conversation_id}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : "导入失败");
      setImporting(false);
    }
  };

  return (
    <div className={styles.layout}>
      <header className={styles.header}>
        <a className={styles.brand} href="/" aria-label="Familiar">
          <img src="/favicon.svg" alt="" />
          Familiar
        </a>
        <span className={styles.divider} aria-hidden="true" />
        <span className={styles.title}>{data?.name ?? "分享的对话"}</span>
        <button
          className={styles.continueBtn}
          onClick={handleContinue}
          disabled={importing || loading || !!error}
          title="把这个对话导入到自己的账号继续聊"
        >
          {importing ? "导入中…" : "和 Familiar 继续聊"}
        </button>
      </header>

      <div className={styles.scroll}>
        {loading && <div className={styles.empty}>加载中…</div>}
        {!loading && error && <div className={styles.error}>⚠️ {error}</div>}

        {!loading && !error && data && (
          <div className={styles.messages}>
            <div className={styles.banner}>
              这是一个只读快照。点击右上角"和 Familiar 继续聊"
              可将该对话导入你的账号继续聊天。
            </div>
            {bubbles.map((bubble) => (
              <MessageBubble
                key={bubble.key}
                bubble={bubble}
                conversationId={null}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
