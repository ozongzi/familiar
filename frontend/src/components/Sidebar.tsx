import { useState, useRef, useEffect, type KeyboardEvent } from "react";
import type { Conversation } from "../api/types";
import styles from "./Sidebar.module.css";

interface Props {
  conversations: Conversation[];
  activeId: string | null;
  loading: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, name: string) => void;
  userName: string;
  onLogout: () => void;
  onOpenSettings?: () => void;
  isOpen?: boolean;
  onClose?: () => void;
}

export function Sidebar({
  conversations,
  activeId,
  loading,
  onSelect,
  onCreate,
  onDelete,
  onRename,
  userName,
  onLogout,
  onOpenSettings,
  isOpen = false,
  onClose,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const editInputRef = useRef<HTMLInputElement>(null);
  const confirmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Focus the rename input when it appears
  useEffect(() => {
    if (editingId) {
      editInputRef.current?.focus();
      editInputRef.current?.select();
    }
  }, [editingId]);

  // Clear the confirm-delete timer on unmount.
  useEffect(() => {
    return () => {
      if (confirmTimerRef.current !== null)
        clearTimeout(confirmTimerRef.current);
    };
  }, []);

  function startRename(conv: Conversation) {
    setEditingId(conv.id);
    setEditValue(conv.name);
    setConfirmDeleteId(null);
  }

  function commitRename() {
    if (!editingId) return;
    const trimmed = editValue.trim();
    if (trimmed.length > 0) {
      onRename(editingId, trimmed);
    }
    setEditingId(null);
  }

  function handleRenameKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") commitRename();
    if (e.key === "Escape") setEditingId(null);
  }

  function handleDeleteClick(id: string) {
    if (confirmDeleteId === id) {
      if (confirmTimerRef.current !== null) {
        clearTimeout(confirmTimerRef.current);
        confirmTimerRef.current = null;
      }
      onDelete(id);
      setConfirmDeleteId(null);
    } else {
      if (confirmTimerRef.current !== null)
        clearTimeout(confirmTimerRef.current);
      setConfirmDeleteId(id);
      confirmTimerRef.current = setTimeout(() => {
        setConfirmDeleteId(null);
        confirmTimerRef.current = null;
      }, 3000);
    }
  }

  return (
    <aside className={`${styles.sidebar} ${isOpen ? styles.open : ""}`}>
      {/* Header */}
      <div className={styles.header}>
        <button
          className={styles.closeBtn}
          onClick={onClose}
          aria-label="关闭菜单"
        >
          <CloseIcon />
        </button>
        <span className={styles.logo}>
          <img src="/favicon.svg" width={22} height={22} alt="" />
          Familiar
        </span>
        <button
          className={styles.newBtn}
          onClick={onCreate}
          title="新建对话"
          aria-label="新建对话"
        >
          <PlusIcon />
        </button>
      </div>

      {/* Conversation list */}
      <nav className={styles.list} aria-label="对话列表">
        {loading && conversations.length === 0 && (
          <p className={styles.empty}>加载中…</p>
        )}
        {!loading && conversations.length === 0 && (
          <p className={styles.empty}>还没有对话，点击 + 新建</p>
        )}

        {conversations.map((conv) => {
          const isActive = conv.id === activeId;
          const isEditing = editingId === conv.id;
          const isConfirming = confirmDeleteId === conv.id;

          return (
            <div
              key={conv.id}
              className={`${styles.item} ${isActive ? styles.itemActive : ""}`}
              onClick={() => {
                if (!isEditing) onSelect(conv.id);
              }}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  if (!isEditing) onSelect(conv.id);
                }
              }}
              aria-current={isActive ? "page" : undefined}
            >
              <div className={styles.itemInner}>
                {isEditing ? (
                  <input
                    ref={editInputRef}
                    className={styles.renameInput}
                    value={editValue}
                    onChange={(e) => setEditValue(e.target.value)}
                    onKeyDown={handleRenameKeyDown}
                    onBlur={commitRename}
                    onClick={(e) => e.stopPropagation()}
                    maxLength={80}
                    aria-label="重命名对话"
                  />
                ) : (
                  <span className={styles.convName}>{conv.name}</span>
                )}
              </div>

              {/* Action buttons — only visible on hover / active */}
              {!isEditing && (
                <div
                  className={styles.actions}
                  onClick={(e) => e.stopPropagation()}
                >
                  <button
                    className={styles.actionBtn}
                    onClick={() => startRename(conv)}
                    title="重命名"
                    aria-label="重命名对话"
                  >
                    <PencilIcon />
                  </button>
                  <button
                    className={`${styles.actionBtn} ${
                      isConfirming ? styles.actionBtnDanger : ""
                    }`}
                    onClick={() => handleDeleteClick(conv.id)}
                    title={isConfirming ? "再次点击确认删除" : "删除"}
                    aria-label={isConfirming ? "确认删除对话" : "删除对话"}
                  >
                    {isConfirming ? <CheckIcon /> : <TrashIcon />}
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </nav>

      {/* Footer / user info */}
      <div className={styles.footer}>
        <span className={styles.userName} title={userName}>
          <UserIcon />
          {userName}
        </span>
        <div style={{ display: "flex", gap: "8px" }}>
          <button
            className={styles.logoutBtn}
            onClick={onOpenSettings}
            title="设置"
            aria-label="打开设置"
          >
            <SettingsIcon />
          </button>
          <button
            className={styles.logoutBtn}
            onClick={onLogout}
            title="退出登录"
            aria-label="退出登录"
          >
            <LogoutIcon />
          </button>
        </div>
      </div>
    </aside>
  );
}

/* ─── Inline SVG Icons ───────────────────────────────────────────────────── */

function SettingsIcon() {
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
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

function CloseIcon() {
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
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
      <path d="M9 6V4h6v2" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

// function PlugIcon() {
//   return (
//     <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
//       <path d="M18 7l-1.5-1.5" /><path d="M6 7l1.5-1.5" />
//       <path d="M12 2v2" /><rect x="4" y="7" width="16" height="8" rx="2" />
//       <path d="M12 17v3" /><path d="M9 20h6" />
//     </svg>
//   );
// }

function LogoutIcon() {
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
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <polyline points="16 17 21 12 16 7" />
      <line x1="21" y1="12" x2="9" y2="12" />
    </svg>
  );
}
