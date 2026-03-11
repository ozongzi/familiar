import { useEffect, useRef, useState, useCallback } from "react";
import { Sidebar } from "../components/Sidebar";
import { MessageBubble } from "../components/MessageBubble";
import { ChatInput } from "../components/ChatInput";
import { useAuth } from "../store/auth.shared";
import { useConversations } from "../hooks/useConversations";
import { useChat } from "../hooks/useChat";
import { api } from "../api/client";
import styles from "./ChatPage.module.css";

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
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);

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

  // Close sidebar when selecting a conversation on mobile
  const handleSelectConversation = useCallback((id: string) => {
    if (id !== activeId) {
      setActiveId(id);
      // Close sidebar on mobile after selection
      if (window.innerWidth < 768) {
        setSidebarOpen(false);
      }
    }
  }, [activeId]);

  // Close sidebar when clicking outside on mobile
  const handleOverlayClick = useCallback(() => {
    setSidebarOpen(false);
  }, []);

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

    // Defer synchronous state updates out of the effect body to avoid the
    // react-hooks/set-state-in-effect lint rule (which warns about
    // cascaded renders when setState is called synchronously in an effect).
    const startTimer = setTimeout(() => {
      setHistoryLoading(true);
      clearBubbles();
    }, 0);

    // Load history asynchronously.
    (async () => {
      try {
        const msgs = await api.listMessages(token, activeId);
        if (!cancelled) {
          setHistory(msgs);
          setHistoryLoading(false);
          // Only open the reattach WS after history is loaded so replay
          // events never race with setHistory overwriting bubbles.
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
      // Defer setting state to avoid calling setState synchronously within the
      // effect body (avoids react-hooks/set-state-in-effect warnings).
      const t = setTimeout(() => {
        setActiveId(conversations[0].id);
      }, 0);
      return () => clearTimeout(t);
    }
  }, [convsLoading, conversations, activeId]);

  // Handle resize - close sidebar on desktop
  useEffect(() => {
    const handleResize = () => {
      if (window.innerWidth >= 768) {
        setSidebarOpen(false);
      }
    };

    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  // ── Handlers ─────────────────────────────────────────────────────────────

  const handleCreate = useCallback(async () => {
    const conv = await createConversation();
    if (conv) {
      setActiveId(conv.id);
      // Close sidebar on mobile after creating
      if (window.innerWidth < 768) {
        setSidebarOpen(false);
      }
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

  // ── Derive UI state ─────────────────────────────────────────────────────

  const isStreaming = status === "connecting" || status === "streaming";
  const activeConv = conversations.find((c) => c.id === activeId);

  return (
    <div className={styles.layout}>
      {/* Sidebar overlay for mobile */}
      <div
        className={`${styles.sidebarOverlay} ${sidebarOpen ? styles.visible : ""}`}
        onClick={handleOverlayClick}
        aria-hidden="true"
      />

      {/* Sidebar */}
      <div className={`${styles.sidebarContainer} ${sidebarVisible ? "" : styles.sidebarHidden}`}>
        <Sidebar
          conversations={conversations}
          activeId={activeId}
          loading={convsLoading}
          onSelect={handleSelectConversation}
          onCreate={handleCreate}
          onDelete={handleDelete}
          onRename={handleRename}
          userName={user?.name ?? ""}
          onLogout={logout}
          isOpen={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
        />
      </div>

      {/* Main panel */}
      <main className={styles.main}>
        {/* Header */}
        <header className={styles.header}>
          <button
            className={styles.menuBtn}
            onClick={() => setSidebarOpen(true)}
            aria-label="打开菜单"
          >
            <MenuIcon />
          </button>
          <button
            className={styles.sidebarToggle}
            onClick={() => setSidebarVisible(!sidebarVisible)}
            aria-label={sidebarVisible ? "收起侧边栏" : "展开侧边栏"}
          >
            {sidebarVisible ? <SidebarCloseIcon /> : <SidebarOpenIcon />}
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
                点击左上角菜单图标开始对话
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

/* ─── Icons ─────────────────────────────────────────────────────────────── */

function MenuIcon() {
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="3" y1="12" x2="21" y2="12" />
      <line x1="3" y1="6" x2="21" y2="6" />
      <line x1="3" y1="18" x2="21" y2="18" />
    </svg>
  );
}

function SidebarOpenIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <line x1="9" y1="3" x2="9" y2="21" />
    </svg>
  );
}

function SidebarCloseIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <line x1="15" y1="3" x2="15" y2="21" />
    </svg>
  );
}
