import { useEffect, useRef, useState, useCallback } from "react";
import { Sidebar } from "../components/Sidebar";
import { McpSettings } from "../components/McpSettings";
import { LocalMcpSettings } from "../components/LocalMcpSettings";
import { UserSettingsModal } from "../components/UserSettingsModal";
import { MessageBubble } from "../components/MessageBubble";
import type { ChatBubble, BubbleGroup } from "../api/types";

/** 把连续 ToolBubble 合并成一个 tools 组，其余保持 single */
function groupBubbles(bubbles: ChatBubble[]): BubbleGroup[] {
  const groups: BubbleGroup[] = [];
  for (const b of bubbles) {
    if (b.kind === "tool") {
      const last = groups[groups.length - 1];
      if (last?.kind === "tools") {
        last.bubbles.push(b);
      } else {
        groups.push({ kind: "tools", bubbles: [b] });
      }
    } else {
      groups.push({ kind: "single", bubble: b });
    }
  }
  return groups;
}
import { ChatInput } from "../components/ChatInput";
import { useAuth } from "../store/auth.shared";
import { useConversations } from "../hooks/useConversations";
import { useChat } from "../hooks/useChat";
import { api } from "../api/client";
import styles from "./ChatPage.module.css";
import { getZenGreetingBySeason } from "../utils/seasonalGreeting";

// Sentinel value meaning "new draft conversation, not yet persisted".
const DRAFT_ID = "__draft__" as const;

