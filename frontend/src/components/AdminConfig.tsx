import { useState, useEffect } from "react";
import { api } from "../api/client";
import { useAuth } from "../store/auth.shared";
import {
  listGlobalMcps,
  createGlobalMcp,
  updateGlobalMcp,
  deleteGlobalMcp,
  listCatalog,
  createCatalogEntry,
  deleteCatalogEntry,
} from "../api/admin";
import type { AdminConfig, GlobalMcp, CatalogEntry } from "../api/types";
import { CodeEditor } from "./CodeEditor";
import styles from "./AdminConfig.module.css";

type Tab = "general" | "mcp" | "catalog";

export function AdminConfig() {
  const { token } = useAuth();
  const [config, setConfig] = useState<AdminConfig | null>(null);
  const [mcps, setMcps] = useState<GlobalMcp[]>([]);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [tab, setTab] = useState<Tab>("general");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const [editingMcpId, setEditingMcpId] = useState<string | null>(null);
  const [mcpForm, setMcpForm] = useState<{ name: string; type: "http" | "stdio"; config: any } | null>(null);
  const [envJsonStr, setEnvJsonStr] = useState("{}");
  const [envJsonError, setEnvJsonError] = useState("");

  const [catalogForm, setCatalogForm] = useState({ name: "", description: "", command: "", args: "" });
  const [catalogSaving, setCatalogSaving] = useState(false);

  useEffect(() => {
    if (!token) return;
    Promise.all([api.getAdminConfig(token), listGlobalMcps(token), listCatalog(token)])
      .then(([cfg, mcpList, catalogList]) => { setConfig(cfg); setMcps(mcpList); setCatalog(catalogList); })
      .catch((e) => setError(e instanceof Error ? e.message : "加载配置失败"));
  }, [token]);

  async function handleAddCatalogEntry() {
    if (!token || !catalogForm.name || !catalogForm.command) {
      alert("名称和命令不能为空"); return;
    }
    setCatalogSaving(true);
    try {
      const entry = await createCatalogEntry({
        name: catalogForm.name,
        description: catalogForm.description,
        command: catalogForm.command,
        args: catalogForm.args.split("\n").map((l) => l.trim()).filter(Boolean),
      }, token);
      setCatalog((prev) => [...prev, entry]);
      setCatalogForm({ name: "", description: "", command: "", args: "" });
    } catch (e) {
      alert(e instanceof Error ? e.message : "添加失败");
    } finally {
      setCatalogSaving(false);
    }
  }

  async function handleDeleteCatalogEntry(id: string) {
    if (!token || !confirm("删除此目录项？")) return;
    try {
      await deleteCatalogEntry(id, token);
      setCatalog((prev) => prev.filter((e) => e.id !== id));
    } catch (e) {
      alert(e instanceof Error ? e.message : "删除失败");
    }
  }

  async function handleSave() {
    if (!config || !token) return;
    setSaving(true); setError("");
    try {
      await api.updateAdminConfig(token, config);
      alert("已保存");
    } catch (e) {
      setError(e instanceof Error ? e.message : "保存失败");
    } finally { setSaving(false); }
  }

  function handleAddMcp() {
    setMcpForm({ name: "", type: "http", config: { url: "" } });
    setEditingMcpId(null); setEnvJsonStr("{}"); setEnvJsonError("");
  }

  function handleEditMcp(mcp: GlobalMcp) {
    setEditingMcpId(mcp.id);
    setMcpForm({ name: mcp.name, type: mcp.type, config: JSON.parse(JSON.stringify(mcp.config)) });
    setEnvJsonStr(JSON.stringify(mcp.config.env || {}, null, 2));
    setEnvJsonError("");
  }

  async function handleDeleteMcp(id: string) {
    if (!token || !confirm("确定删除？")) return;
    try { await deleteGlobalMcp(id, token); setMcps(mcps.filter((m) => m.id !== id)); }
    catch (e) { alert(e instanceof Error ? e.message : "删除失败"); }
  }

  async function handleSaveMcp() {
    if (!token || !mcpForm) return;
    if (!mcpForm.name) { alert("请输入名称"); return; }
    const finalConfig = { ...mcpForm.config };
    if (mcpForm.type === "stdio") {
      try { finalConfig.env = JSON.parse(envJsonStr); }
      catch { alert("环境变量 JSON 格式错误"); return; }
    }
    setSaving(true);
    try {
      if (editingMcpId) {
        const updated = await updateGlobalMcp(editingMcpId, { name: mcpForm.name, type: mcpForm.type, config: finalConfig }, token);
        setMcps(mcps.map((m) => (m.id === editingMcpId ? updated : m)));
      } else {
        const created = await createGlobalMcp({ name: mcpForm.name, type: mcpForm.type, config: finalConfig }, token);
        setMcps([...mcps, created]);
      }
      setEditingMcpId(null); setMcpForm(null); setEnvJsonStr("{}"); setEnvJsonError("");
    } catch (e) { alert(e instanceof Error ? e.message : "保存失败"); }
    finally { setSaving(false); }
  }

  if (!config) return <div className={styles.loading}>加载中...</div>;

  return (
    <div className={styles.page}>
      {/* Segment tabs */}
      <div className={styles.segmentBar}>
        {(["general", "mcp", "catalog"] as Tab[]).map((t) => (
          <button
            key={t}
            className={`${styles.segBtn} ${tab === t ? styles.segBtnActive : ""}`}
            onClick={() => setTab(t)}
          >
            {{ general: "通用设置", mcp: "全局 MCP", catalog: "MCP 目录" }[t]}
          </button>
        ))}
      </div>

      {error && <div className={styles.error}>{error}</div>}

      {/* ── 通用设置 ── */}
      {tab === "general" && (
        <div className={styles.panel}>
          <div className={styles.group}>
            <h4>API Keys</h4>
            <div className={styles.fieldRow}>
              <div className={styles.field}>
                <label>Tavily API Key</label>
                <input
                  type="password"
                  value={config.tavily_api_key ?? ""}
                  onChange={(e) => setConfig({ ...config, tavily_api_key: e.target.value || null })}
                  placeholder="tvly-..."
                />
              </div>
              <div className={styles.field}>
                <label>SiliconFlow API Key（仅支持 SFW 内容）</label>
                <input
                  type="password"
                  value={config.siliconflow_api_key ?? ""}
                  onChange={(e) => setConfig({ ...config, siliconflow_api_key: e.target.value || null })}
                  placeholder="sk-..."
                />
              </div>
              <div className={styles.field}>
                <label>fal.ai API Key（支持 NSFW 内容）</label>
                <input
                  type="password"
                  value={config.fal_api_key ?? ""}
                  onChange={(e) => setConfig({ ...config, fal_api_key: e.target.value || null })}
                  placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx:..."
                />
              </div>
            </div>
          </div>

          <div className={styles.actions}>
            <button className={styles.saveBtn} onClick={handleSave} disabled={saving}>
              {saving ? "保存中..." : "保存更改"}
            </button>
          </div>
        </div>
      )}

      {/* ── 全局 MCP ── */}
      {tab === "mcp" && (
        <div className={styles.panel}>
          <p className={styles.hint}>在此配置的 MCP 服务器对所有用户可用。</p>
          <ul className={styles.mcpList}>
            {mcps.map((m) => (
              <li key={m.id} className={`${styles.mcpItem} ${editingMcpId === m.id ? styles.active : ""}`}>
                <div className={styles.mcpInfo}>
                  <span className={styles.mcpName}>{m.name}</span>
                  <span className={styles.mcpDetail}>
                    {m.type === "http" ? `HTTP: ${m.config.url}` : `Stdio: ${m.config.command}`}
                  </span>
                </div>
                <div className={styles.mcpActions}>
                  <button className={styles.iconBtn} onClick={() => handleEditMcp(m)} title="编辑"><EditIcon /></button>
                  <button className={`${styles.iconBtn} ${styles.danger}`} onClick={() => handleDeleteMcp(m.id)} title="删除"><TrashIcon /></button>
                </div>
              </li>
            ))}
          </ul>

          {!mcpForm ? (
            <button className={styles.addBtn} onClick={handleAddMcp}>+ 添加全局 MCP</button>
          ) : (
            <div className={styles.mcpForm}>
              <div className={styles.formHeader}>
                <h4>{editingMcpId ? "编辑 MCP" : "新建 MCP"}</h4>
                <button className={styles.cancelBtn} onClick={() => { setMcpForm(null); setEditingMcpId(null); }}>取消</button>
              </div>
              <div className={styles.field}>
                <label>名称</label>
                <input type="text" value={mcpForm.name}
                  onChange={(e) => setMcpForm({ ...mcpForm, name: e.target.value })} />
              </div>
              <div className={styles.field}>
                <label>类型</label>
                <div className={styles.typeToggle}>
                  {(["http", "stdio"] as const).map((t) => (
                    <button key={t}
                      className={`${styles.typeBtn} ${mcpForm.type === t ? styles.active : ""}`}
                      onClick={() => setMcpForm({ ...mcpForm, type: t, config: t === "http" ? { url: "" } : { command: "", args: [] } })}>
                      {t.toUpperCase()}
                    </button>
                  ))}
                </div>
              </div>
              {mcpForm.type === "http" ? (
                <div className={styles.field}>
                  <label>URL</label>
                  <input type="text" value={mcpForm.config.url || ""}
                    onChange={(e) => setMcpForm({ ...mcpForm, config: { ...mcpForm.config, url: e.target.value } })}
                    placeholder="http://localhost:3000/sse" />
                </div>
              ) : (
                <>
                  <div className={styles.field}>
                    <label>Command</label>
                    <input type="text" value={mcpForm.config.command || ""}
                      onChange={(e) => setMcpForm({ ...mcpForm, config: { ...mcpForm.config, command: e.target.value } })}
                      placeholder="npx" />
                  </div>
                  <div className={styles.field}>
                    <label>Arguments（每行一个）</label>
                    <textarea rows={3} value={Array.isArray(mcpForm.config.args) ? mcpForm.config.args.join("\n") : ""}
                      onChange={(e) => setMcpForm({ ...mcpForm, config: { ...mcpForm.config, args: e.target.value.split("\n").map((l) => l.trim()).filter(Boolean) } })}
                      placeholder={"-y\ntavily-mcp"} />
                  </div>
                  <div className={styles.field}>
                    <label>Environment Variables (JSON)</label>
                    <CodeEditor
                      value={envJsonStr}
                      onChange={(next) => {
                        setEnvJsonStr(next);
                        try { JSON.parse(next || "{}"); setEnvJsonError(""); }
                        catch { setEnvJsonError("JSON 格式错误"); }
                      }}
                      language="json"
                      height={120}
                    />
                    {envJsonError && <div className={styles.fieldError}>{envJsonError}</div>}
                  </div>
                </>
              )}
              <div className={styles.actions}>
                <button className={styles.saveBtn} onClick={handleSaveMcp} disabled={saving}>
                  {saving ? "保存中..." : "确认"}
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* ── MCP 目录 ── */}
      {tab === "catalog" && (
        <div className={styles.panel}>
          <p className={styles.hint}>用户可在个人设置中一键启用这些预配置的服务器。</p>
          <div className={styles.catalogList}>
            {catalog.map((entry) => (
              <div key={entry.id} className={styles.catalogItem}>
                <div className={styles.catalogHeader}>
                  <strong>{entry.name}</strong>
                  <button className={styles.deleteBtn} onClick={() => handleDeleteCatalogEntry(entry.id)}>
                    删除
                  </button>
                </div>
                <div className={styles.catalogDesc}>{entry.description}</div>
                <div className={styles.catalogCmd}><code>{entry.command} {entry.args.join(" ")}</code></div>
              </div>
            ))}
          </div>

          <div className={styles.catalogForm}>
            <h4>添加目录项</h4>
            <div className={styles.fieldRow}>
              <div className={styles.field}>
                <label>名称</label>
                <input type="text" placeholder="filesystem" value={catalogForm.name}
                  onChange={(e) => setCatalogForm({ ...catalogForm, name: e.target.value })} />
              </div>
              <div className={styles.field}>
                <label>描述</label>
                <input type="text" placeholder="文件系统访问工具" value={catalogForm.description}
                  onChange={(e) => setCatalogForm({ ...catalogForm, description: e.target.value })} />
              </div>
              <div className={styles.field}>
                <label>命令</label>
                <input type="text" placeholder="npx" value={catalogForm.command}
                  onChange={(e) => setCatalogForm({ ...catalogForm, command: e.target.value })} />
              </div>
            </div>
            <div className={styles.field}>
              <label>参数（每行一个）</label>
              <textarea rows={3} placeholder="-y&#10;@modelcontextprotocol/server-filesystem"
                value={catalogForm.args}
                onChange={(e) => setCatalogForm({ ...catalogForm, args: e.target.value })} />
            </div>
            <button className={styles.addBtn} onClick={handleAddCatalogEntry} disabled={catalogSaving}>
              {catalogSaving ? "添加中..." : "添加到目录"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function EditIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
      <path d="M10 11v6" /><path d="M14 11v6" />
      <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
    </svg>
  );
}
