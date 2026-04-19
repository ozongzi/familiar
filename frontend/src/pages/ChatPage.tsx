import { useEffect, useRef, useState, useCallback } from "react";
import { Sidebar } from "../components/Sidebar";
import { SearchPanel } from "../components/SearchPanel";
import { McpSettings } from "../components/McpSettings";
import { LocalMcpSettings } from "../components/LocalMcpSettings";
import { UserSettingsModal } from "../components/UserSettingsModal";
import { MessageBubble } from "../components/MessageBubble";
import { useParams, useNavigate } from "react-router-dom";

import { ChatInput } from "../components/ChatInput";
import { ModelPicker } from "../components/ModelPicker";
import { useAuth } from "../store/auth.shared";
import { useConversations } from "../hooks/useConversations";
import { useChat } from "../hooks/useChat";
import { api } from "../api/client";
import styles from "./ChatPage.module.css";
import { getZenGreetingBySeason } from "../utils/seasonalGreeting";

// Sentinel value meaning "new draft conversation, not yet persisted".
const DRAFT_ID = "__draft__" as const;

export function ChatPage() {
  const { token, user, logout } = useAuth();
  const { conversationId } = useParams();
  const navigate = useNavigate();

  const {
    conversations,
    loading: convsLoading,
    createConversation,
    deleteConversation,
    renameConversation,
  } = useConversations(token);

  // Derived from URL
  const activeId = conversationId ?? DRAFT_ID;

  const [historyLoading, setHistoryLoading] = useState(false);
  const [draftModelId, setDraftModelId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [localMcpOpen, setLocalMcpOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);

  // When useChat creates a conversation in draft mode, we want to update
  // activeId WITHOUT triggering the history-load effect (there's no history
  // yet, and a clearBubbles() would wipe the optimistic user bubble).
  // We use a ref flag that the effect reads synchronously before deciding
  // whether to load history.
  const skipNextHistoryLoadRef = useRef(false);

  // ── Draft-mode conversation factory passed to useChat ──────────────────
  // Creates a real conversation and returns its id.
  const draftModelIdRef = useRef<string | null>(null);
  useEffect(() => {
    draftModelIdRef.current = draftModelId;
  }, [draftModelId]);

  const createDraftConversation = useCallback(async (): Promise<
    string | null
  > => {
    const conv = await createConversation(undefined, draftModelIdRef.current);
    if (!conv) return null;
    // Set the flag before navigating so the effect sees it synchronously.
    skipNextHistoryLoadRef.current = true;
    navigate(`/${conv.id}`, { replace: true });
    return conv.id;
  }, [createConversation, navigate]);

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
    branch,
  } = useChat(
    activeId === DRAFT_ID ? null : activeId,
    token,
    createDraftConversation,
    { onConversationCreated: autoTitle },
  );

  const messagesRef = useRef<HTMLDivElement>(null);
  const lastUserBubbleRef = useRef<HTMLDivElement>(null);
  const lastBubbleRef = useRef<HTMLDivElement>(null);
  const [showScrollDown, setShowScrollDown] = useState(false);

  // Watch whether the last bubble is inside the scroll viewport. When it
  // scrolls out of view (user scrolled up, or assistant reply grew past the
  // fold), surface a jump-to-bottom button.
  const lastBubbleKey = bubbles.length > 0 ? bubbles[bubbles.length - 1].key : null;
  useEffect(() => {
    const target = lastBubbleRef.current;
    const root = messagesRef.current;
    if (!target || !root) {
      setShowScrollDown(false);
      return;
    }
    const io = new IntersectionObserver(
      (entries) => setShowScrollDown(!entries[0].isIntersecting),
      { root, threshold: 0, rootMargin: "0px 0px -40px 0px" },
    );
    io.observe(target);
    return () => io.disconnect();
  }, [lastBubbleKey]);

  const handleScrollToBottom = useCallback(() => {
    lastBubbleRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, []);

  // When a new user bubble appears during connecting/streaming, scroll it to top
  const lastUserBubbleKey = (() => {
    for (let i = bubbles.length - 1; i >= 0; i--) {
      if (bubbles[i].role === "user") return bubbles[i].key;
    }
    return null;
  })();

  useEffect(() => {
    if (status !== "connecting" && status !== "streaming") return;
    requestAnimationFrame(() =>
      lastUserBubbleRef.current?.scrollIntoView({ block: "start", behavior: "instant" })
    );
   
  }, [lastUserBubbleKey, status]);

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
    setHistoryLoading(true);
    clearBubbles();

    (async () => {
      try {
        const msgs = await api.listMessages(token, activeId);
        if (!cancelled) {
          setHistory(msgs);
          setHistoryLoading(false);
          reattach(activeId, token);
          // Scroll last bubble into view after history renders (double rAF for React flush)
          requestAnimationFrame(() =>
            requestAnimationFrame(() =>
              lastBubbleRef.current?.scrollIntoView({ block: "end", behavior: "instant" })
            )
          );
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

  // ── Validation: if direct link is invalid, go back to root ────────────
  useEffect(() => {
    if (!convsLoading && activeId !== DRAFT_ID) {
      const exists = conversations.some((c) => c.id === activeId);
      if (!exists) {
        navigate("/", { replace: true });
      }
    }
  }, [convsLoading, conversations, activeId, navigate]);

  const layoutRef = useRef<HTMLDivElement>(null);

  // ── Handle Visual Viewport (Mobile Keyboard) ──────────────────────────
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return;

    const handleVVChange = () => {
      if (layoutRef.current) {
        layoutRef.current.style.height = `${vv.height}px`;
      }
      // On some iOS versions, we might need to scroll the body to top to prevent
      // the browser from "helping" us by scrolling the page and hiding our header.
      window.scrollTo(0, 0);
    };

    vv.addEventListener("resize", handleVVChange);
    vv.addEventListener("scroll", handleVVChange);
    handleVVChange();

    return () => {
      vv.removeEventListener("resize", handleVVChange);
      vv.removeEventListener("scroll", handleVVChange);
    };
  }, []);

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
    navigate("/");
    if (window.innerWidth < 768) setSidebarOpen(false);
  }, [navigate]);

  const handleSelectConversation = useCallback(
    (id: string) => {
      if (id !== activeId) {
        navigate(`/${id}`);
        if (window.innerWidth < 768) setSidebarOpen(false);
      }
    },
    [activeId, navigate],
  );

  const handleDelete = useCallback(
    async (id: string) => {
      const ok = await deleteConversation(id);
      if (ok && activeId === id) {
        navigate("/");
      }
    },
    [deleteConversation, activeId, navigate],
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
    <div ref={layoutRef} className={styles.layout}>
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
          onOpenSearch={() => setSearchOpen(true)}
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

      {searchOpen && token && (
        <SearchPanel
          token={token}
          onSelectConversation={(id) => {
            handleSelectConversation(id);
          }}
          onClose={() => setSearchOpen(false)}
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
        <div className={styles.messagesWrapper}>
        {showScrollDown && (
          <button
            type="button"
            className={styles.scrollToBottomBtn}
            onClick={handleScrollToBottom}
            aria-label="滚动到最新消息"
          >
            <ScrollDownIcon />
          </button>
        )}
        <div ref={messagesRef} className={styles.messages}>
          {isDraft && bubbles.length === 0 && (
            <div className={styles.empty}>
              <img src="/favicon.svg" width={52} height={52} alt="" />
              <p className={styles.emptyTitle}>{getZenGreetingBySeason()}</p>
              {token && (
                <ModelPicker
                  token={token}
                  value={draftModelId}
                  onChange={setDraftModelId}
                />
              )}
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

          {bubbles.map((bubble, i) => {
            const isLast = i === bubbles.length - 1;
            const isLastUser =
              bubble.role === "user" &&
              !bubbles.slice(i + 1).some((b) => b.role === "user");
            return (
              <div
                key={bubble.key}
                ref={(el) => {
                  if (isLast) lastBubbleRef.current = el;
                  if (isLastUser) lastUserBubbleRef.current = el;
                }}
              >
                <MessageBubble
                  bubble={bubble}
                  onAnswer={answerQuestion}
                  conversationId={activeId === DRAFT_ID ? null : activeId}
                  onBranch={branch}
                />
              </div>
            );
          })}

          {errorMsg && (
            <div className={styles.errorBanner} role="alert">
              ⚠️ {errorMsg}
            </div>
          )}

          {/* Spacer only during streaming so user message can scroll to top */}
          {(status === "streaming" || status === "connecting") && (
            <div style={{ minHeight: "100vh", flexShrink: 0 }} aria-hidden="true" />
          )}

        </div>
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
          placeholder="请说……"
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

function ScrollDownIcon() {
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
      <path d="M12 5v14M5 12l7 7 7-7" />
    </svg>
  );
}