export function ChatPage({
  initialConversationId,
}: {
  initialConversationId: string | null;
}) {
  const { token, user, logout } = useAuth();
  const {
    conversations,
    loading: convsLoading,
    createConversation,
    deleteConversation,
    renameConversation,
  } = useConversations(token);

  // null  = loading, DRAFT_ID = new draft, otherwise a real conversation id.
  const [activeId, setActiveId] = useState<string | null>(() => {
    // Initialize from URL if provided, otherwise null (will be set to DRAFT_ID later)
    return initialConversationId || null;
  });
  const [historyLoading, setHistoryLoading] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [localMcpOpen, setLocalMcpOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  // When useChat creates a conversation in draft mode, we want to update
  // activeId WITHOUT triggering the history-load effect (there's no history
  // yet, and a clearBubbles() would wipe the optimistic user bubble).
  // We use a ref flag that the effect reads synchronously before deciding
  // whether to load history.
  const skipNextHistoryLoadRef = useRef(false);

  // ── Draft-mode conversation factory passed to useChat ──────────────────
  // Creates a real conversation and returns its id.  Does NOT call setActiveId
  // here — that happens in onConversationCreated so we can set the skip flag
  // first.
  const createDraftConversation = useCallback(async (): Promise<
    string | null
  > => {
    const conv = await createConversation();
    if (!conv) return null;
    // Set the flag before setActiveId so the effect sees it synchronously.
    skipNextHistoryLoadRef.current = true;
    setActiveId(conv.id);
    return conv.id;
  }, [createConversation]);

  const autoTitle = useCallback(
    async (convId: string, firstMessage: string) => {
      if (!token) return;
      const prompt = firstMessage.trim().slice(0, 200);
      try {
        const { title } = await api.autoTitle(token, convId, prompt);
        if (title) await renameConversation(convId, title);
      } catch {
        // Non-critical; silently ignore.
      }
    },
    [token, renameConversation],
  );

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
    addUploadBubble,
  } = useChat(
    activeId === DRAFT_ID ? null : activeId,
    token,
    createDraftConversation,
    { onConversationCreated: autoTitle },
  );

  const messagesRef = useRef<HTMLDivElement>(null);
  const lockedRef = useRef(true);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = "auto") => {
    const el = messagesRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior });
  }, []);

  // Unlock auto-scroll when the user scrolls up; re-lock when back at bottom.
  useEffect(() => {
    const el = messagesRef.current;
    if (!el) return;
    const onScroll = () => {
      lockedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  // ── Auto-scroll ────────────────────────────────────────────────────────
  useEffect(() => {
    if (!lockedRef.current) return;
    // Keep the viewport pinned while stream/tool output is growing,
    // preventing large chunks from knocking us out of auto-scroll mode.
    scrollToBottom("auto");
  }, [bubbles, status, scrollToBottom]);

  // ── Load history when switching to a real conversation ─────────────────
  useEffect(() => {
    if (!activeId || activeId === DRAFT_ID || !token) {
      clearBubbles();
      return;
    }

    // If this activeId change was caused by useChat creating a draft conversation,
    // skip the history load — the WS send is already in flight and bubbles are live.
    if (skipNextHistoryLoadRef.current) {
      skipNextHistoryLoadRef.current = false;
      return;
    }

    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setHistoryLoading(true);
    clearBubbles();

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
    };
  }, [activeId, token, clearBubbles, setHistory, reattach]);

  // ── On initial load: validate activeId and start in draft mode if needed ────
  useEffect(() => {
    if (!convsLoading) {
      // If activeId is null, set to draft mode
      if (activeId === null) {
        // eslint-disable-next-line react-hooks/set-state-in-effect
        setActiveId(DRAFT_ID);
      }
      // If activeId is a real conversation, verify it exists in the list
      else if (activeId !== DRAFT_ID) {
        const exists = conversations.some((c) => c.id === activeId);
        if (!exists) {
          // Conversation doesn't exist, fall back to draft mode
          setActiveId(DRAFT_ID);
        }
      }
    }
  }, [convsLoading, activeId, conversations]);

  // ── Sync activeId to URL ────────────────────────────────────────────────
  useEffect(() => {
    if (activeId === DRAFT_ID) {
      // Draft mode: use root path
      window.history.pushState({}, "", "/");
    } else if (activeId && activeId !== null) {
      // Real conversation: use conversation ID as path
      window.history.pushState({}, "", `/${activeId}`);
    }
  }, [activeId]);

  // ── Handle resize ──────────────────────────────────────────────────────
  useEffect(() => {
    const handleResize = () => {
      if (window.innerWidth >= 768) setSidebarOpen(false);
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  // ── Handlers ──────────────────────────────────────────────────────────

  // Enter draft mode (new conversation).
  const handleCreate = useCallback(() => {
    setActiveId(DRAFT_ID);
    if (window.innerWidth < 768) setSidebarOpen(false);
  }, []);

  const handleSelectConversation = useCallback(
    (id: string) => {
      if (id !== activeId) {
        setActiveId(id);
        if (window.innerWidth < 768) setSidebarOpen(false);
      }
    },
    [activeId],
  );

  const handleDelete = useCallback(
    async (id: string) => {
      const ok = await deleteConversation(id);
      if (ok && activeId === id) {
        setActiveId(DRAFT_ID);
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
      send(text);
    },
    [send],
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

  const handleUpload = useCallback(
    (result: { filename: string; path: string; size: number }) => {
      addUploadBubble(result.filename, result.path, result.size);
    },
    [addUploadBubble],
  );

  // Called by ChatInput when it needs a conversation id but we're in draft mode.
  // Creates a real conversation so the upload can be linked to it.
  const handleRequestConversationId = useCallback(async (): Promise<
    string | null
  > => {
    if (activeId && activeId !== DRAFT_ID) return activeId;
    return await createDraftConversation();
  }, [activeId, createDraftConversation]);

  // ── Derive UI state ────────────────────────────────────────────────────

  const isStreaming = status === "connecting" || status === "streaming";
  const isDraft = activeId === DRAFT_ID;
  // For the sidebar, treat the draft as no active selection.
  const sidebarActiveId = isDraft ? null : activeId;
  const activeConv = conversations.find((c) => c.id === activeId);

  return (
    <div className={styles.layout}>
      {/* Sidebar overlay for mobile */}
      <div
        className={`${styles.sidebarOverlay} ${sidebarOpen ? styles.visible : ""}`}
        onClick={() => setSidebarOpen(false)}
        aria-hidden="true"
      />

      {/* Sidebar */}
      <div
        className={`${styles.sidebarContainer} ${sidebarVisible ? "" : styles.sidebarHidden}`}
      >
        <Sidebar
          conversations={conversations}
          activeId={sidebarActiveId}
          loading={convsLoading}
          onSelect={handleSelectConversation}
          onCreate={handleCreate}
          onDelete={handleDelete}
          onRename={handleRename}
          userName={user?.name ?? ""}
          user={user}
          onLogout={logout}
          onOpenSettings={() => setSettingsOpen(true)}
          isOpen={sidebarOpen}
          onClose={() => setSidebarOpen(false)}
        />
      </div>

      {mcpOpen && token && (
        <McpSettings token={token} onClose={() => setMcpOpen(false)} />
      )}

      {localMcpOpen && (
        <LocalMcpSettings onClose={() => setLocalMcpOpen(false)} />
      )}

      {settingsOpen && token && (
        <UserSettingsModal
          token={token}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {/* Main panel */}
      <main className={styles.main}>
        {/* Header */}
        <header className={styles.header}>
          {/* Mobile: hamburger */}
          <button
            className={styles.menuBtn}
            onClick={() => setSidebarOpen(true)}
            aria-label="打开菜单"
          >
            <MenuIcon />
          </button>

          {/* Desktop: sidebar toggle */}
          <button
            className={styles.sidebarToggle}
            onClick={() => setSidebarVisible(!sidebarVisible)}
            aria-label={sidebarVisible ? "收起侧边栏" : "展开侧边栏"}
          >
            {sidebarVisible ? <SidebarCloseIcon /> : <SidebarOpenIcon />}
          </button>

          <h2 className={styles.convTitle}>
            {isDraft ? "新对话" : (activeConv?.name ?? "Familiar")}
          </h2>

          {/* Mobile: new conversation button */}
          <button
            className={styles.newConvBtn}
            onClick={handleCreate}
            aria-label="新建对话"
            title="新建对话"
          >
            <NewChatIcon />
          </button>
        </header>

        {/* Message area */}
        <div ref={messagesRef} className={styles.messages}>
          {isDraft && bubbles.length === 0 && (
            <div className={styles.empty}>
              <img src="/favicon.svg" width={52} height={52} alt="" />
              <p className={styles.emptyTitle}>{getZenGreetingBySeason()}</p>
            </div>
          )}

          {!isDraft && historyLoading && (
            <div className={styles.empty}>
              <p className={styles.emptyHint}>加载消息中…</p>
            </div>
          )}

          {!isDraft && !historyLoading && bubbles.length === 0 && (
            <div className={styles.empty}>
              <img src="/favicon.svg" width={44} height={44} alt="" />
              <p className={styles.emptyHint}>发送消息开始对话</p>
            </div>
          )}

          {groupBubbles(bubbles).map((group) => {
            if (group.kind === "single") {
              return (
                <MessageBubble
                  key={group.bubble.key}
                  bubble={group.bubble}
                  onAnswer={answerQuestion}
                />
              );
            }
            // 工具组：合并渲染
            return (
              <MessageBubble
                key={group.bubbles[0].key}
                bubble={group.bubbles[0]}
                extraTools={group.bubbles.slice(1)}
                onAnswer={answerQuestion}
              />
            );
          })}

          {errorMsg && (
            <div className={styles.errorBanner} role="alert">
              ⚠️ {errorMsg}
            </div>
          )}
        </div>

        {/* Input — always enabled in draft mode */}
        <ChatInput
          onSend={handleSend}
          onInterrupt={handleInterrupt}
          onAbort={handleAbort}
          streaming={isStreaming}
          disabled={false}
          token={token}
          conversationId={isDraft ? null : activeId}
          requestConversationId={handleRequestConversationId}
          onUpload={handleUpload}
          onOpenMcp={() => setMcpOpen(true)}
          onOpenLocalMcp={() => setLocalMcpOpen(true)}
          placeholder="发消息… (Enter 发送，Shift+Enter 换行)"
        />
      </main>
    </div>
  );
}

/* ─── Icons ──────────────────────────────────────────────────────────────── */

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

function NewChatIcon() {
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
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}
