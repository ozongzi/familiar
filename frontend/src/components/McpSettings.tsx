import { useState, useEffect } from "react";
import { api } from "../api/client";
import type { Mcp, CreateMcpRequest } from "../api/types";
import styles from "./McpSettings.module.css";

interface Props {
  token: string;
  onClose: () => void;
}

const EMPTY_FORM: CreateMcpRequest = {
  name: "",
  type: "http",
  config: { url: "" },
};

export function McpSettings({ token, onClose }: Props) {
  const [mcps, setMcps] = useState<Mcp[]>([]);
  const [form, setForm] = useState<CreateMcpRequest>(EMPTY_FORM);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  
  // Separate state for env JSON editing
  const [envJsonStr, setEnvJsonStr] = useState<string>("{}");
  const [envJsonError, setEnvJsonError] = useState<string>("");

  useEffect(() => {
    api.listMcps(token).then(setMcps).catch(() => {});
  }, [token]);

  function resetForm() {
    setForm(EMPTY_FORM);
    setEditingId(null);
    setError("");
    setEnvJsonStr("{}");
    setEnvJsonError("");
  }

  function startEdit(mcp: Mcp) {
    setEditingId(mcp.id);
    setForm({ name: mcp.name, type: mcp.type, config: mcp.config });
    setError("");
    // Initialize envJsonStr from existing env
    const env = (mcp.config.env as Record<string, string>) || {};
    setEnvJsonStr(JSON.stringify(env, null, 2));
    setEnvJsonError("");
  }

  function handleTypeChange(type: "http" | "stdio") {
    setForm((f) => ({
      ...f,
      type,
      config: type === "http" ? { url: "" } : { command: "", args: [] },
    }));
    if (type === "stdio") {
      setEnvJsonStr("{}");
      setEnvJsonError("");
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    
    // Validate and parse env JSON for stdio type
    const finalConfig = { ...form.config };
    if (form.type === 'stdio') {
      try {
        const env = JSON.parse(envJsonStr);
        finalConfig.env = env;
      } catch (e) {
        setError("Environment Variables JSON 格式错误");
        return;
      }
    }
    
    setLoading(true);
    try {
      const submissionForm = { ...form, config: finalConfig };
      if (editingId) {
        const updated = await api.updateMcp(token, editingId, submissionForm);
        setMcps((prev) => prev.map((m) => (m.id === editingId ? updated : m)));
      } else {
        const created = await api.createMcp(token, submissionForm);
        setMcps((prev) => [...prev, created]);
      }
      resetForm();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "操作失败");
    } finally {
      setLoading(false);
    }
  }

  async function handleDelete(id: string) {
    await api.deleteMcp(token, id).catch(() => {});
    setMcps((prev) => prev.filter((m) => m.id !== id));
    if (editingId === id) resetForm();
  }

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2 className={styles.title}>MCP 服务器</h2>
          <button className={styles.closeBtn} onClick={onClose} aria-label="关闭">
            <CloseIcon />
          </button>
        </div>

        {/* List */}
        <ul className={styles.list}>
          {mcps.length === 0 && (
            <li className={styles.empty}>还没有配置 MCP 服务器，在下方添加一个。</li>
          )}
          {mcps.map((mcp) => (
            <li
              key={mcp.id}
              className={`${styles.item} ${editingId === mcp.id ? styles.itemActive : ""}`}
            >
              <div className={styles.itemInfo}>
                <span className={styles.itemName}>{mcp.name}</span>
                <span className={styles.itemType}>{mcp.type}</span>
                <span className={styles.itemDetail}>
                  {mcp.type === "http"
                    ? String(mcp.config.url ?? "")
                    : String(mcp.config.command ?? "")}
                </span>
              </div>
              <div className={styles.itemActions}>
                <button
                  className={styles.iconBtn}
                  onClick={() => startEdit(mcp)}
                  title="编辑"
                >
                  <PencilIcon />
                </button>
                <button
                  className={`${styles.iconBtn} ${styles.danger}`}
                  onClick={() => handleDelete(mcp.id)}
                  title="删除"
                >
                  <TrashIcon />
                </button>
              </div>
            </li>
          ))}
        </ul>

        {/* Form */}
        <form className={styles.form} onSubmit={handleSubmit}>
          <div className={styles.formTitle}>
            <span>{editingId ? "编辑 MCP" : "添加新 MCP"}</span>
            {editingId && (
              <button
                type="button"
                className={styles.cancelBtn}
                onClick={resetForm}
              >
                取消编辑
              </button>
            )}
          </div>

          <div className={styles.row}>
            <label className={styles.label}>名称</label>
            <input
              className={styles.input}
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="e.g. local-filesystem, web-search"
              required
            />
          </div>

          <div className={styles.row}>
            <label className={styles.label}>通信协议</label>
            <div className={styles.typeToggle}>
              {(["http", "stdio"] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  className={`${styles.typeBtn} ${form.type === t ? styles.typeBtnActive : ""}`}
                  onClick={() => handleTypeChange(t)}
                >
                  {t}
                </button>
              ))}
            </div>
          </div>

          {form.type === "http" ? (
            <div className={styles.row}>
              <label className={styles.label}>服务地址 (URL)</label>
              <input
                className={styles.input}
                value={String(form.config.url ?? "")}
                onChange={(e) =>
                  setForm((f) => ({ ...f, config: { url: e.target.value } }))
                }
                placeholder="http://localhost:3001/mcp"
                required
              />
            </div>
          ) : (
            <>
              <div className={styles.row}>
                <label className={styles.label}>启动命令 (Command)</label>
                <input
                  className={styles.input}
                  value={String(form.config.command ?? "")}
                  onChange={(e) =>
                    setForm((f) => ({
                      ...f,
                      config: { ...f.config, command: e.target.value },
                    }))
                  }
                  placeholder="e.g. npx"
                  required
                />
              </div>
              <div className={styles.row}>
                <label className={styles.label}>参数 (Arguments, 每行一个)</label>
                <textarea
                  className={styles.textarea}
                  value={(form.config.args as string[] | undefined ?? []).join("\n")}
                  onChange={(e) =>
                    setForm((f) => ({
                      ...f,
                      config: {
                        ...f.config,
                        args: e.target.value.split("\n").filter(Boolean),
                      },
                    }))
                  }
                  placeholder="-y&#10;@wonderwhy-er/desktop-commander"
                  rows={3}
                  style={{ fontFamily: "monospace", fontSize: "13px" }}
                />
              </div>
              <div className={styles.row}>
                <label className={styles.label}>环境变量 (Environment Variables, JSON)</label>
                <textarea
                  className={styles.textarea}
                  value={envJsonStr}
                  onChange={(e) => {
                    setEnvJsonStr(e.target.value);
                    try {
                      JSON.parse(e.target.value);
                      setEnvJsonError("");
                    } catch (err) {
                      setEnvJsonError(err instanceof Error ? err.message : "Invalid JSON");
                    }
                  }}
                  placeholder='{"API_KEY": "your-key-here"}'
                  rows={4}
                  style={{ fontFamily: "monospace", fontSize: "13px" }}
                />
                {envJsonError && (
                  <span style={{ color: "#ff4444", fontSize: "12px", marginTop: "4px", display: "block" }}>
                    {envJsonError}
                  </span>
                )}
              </div>
            </>
          )}

          {error && <p className={styles.error}>{error}</p>}

          <button className={styles.submitBtn} type="submit" disabled={loading}>
            {loading ? "保存中…" : editingId ? "保存变动" : "添加 MCP"}
          </button>
        </form>
      </div>
    </div>
  );
}

function CloseIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}
function PencilIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </svg>
  );
}
function TrashIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
      <path d="M10 11v6" /><path d="M14 11v6" /><path d="M9 6V4h6v2" />
    </svg>
  );
}
