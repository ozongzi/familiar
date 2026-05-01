import { useState, useRef, useEffect, type KeyboardEvent } from "react";
import type { Conversation, Folder, MeResponse } from "../api/types";
import { Avatar } from "./Avatar";
import styles from "./Sidebar.module.css";
import { useNavigate } from "react-router-dom";

/* ─── Tree types ──────────────────────────────────────────────────────────── */

interface FolderNode {
  type: "folder";
  id: string;
  name: string;
  children: TreeNode[];
}

interface ConvNode {
  type: "conversation";
  id: string;
  name: string;
  folder_id: string | null;
}

type TreeNode = FolderNode | ConvNode;

function buildTree(
  folders: Folder[],
  conversations: Conversation[],
): TreeNode[] {
  const folderMap = new Map<string, FolderNode>();
  const roots: TreeNode[] = [];

  // Create folder nodes
  for (const f of folders) {
    folderMap.set(f.id, {
      type: "folder",
      id: f.id,
      name: f.name,
      children: [],
    });
  }

  // Link folders into tree
  for (const f of folders) {
    const node = folderMap.get(f.id)!;
    if (f.parent_id && folderMap.has(f.parent_id)) {
      folderMap.get(f.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }

  // Place conversations into their folders or root
  for (const c of conversations) {
    const convNode: ConvNode = {
      type: "conversation",
      id: c.id,
      name: c.name,
      folder_id: c.folder_id,
    };
    if (c.folder_id && folderMap.has(c.folder_id)) {
      folderMap.get(c.folder_id)!.children.push(convNode);
    } else {
      roots.push(convNode);
    }
  }

  return roots;
}

/* ─── Context menu state ──────────────────────────────────────────────────── */

interface ContextMenuState {
  x: number;
  y: number;
  target: { type: "folder"; id: string } | { type: "conversation"; id: string };
}

/* ─── Props ───────────────────────────────────────────────────────────────── */

interface Props {
  conversations: Conversation[];
  folders: Folder[];
  activeId: string | null;
  loading: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onCreateFolder: (name: string, parentId?: string | null) => void;
  onDeleteFolder: (id: string) => void;
  onRenameFolder: (id: string, name: string) => void;
  onMoveConversation: (convId: string, folderId: string | null) => void;
  userName: string;
  user?: MeResponse | null;
  onLogout: () => void;
  onOpenSettings?: () => void;
  onOpenSearch?: () => void;
  isOpen?: boolean;
  onClose?: () => void;
}

/* ─── Main component ──────────────────────────────────────────────────────── */

export function Sidebar({
  conversations,
  folders,
  activeId,
  loading,
  onSelect,
  onCreate,
  onDelete,
  onRename,
  onCreateFolder,
  onDeleteFolder,
  onRenameFolder,
  onMoveConversation,
  userName,
  user,
  onLogout,
  onOpenSettings,
  onOpenSearch,
  isOpen = false,
  onClose,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const editInputRef = useRef<HTMLInputElement>(null);
  const confirmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const navigate = useNavigate();

  // Expanded folder state, persisted in localStorage
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    try {
      const saved = localStorage.getItem("familiar-folders-expanded");
      return saved ? new Set(JSON.parse(saved)) : new Set();
    } catch {
      return new Set();
    }
  });

  useEffect(() => {
    localStorage.setItem(
      "familiar-folders-expanded",
      JSON.stringify([...expanded]),
    );
  }, [expanded]);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  // Move-to submenu open
  const [moveToOpen, setMoveToOpen] = useState(false);

  // Drag state
  const [dragOverTarget, setDragOverTarget] = useState<string | null>(null);
  const [draggingId, setDraggingId] = useState<string | null>(null);

  // Close context menu on click outside
  useEffect(() => {
    if (!contextMenu) return;
    const handleClick = () => {
      setContextMenu(null);
      setMoveToOpen(false);
    };
    document.addEventListener("click", handleClick);
    return () => document.removeEventListener("click", handleClick);
  }, [contextMenu]);

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

  // Build the tree
  const tree = buildTree(folders, conversations);

  /* ── Rename helpers ─────────────────────────────────────────────────── */

  function startRenameConversation(conv: { id: string; name: string }) {
    setEditingId(conv.id);
    setEditValue(conv.name);
    setConfirmDeleteId(null);
    setContextMenu(null);
  }

  function startRenameFolder(folder: { id: string; name: string }) {
    setEditingId(folder.id);
    setEditValue(folder.name);
    setConfirmDeleteId(null);
    setContextMenu(null);
  }

  function commitRename() {
    if (!editingId) return;
    const trimmed = editValue.trim();
    if (trimmed.length > 0) {
      // Determine if this is a folder or conversation by checking folders array
      const isFolder = folders.some((f) => f.id === editingId);
      if (isFolder) {
        onRenameFolder(editingId, trimmed);
      } else {
        onRename(editingId, trimmed);
      }
    }
    setEditingId(null);
  }

  function handleRenameKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") commitRename();
    if (e.key === "Escape") setEditingId(null);
  }

  /* ── Delete helpers ─────────────────────────────────────────────────── */

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

  /* ── Folder toggle ──────────────────────────────────────────────────── */

  function toggleFolder(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  /* ── Context menu handlers ──────────────────────────────────────────── */

  function handleContextMenu(
    e: React.MouseEvent,
    target: ContextMenuState["target"],
  ) {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, target });
    setMoveToOpen(false);
  }

  function handleNewFolder() {
    onCreateFolder("New Folder");
  }

  /* ── Drag & Drop handlers ───────────────────────────────────────────── */

  function handleDragStart(e: React.DragEvent, convId: string) {
    e.dataTransfer.setData("application/familiar-conv", convId);
    e.dataTransfer.effectAllowed = "move";
    setDraggingId(convId);
  }

  function handleDragEnd() {
    setDraggingId(null);
    setDragOverTarget(null);
  }

  function handleDragOver(e: React.DragEvent, folderId: string) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDragOverTarget(folderId);
  }

  function handleDragLeave() {
    setDragOverTarget(null);
  }

  function handleDrop(e: React.DragEvent, folderId: string) {
    e.preventDefault();
    const convId = e.dataTransfer.getData("application/familiar-conv");
    if (convId) {
      onMoveConversation(convId, folderId);
    }
    setDragOverTarget(null);
    setDraggingId(null);
  }

  function handleRootDragOver(e: React.DragEvent) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  }

  function handleRootDrop(e: React.DragEvent) {
    e.preventDefault();
    const convId = e.dataTransfer.getData("application/familiar-conv");
    if (convId) {
      onMoveConversation(convId, null);
    }
    setDragOverTarget(null);
    setDraggingId(null);
  }

  /* ── Recursive tree renderer ────────────────────────────────────────── */

  function TreeNodeRenderer({
    node,
    depth,
  }: {
    node: TreeNode;
    depth: number;
  }) {
    if (node.type === "folder") {
      const isExpanded = expanded.has(node.id);
      const isDropTarget = dragOverTarget === node.id;

      return (
        <div>
          <div
            className={`${styles.folderItem} ${isDropTarget ? styles.dropTarget : ""}`}
            style={{ paddingLeft: 12 + depth * 16 }}
            onClick={() => toggleFolder(node.id)}
            onContextMenu={(e) =>
              handleContextMenu(e, { type: "folder", id: node.id })
            }
            onDragOver={(e) => handleDragOver(e, node.id)}
            onDragLeave={handleDragLeave}
            onDrop={(e) => handleDrop(e, node.id)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") toggleFolder(node.id);
            }}
          >
            <span className={styles.chevron}>
              {isExpanded ? <ChevronDownIcon /> : <ChevronRightIcon />}
            </span>
            <FolderIcon />
            {editingId === node.id ? (
              <input
                ref={editInputRef}
                className={styles.renameInput}
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
                onKeyDown={handleRenameKeyDown}
                onBlur={commitRename}
                onClick={(e) => e.stopPropagation()}
                maxLength={80}
                aria-label="Rename folder"
              />
            ) : (
              <span className={styles.folderName}>{node.name}</span>
            )}
          </div>
          {isExpanded && (
            <div>
              {node.children.map((child) => (
                <TreeNodeRenderer
                  key={child.id}
                  node={child}
                  depth={depth + 1}
                />
              ))}
            </div>
          )}
        </div>
      );
    }

    // Conversation node
    const conv = node;
    const isActive = conv.id === activeId;
    const isEditing = editingId === conv.id;
    const isConfirming = confirmDeleteId === conv.id;
    const isDragging = draggingId === conv.id;

    return (
      <div
        className={`${styles.item} ${isActive ? styles.itemActive : ""} ${isDragging ? styles.dragging : ""}`}
        style={{ paddingLeft: 12 + depth * 16 }}
        onClick={() => {
          if (!isEditing) onSelect(conv.id);
        }}
        onContextMenu={(e) =>
          handleContextMenu(e, { type: "conversation", id: conv.id })
        }
        draggable={!isEditing}
        onDragStart={(e) => handleDragStart(e, conv.id)}
        onDragEnd={handleDragEnd}
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
              aria-label="Rename conversation"
            />
          ) : (
            <span className={styles.convName}>{conv.name}</span>
          )}
        </div>

        {/* Action buttons — only visible on hover / active */}
        {!isEditing && (
          <div className={styles.actions} onClick={(e) => e.stopPropagation()}>
            <button
              className={styles.actionBtn}
              onClick={() => startRenameConversation(conv)}
              title="Rename"
              aria-label="Rename conversation"
            >
              <PencilIcon />
            </button>
            <button
              className={`${styles.actionBtn} ${
                isConfirming ? styles.actionBtnDanger : ""
              }`}
              onClick={() => handleDeleteClick(conv.id)}
              title={isConfirming ? "Click again to confirm" : "Delete"}
              aria-label={
                isConfirming
                  ? "Confirm delete conversation"
                  : "Delete conversation"
              }
            >
              {isConfirming ? <CheckIcon /> : <TrashIcon />}
            </button>
          </div>
        )}
      </div>
    );
  }

  /* ── Context menu component ─────────────────────────────────────────── */

  function ContextMenu() {
    if (!contextMenu) return null;

    const { x, y, target } = contextMenu;

    if (target.type === "folder") {
      return (
        <div
          className={styles.contextMenu}
          style={{ left: x, top: y }}
          onClick={(e) => e.stopPropagation()}
        >
          <button
            className={styles.contextMenuItem}
            onClick={() => {
              const folder = folders.find((f) => f.id === target.id);
              if (folder) startRenameFolder(folder);
              setContextMenu(null);
            }}
          >
            <PencilIcon />
            Rename
          </button>
          <div className={styles.contextMenuSeparator} />
          <button
            className={`${styles.contextMenuItem} ${styles.contextMenuDanger}`}
            onClick={() => {
              onDeleteFolder(target.id);
              setContextMenu(null);
            }}
          >
            <TrashIcon />
            Delete
          </button>
        </div>
      );
    }

    // Conversation context menu
    return (
      <div
        className={styles.contextMenu}
        style={{ left: x, top: y }}
        onClick={(e) => e.stopPropagation()}
      >
        <button
          className={styles.contextMenuItem}
          onClick={() => {
            const conv = conversations.find((c) => c.id === target.id);
            if (conv) startRenameConversation(conv);
            setContextMenu(null);
          }}
        >
          <PencilIcon />
          Rename
        </button>
        <div className={styles.contextMenuSeparator} />
        <div
          className={styles.contextMenuSubmenu}
          onMouseEnter={() => setMoveToOpen(true)}
          onMouseLeave={() => setMoveToOpen(false)}
        >
          <button
            className={styles.contextMenuItem}
            onClick={() => setMoveToOpen(!moveToOpen)}
          >
            <FolderIcon />
            Move to
          </button>
          {moveToOpen && (
            <div className={styles.contextMenuSubmenuContent}>
              <button
                className={styles.contextMenuItem}
                onClick={() => {
                  onMoveConversation(target.id, null);
                  setContextMenu(null);
                  setMoveToOpen(false);
                }}
              >
                Root
              </button>
              {folders.map((f) => (
                <button
                  key={f.id}
                  className={styles.contextMenuItem}
                  onClick={() => {
                    onMoveConversation(target.id, f.id);
                    setContextMenu(null);
                    setMoveToOpen(false);
                  }}
                >
                  <FolderIcon />
                  {f.name}
                </button>
              ))}
            </div>
          )}
        </div>
        <div className={styles.contextMenuSeparator} />
        <button
          className={`${styles.contextMenuItem} ${styles.contextMenuDanger}`}
          onClick={() => {
            onDelete(target.id);
            setContextMenu(null);
          }}
        >
          <TrashIcon />
          Delete
        </button>
      </div>
    );
  }

  /* ── Render ─────────────────────────────────────────────────────────── */

  return (
    <aside className={`${styles.sidebar} ${isOpen ? styles.open : ""}`}>
      {/* Header */}
      <div className={styles.header}>
        <button
          className={styles.closeBtn}
          onClick={onClose}
          aria-label="Close menu"
        >
          <CloseIcon />
        </button>
        <span className={styles.logo}>
          <img src="/favicon.svg" width={22} height={22} alt="" />
          Familiar
        </span>
        <button
          className={styles.searchBtn}
          onClick={onOpenSearch}
          title="Search history"
          aria-label="Search history"
        >
          <SearchIcon />
        </button>
        <button
          className={styles.newBtn}
          onClick={handleNewFolder}
          title="New folder"
          aria-label="New folder"
        >
          <FolderPlusIcon />
        </button>
        <button
          className={styles.newBtn}
          onClick={onCreate}
          title="New conversation"
          aria-label="New conversation"
        >
          <PlusIcon />
        </button>
      </div>

      {/* Conversation list */}
      <nav
        className={styles.list}
        aria-label="Conversation list"
        onDragOver={handleRootDragOver}
        onDrop={handleRootDrop}
      >
        {loading && conversations.length === 0 && folders.length === 0 && (
          <p className={styles.empty}>Loading...</p>
        )}
        {!loading && conversations.length === 0 && folders.length === 0 && (
          <p className={styles.empty}>
            No conversations yet. Click + to create one.
          </p>
        )}

        {tree.map((node) => (
          <TreeNodeRenderer key={node.id} node={node} depth={0} />
        ))}
      </nav>

      {/* Context Menu */}
      <ContextMenu />

      {/* Footer / user info */}
      <div className={styles.footer}>
        <div className={styles.userInfo}>
          {user && <Avatar user={user} size="sm" />}
          <span className={styles.userName} title={userName}>
            {userName}
          </span>
        </div>
        <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
          {user?.is_admin && (
            <button
              className={styles.adminBtn}
              onClick={() => navigate("/admin")}
              title="Admin panel"
              aria-label="Open admin panel"
            >
              <AdminIcon />
            </button>
          )}
          <button
            className={styles.logoutBtn}
            onClick={onOpenSettings}
            title="Settings"
            aria-label="Open settings"
          >
            <SettingsIcon />
          </button>
          <button
            className={styles.logoutBtn}
            onClick={onLogout}
            title="Logout"
            aria-label="Logout"
          >
            <LogoutIcon />
          </button>
        </div>
      </div>
    </aside>
  );
}

/* ─── Inline SVG Icons ───────────────────────────────────────────────────── */

function AdminIcon() {
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
      <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z" />
    </svg>
  );
}

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

function SearchIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function FolderIcon() {
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
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  );
}

function ChevronRightIcon() {
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
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
}

function ChevronDownIcon() {
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
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}

function FolderPlusIcon() {
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
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
      <line x1="12" y1="11" x2="12" y2="17" />
      <line x1="9" y1="14" x2="15" y2="14" />
    </svg>
  );
}
