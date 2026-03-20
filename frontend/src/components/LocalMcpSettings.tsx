import { useState, useEffect } from "react";
import { invoke, isTauri, getServerBase } from "../utils/tauri";
import { useAuth } from "../store/auth.shared";
import styles from "./McpSettings.module.css";

// ── Types ──────────────────────────────────────────────────────────────────

interface LocalMcpStdio {
  id: string;
  name: string;
  type: "stdio";
  command: string;
  args: string[];
  env?: Record<string, string>;
}

interface LocalMcpHttp {
  id: string;
  name: string;
  type: "http";
  url: string;
}

type LocalMcp = LocalMcpStdio | LocalMcpHttp;

type FormData =
  | { type: "stdio"; name: string; command: string; args: string; env: string }
  | { type: "http"; name: string; url: string };

const EMPTY_FORM: FormData = { type: "stdio", name: "", command: "", args: "", env: "{}" };

interface Props {
  onClose: () => void;
}

// ── Component ──────────────────────────────────────────────────────────────

export function LocalMcpSettings({ onClose }: Props) {
  const { token } = useAuth();
  const [mcps, setMcps] = useState<LocalMcp[]>([]);
  const [form, setForm] = useState<FormData>(EMPTY_FORM);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<LocalMcp[]>("get_local_mcps").then((data) => {
      setMcps(Array.isArray(data) ? data : []);
    }).catch(() => {});
  }, []);

  async function saveMcps(updated: LocalMcp[]) {
    setSaving(true);
    try {
      await invoke("set_local_mcps", { mcps: updated });
      setMcps(updated);
      if (token) {
        await invoke("start_tunnel", { token, serverUrl: getServerBase() });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  function resetForm() {
    setForm(EMPTY_FORM);
    setEditingId(null);
    setError("");
  }

  function startEdit(mcp: LocalMcp) {
    setEditingId(mcp.id);
    if (mcp.type === "stdio") {
      setForm({
        type: "stdio",
        name: mcp.name,
        command: mcp.command,
        args: (mcp.args || []).join(" "),
        env: JSON.stringify(mcp.env || {}, null, 2),
      });
    } else {
      setForm({ type: "http", name: mcp.name, url: mcp.url });
    }
    setError("");
  }

  async function handleSubmit() {
    setError("");
    if (!form.name.trim()) { setError("名称不能为空"); return; }

    let newMcp: LocalMcp;
    if (form.type === "stdio") {
      if (!form.command.trim()) { setError("命令不能为空"); return; }
      let env: Record<string, string> = {};
      try { env = JSON.parse(form.env || "{}"); } catch { setError("env JSON 格式错误"); return; }
      newMcp = {
        id: editingId ?? crypto.randomUUID(),
        name: form.name.trim(),
        type: "stdio",
        command: form.command.trim(),
        args: form.args.trim() ? form.args.trim().split(/\s+/) : [],
        env,
      };
    } else {
      if (!form.url.trim()) { setError("URL 不能为空"); return; }
      newMcp = {
        id: editingId ?? crypto.randomUUID(),
        name: form.name.trim(),
        type: "http",
        url: form.url.trim(),
      };
    }

    const updated = editingId
      ? mcps.map((m) => (m.id === editingId ? newMcp : m))
      : [...mcps, newMcp];

    await saveMcps(updated);
    resetForm();
  }

  async function handleDelete(id: string) {
    await saveMcps(mcps.filter((m) => m.id !== id));
    if (editingId === id) resetForm();
  }

  if (!isTauri()) {
    return (
      <div className={styles.overlay} onClick={onClose}>
        <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
          <div className={styles.header}>
            <span className={styles.title}>本地 MCP</span>
            <button className={styles.closeBtn} onClick={onClose}>✕</button>
          </div>
          <div className={styles.form}>
            <p style={{ color: "var(--text-muted)", textAlign: "center", padding: "2rem 0" }}>
              本功能仅在桌面端 (Tauri) 中可用
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className={styles.header}>
          <span className={styles.title}>本地 MCP</span>
          <button className={styles.closeBtn} onClick={onClose}>✕</button>
        </div>

        {/* List */}
        <ul className={styles.list}>
          {mcps.length === 0 && (
            <p className={styles.empty}>暂无本地 MCP，添加后自动生效</p>
          )}
          {mcps.map((mcp) => (
            <li
              key={mcp.id}
              className={`${styles.item} ${editingId === mcp.id ? styles.itemActive : ""}`}
            >
              <div className={styles.itemInfo}>
                <span className={styles.itemName}>{mcp.name}</span>
                <span className={styles.itemType}>{mcp.type}</span>
                {mcp.type === "stdio" && (
                  <span className={styles.itemDetail}>
                    {mcp.command} {(mcp.args || []).join(" ")}
                  </span>
                )}
                {mcp.type === "http" && (
                  <span className={styles.itemDetail}>{mcp.url}</span>
                )}
              </div>
              <div className={styles.itemActions}>
                <button className={styles.iconBtn} onClick={() => startEdit(mcp)} title="编辑">
                  ✎
                </button>
                <button
                  className={`${styles.iconBtn} ${styles.danger}`}
                  onClick={() => handleDelete(mcp.id)}
                  title="删除"
                >
                  ✕
                </button>
              </div>
            </li>
          ))}
        </ul>

        {/* Form */}
        <div className={styles.form}>
          {editingId && (
            <div className={styles.formTitle}>
              <span>编辑</span>
              <button className={styles.cancelBtn} onClick={resetForm}>取消</button>
            </div>
          )}

          {/* Type toggle */}
          <div className={styles.row}>
            <span className={styles.label}>类型</span>
            <div className={styles.typeToggle}>
              <button
                className={`${styles.typeBtn} ${form.type === "stdio" ? styles.typeBtnActive : ""}`}
                onClick={() => setForm({ ...EMPTY_FORM, type: "stdio" } as FormData)}
              >
                stdio
              </button>
              <button
                className={`${styles.typeBtn} ${form.type === "http" ? styles.typeBtnActive : ""}`}
                onClick={() => setForm({ ...EMPTY_FORM, type: "http" } as FormData)}
              >
                HTTP
              </button>
            </div>
          </div>

          <div className={styles.row}>
            <label className={styles.label}>名称</label>
            <input
              className={styles.input}
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="my-mcp"
            />
          </div>

          {form.type === "stdio" && (
            <>
              <div className={styles.row}>
                <label className={styles.label}>命令</label>
                <input
                  className={styles.input}
                  value={form.command}
                  onChange={(e) => setForm((f) => ({ ...f, command: e.target.value } as FormData))}
                  placeholder="npx / uvx / /usr/local/bin/mcp"
                />
              </div>
              <div className={styles.row}>
                <label className={styles.label}>参数</label>
                <input
                  className={styles.input}
                  value={form.args}
                  onChange={(e) => setForm((f) => ({ ...f, args: e.target.value } as FormData))}
                  placeholder="-y @my/mcp-server --flag"
                />
              </div>
              <div className={styles.row}>
                <label className={styles.label}>环境变量 (JSON)</label>
                <textarea
                  className={styles.input}
                  value={form.env}
                  onChange={(e) => setForm((f) => ({ ...f, env: e.target.value } as FormData))}
                  rows={3}
                  placeholder='{"API_KEY": "..."}'
                />
              </div>
            </>
          )}

          {form.type === "http" && (
            <div className={styles.row}>
              <label className={styles.label}>URL</label>
              <input
                className={styles.input}
                value={form.url}
                onChange={(e) => setForm((f) => ({ ...f, url: e.target.value } as FormData))}
                placeholder="http://localhost:8931"
              />
            </div>
          )}

          {error && <p className={styles.error}>{error}</p>}

          <button
            className={styles.submitBtn}
            onClick={handleSubmit}
            disabled={saving}
          >
            {saving ? "保存中…" : editingId ? "保存" : "添加"}
          </button>
        </div>
      </div>
    </div>
  );
}
