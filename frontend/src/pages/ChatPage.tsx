import { useEffect, useRef, useState, useCallback } from "react";
import { Sidebar } from "../components/Sidebar";
import { MessageBubble } from "../components/MessageBubble";
import { ChatInput } from "../components/ChatInput";
import { useAuth } from "../store/auth.shared";
import { useConversations } from "../hooks/useConversations";
import { useChat } from "../hooks/useChat";
import { api } from "../api/client";
import styles from "./ChatPage.module.css";

// 简单的 SVG 汉堡图标组件
const MenuIcon = () => (
  <svg
    width="24"
    height="24"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <line x1="3" y1="12" x2="21" y2="12"></line>
    <line x1="3" y1="6" x2="21" y2="6"></line>
    <line x1="3" y1="18" x2="21" y2="18"></line>
  </svg>
);

export function ChatPage() {
  const { token, user, logout } = useAuth();
  const {
    conversations,
    loading: convsLoading,
    createConversation,
    deleteConversation,
    renameConversation,
  } = useConversations(token);

  const [activeId, setActiveId] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);

  // ── 移动端侧边栏状态 ──────────────────────────────────────────────────────
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);

  const {
    bubbles,
    status,
    errorMsg,
    send,
    interrupt,
    abort,
    answerQuestion,
    reattach,
    setHistory,
    clearBubbles,
  } = useChat(activeId, token);

  const bottomRef = useRef<HTMLDivElement>(null);

  // ── Auto-scroll to bottom whenever bubbles change ────────────────────────
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [bubbles]);

  // ── When active conversation changes, load history ───────────────────────
  useEffect(() => {
    if (!activeId || !token) {
      clearBubbles();
      return;
    }

    let cancelled = false;

    const startTimer = setTimeout(() => {
      setHistoryLoading(true);
      clearBubbles();
    }, 0);

    (async () => {
      try {
        const msgs = await api.listMessages(token, activeId);
        if (!cancelled) {
          setHistory(msgs);
          setHistoryLoading(false);
          reattach(activeId, token);
        }
      } catch {
        if (!cancelled) {
          setHistoryLoading(false);
          reattach(activeId, token);
        }
      }
    })();

    return () => {
      cancelled = true;
      clearTimeout(startTimer);
    };
  }, [activeId, token, clearBubbles, setHistory, reattach]);

  // ── Auto-select first conversation after load ────────────────────────────
  useEffect(() => {
    if (!convsLoading && conversations.length > 0 && activeId === null) {
      const t = setTimeout(() => {
        setActiveId(conversations[0].id);
      }, 0);
      return () => clearTimeout(t);
    }
  }, [convsLoading, conversations, activeId]);

  // ── Handlers ─────────────────────────────────────────────────────────────

  const handleCreate = useCallback(async () => {
    const conv = await createConversation();
    if (conv) {
      setActiveId(conv.id);
      setIsSidebarOpen(false); // 移动端新建后自动收起侧边栏
    }
  }, [createConversation]);

  const handleDelete = useCallback(
    async (id: string) => {
      const ok = await deleteConversation(id);
      if (ok && activeId === id) {
        setActiveId(null);
      }
    },
    [deleteConversation, activeId],
  );

  const handleRename = useCallback(
    async (id: string, name: string) => {
      await renameConversation(id, name);
    },
    [renameConversation],
  );

  const handleSend = useCallback(
    (text: string) => {
      if (!activeId) return;
      send(text);
    },
    [activeId, send],
  );

  const handleInterrupt = useCallback(
    (text: string) => {
      interrupt(text);
    },
    [interrupt],
  );

  const handleAbort = useCallback(() => {
    abort();
  }, [abort]);

  // ── Derive UI state ───────────────────────────────────────────────────────

  const isStreaming = status === "connecting" || status === "streaming";
  const activeConv = conversations.find((c) => c.id === activeId);

  return (
    <div className={styles.layout}>
      {/* ── 移动端遮罩层 ─────────────────────────────────────────────────── */}
      {isSidebarOpen && (
        <div
          className={styles.overlay}
          onClick={() => setIsSidebarOpen(false)}
          aria-hidden="true"
        />
      )}

      {/* ── Sidebar ─────────────────────────────────────────────────────── */}
      <div
        className={`${styles.sidebarWrapper} ${isSidebarOpen ? styles.sidebarOpen : ""}`}
      >
        <Sidebar
          conversations={conversations}
          activeId={activeId}
          loading={convsLoading}
          onSelect={(id) => {
            if (id !== activeId) setActiveId(id);
            setIsSidebarOpen(false); // 移动端选择后自动收起侧边栏
          }}
          onCreate={handleCreate}
          onDelete={handleDelete}
          onRename={handleRename}
          userName={user?.name ?? ""}
          onLogout={logout}
        />
      </div>

      {/* ── Main panel ──────────────────────────────────────────────────── */}
      <main className={styles.main}>
        {/* Header */}
        <header className={styles.header}>
          <button
            className={styles.menuButton}
            onClick={() => setIsSidebarOpen(true)}
            aria-label="Open sidebar"
          >
            <MenuIcon />
          </button>
          <h2 className={styles.convTitle}>
            {activeConv ? activeConv.name : "Familiar"}
          </h2>
        </header>

        {/* Message area */}
        <div className={styles.messages}>
          {!activeId && !convsLoading && (
            <div className={styles.empty}>
              <img src="/favicon.svg" width={52} height={52} alt="" />
              <p className={styles.emptyTitle}>欢迎使用 Familiar</p>
              <p className={styles.emptyHint}>
                点击左侧或菜单「+」新建一个对话开始聊天
              </p>
            </div>
          )}

          {activeId && historyLoading && (
            <div className={styles.empty}>
              <p className={styles.emptyHint}>加载消息中…</p>
            </div>
          )}

          {activeId && !historyLoading && bubbles.length === 0 && (
            <div className={styles.empty}>
              <img src="/favicon.svg" width={44} height={44} alt="" />
              <p className={styles.emptyHint}>发送消息开始对话</p>
            </div>
          )}

          {bubbles.map((bubble) => (
            <MessageBubble
              key={bubble.key}
              bubble={bubble}
              onAnswer={answerQuestion}
            />
          ))}

          {/* Error banner */}
          {errorMsg && (
            <div className={styles.errorBanner} role="alert">
              ⚠️ {errorMsg}
            </div>
          )}

          {/* Scroll anchor */}
          <div ref={bottomRef} />
        </div>

        {/* Input */}
        <ChatInput
          onSend={handleSend}
          onInterrupt={handleInterrupt}
          onAbort={handleAbort}
          streaming={isStreaming}
          disabled={!activeId}
          placeholder={
            !activeId
              ? "请先选择或新建一个对话"
              : "发消息… (Enter 发送，Shift+Enter 换行)"
          }
        />
      </main>
    </div>
  );
}
