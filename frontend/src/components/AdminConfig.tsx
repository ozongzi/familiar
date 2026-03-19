import { useState, useEffect, useMemo } from "react";
import hljs from "highlight.js";
import { api } from "../api/client";
import { useAuth } from "../store/auth.shared";
import { 
  listGlobalMcps, 
  createGlobalMcp, 
  updateGlobalMcp, 
  deleteGlobalMcp 
} from "../api/admin";
import type { AdminConfig, GlobalMcp } from "../api/types";
import styles from "./AdminConfig.module.css";
import "highlight.js/styles/github.css";

export function AdminConfig() {
  const { token } = useAuth();
  const [config, setConfig] = useState<AdminConfig | null>(null);
  const [mcps, setMcps] = useState<GlobalMcp[]>([]);
  const [activeTab, setActiveTab] = useState<"general" | "mcp" | "catalog">("general");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  // MCP Editing State
  const [editingMcpId, setEditingMcpId] = useState<string | null>(null);
  const [mcpForm, setMcpForm] = useState<{
    name: string;
    type: "http" | "stdio";
    config: any;
  } | null>(null);
  const [envJsonStr, setEnvJsonStr] = useState<string>("{}");
  const [envJsonError, setEnvJsonError] = useState<string>("");

  useEffect(() => {
    if (token) {
      Promise.all([
        api.getAdminConfig(token),
        listGlobalMcps(token)
      ])
      .then(([cfg, mcpList]) => {
        setConfig(cfg);
        setMcps(mcpList);
      })
      .catch((e) => setError(e instanceof Error ? e.message : "加载配置失败"));
    }
  }, [token]);

  async function handleSave() {
    if (!config || !token) return;
    setSaving(true);
    setError("");
    try {
      await api.updateAdminConfig(token, config);
      alert("全局配置已保存");
    } catch (e) {
      setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      setSaving(false);
    }
  }

  // --- MCP Helpers ---

  function handleAddMcp() {
    setMcpForm({ name: "", type: "http", config: { url: "" } });
    setEditingMcpId(null);
    setEnvJsonStr("{}");
    setEnvJsonError("");
  }

  function handleEditMcp(mcp: GlobalMcp) {
    setEditingMcpId(mcp.id);
    setMcpForm({
      name: mcp.name,
      type: mcp.type,
      config: JSON.parse(JSON.stringify(mcp.config)),
    });
    // Initialize envJsonStr from existing env
    const env = mcp.config.env || {};
    setEnvJsonStr(JSON.stringify(env, null, 2));
    setEnvJsonError("");
  }

  async function handleDeleteMcp(id: string) {
    if (!token || !confirm("确定要删除这个 MCP 服务器吗？")) return;
    try {
      await deleteGlobalMcp(id, token);
      setMcps(mcps.filter(m => m.id !== id));
    } catch (e) {
      alert(e instanceof Error ? e.message : "删除失败");
    }
  }

  async function handleSaveMcp() {
    if (!token || !mcpForm) return;
    
    // Validate
    if (!mcpForm.name) {
      alert("请输入名称");
      return;
    }

    // Validate and parse env JSON for stdio type
    const finalConfig = { ...mcpForm.config };
    if (mcpForm.type === 'stdio') {
      try {
        const env = JSON.parse(envJsonStr);
        finalConfig.env = env;
      } catch (e) {
        alert("Environment Variables JSON 格式错误，请检查");
        return;
      }
    }

    setSaving(true);
    try {
      if (editingMcpId) {
        const updated = await updateGlobalMcp(editingMcpId, {
          name: mcpForm.name,
          type: mcpForm.type,
          config: finalConfig,
        }, token);
        setMcps(mcps.map(m => m.id === editingMcpId ? updated : m));
      } else {
        const created = await createGlobalMcp({
          name: mcpForm.name,
          type: mcpForm.type,
          config: finalConfig,
        }, token);
        setMcps([...mcps, created]);
      }
      setEditingMcpId(null);
      setMcpForm(null);
      setEnvJsonStr("{}");
      setEnvJsonError("");
    } catch (e) {
      alert(e instanceof Error ? e.message : "保存失败");
    } finally {
      setSaving(false);
    }
  }

  // Highlight logic for prompts
  const highlightedSystemPrompt = useMemo(() => {
    if (!config?.server.system_prompt) return "";
    try {
      return hljs.highlight(config.server.system_prompt, { language: "markdown" }).value;
    } catch {
      return config.server.system_prompt;
    }
  }, [config?.server.system_prompt]);

  const highlightedSubagentPrompt = useMemo(() => {
    if (!config?.server.subagent_prompt) return "";
    try {
      return hljs.highlight(config.server.subagent_prompt, { language: "markdown" }).value;
    } catch {
      return config.server.subagent_prompt;
    }
  }, [config?.server.subagent_prompt]);

  if (!config) return <div className={styles.loading}>加载中...</div>;

  return (
    <div className={styles.container}>
      <div className={styles.sidebar}>
        <div className={styles.sidebarTitle}>配置选项</div>
        <button
          className={`${styles.tabBtn} ${activeTab === "general" ? styles.active : ""}`}
          onClick={() => setActiveTab("general")}
        >
          通用设置
        </button>
        <button
          className={`${styles.tabBtn} ${activeTab === "mcp" ? styles.active : ""}`}
          onClick={() => setActiveTab("mcp")}
        >
          全局 MCP 服务器
        </button>
        <button
          className={`${styles.tabBtn} ${activeTab === "catalog" ? styles.active : ""}`}
          onClick={() => setActiveTab("catalog")}
        >
          MCP 目录
        </button>
      </div>

      <div className={styles.content}>
        {error && <div className={styles.error}>{error}</div>}

        {activeTab === "general" && (
          <div className={styles.section}>
            <h3>通用设置</h3>
            
            <div className={styles.group}>
              <h4>路径配置</h4>
              <div className={styles.field}>
                <label>Frontend Path (Public)</label>
                <input
                  type="text"
                  value={config.public_path}
                  onChange={(e) => setConfig({ ...config, public_path: e.target.value })}
                />
              </div>
              <div className={styles.field}>
                <label>Artifacts Path</label>
                <input
                  type="text"
                  value={config.artifacts_path}
                  onChange={(e) => setConfig({ ...config, artifacts_path: e.target.value })}
                />
              </div>
            </div>

            <div className={styles.group}>
              <h4>Server Config</h4>
              <div className={styles.field}>
                <label>Port</label>
                <input
                  type="number"
                  value={config.server.port}
                  onChange={(e) => setConfig({ ...config, server: { ...config.server, port: parseInt(e.target.value) || 3000 } })}
                />
              </div>
              <div className={styles.field}>
                <label>System Prompt</label>
                <div className={styles.editorContainer}>
                  <textarea
                    className={styles.editorTextarea}
                    value={config.server.system_prompt || ""}
                    onChange={(e) => setConfig({ ...config, server: { ...config.server, system_prompt: e.target.value } })}
                    onScroll={(e) => {
                      const target = e.currentTarget;
                      const highlight = target.nextElementSibling as HTMLElement;
                      if (highlight) {
                        highlight.scrollTop = target.scrollTop;
                        highlight.scrollLeft = target.scrollLeft;
                      }
                    }}
                    placeholder="输入系统提示词..."
                    spellCheck={false}
                  />
                  <pre
                    className={styles.editorHighlight}
                    aria-hidden="true"
                    dangerouslySetInnerHTML={{ __html: highlightedSystemPrompt + "\n" }}
                  />
                </div>
              </div>
              <div className={styles.field}>
                <label>Subagent System Prompt</label>
                <div className={styles.editorContainer}>
                  <textarea
                    className={styles.editorTextarea}
                    value={config.server.subagent_prompt || ""}
                    onChange={(e) => setConfig({ ...config, server: { ...config.server, subagent_prompt: e.target.value } })}
                    onScroll={(e) => {
                      const target = e.currentTarget;
                      const highlight = target.nextElementSibling as HTMLElement;
                      if (highlight) {
                        highlight.scrollTop = target.scrollTop;
                        highlight.scrollLeft = target.scrollLeft;
                      }
                    }}
                    placeholder="输入子代理系统提示词..."
                    spellCheck={false}
                  />
                  <pre
                    className={styles.editorHighlight}
                    aria-hidden="true"
                    dangerouslySetInnerHTML={{ __html: highlightedSubagentPrompt + "\n" }}
                  />
                </div>
              </div>
            </div>

            <div className={styles.group}>
              <h4>Frontier Model</h4>
              <div className={styles.field}>
                <label>Name</label>
                <input
                  type="text"
                  value={config.frontier_model.name}
                  onChange={(e) => setConfig({ ...config, frontier_model: { ...config.frontier_model, name: e.target.value } })}
                />
              </div>
              <div className={styles.field}>
                <label>API Base</label>
                <input
                  type="text"
                  value={config.frontier_model.api_base}
                  onChange={(e) => setConfig({ ...config, frontier_model: { ...config.frontier_model, api_base: e.target.value } })}
                />
              </div>
              <div className={styles.field}>
                <label>API Key</label>
                <input
                  type="password"
                  value={config.frontier_model.api_key}
                  onChange={(e) => setConfig({ ...config, frontier_model: { ...config.frontier_model, api_key: e.target.value } })}
                />
              </div>
            </div>

            <div className={styles.group}>
              <h4>Cheap Model</h4>
              <div className={styles.field}>
                <label>Name</label>
                <input
                  type="text"
                  value={config.cheap_model.name}
                  onChange={(e) => setConfig({ ...config, cheap_model: { ...config.cheap_model, name: e.target.value } })}
                />
              </div>
              <div className={styles.field}>
                <label>API Base</label>
                <input
                  type="text"
                  value={config.cheap_model.api_base}
                  onChange={(e) => setConfig({ ...config, cheap_model: { ...config.cheap_model, api_base: e.target.value } })}
                />
              </div>
              <div className={styles.field}>
                <label>API Key</label>
                <input
                  type="password"
                  value={config.cheap_model.api_key}
                  onChange={(e) => setConfig({ ...config, cheap_model: { ...config.cheap_model, api_key: e.target.value } })}
                />
              </div>
            </div>

            <div className={styles.actions}>
              <button className={styles.saveBtn} onClick={handleSave} disabled={saving}>
                {saving ? "保存中..." : "保存更改"}
              </button>
            </div>
          </div>
        )}

        {activeTab === "mcp" && (
          <div className={styles.section}>
            <h3>全局 MCP 服务器</h3>
            <p className={styles.hint} style={{marginBottom: "20px"}}>
              在此配置的 MCP 服务器将对系统内所有用户可用。
            </p>

            <ul className={styles.mcpList}>
              {mcps.map((m) => (
                <li key={m.id} className={`${styles.mcpItem} ${editingMcpId === m.id ? styles.active : ""}`}>
                  <div className={styles.mcpInfo}>
                    <span className={styles.mcpName}>{m.name}</span>
                    <span className={styles.mcpDetail}>
                      {m.type === 'http' 
                        ? `HTTP: ${m.config.url}` 
                        : `Stdio: ${m.config.command}`}
                    </span>
                  </div>
                  <div className={styles.mcpActions}>
                    <button className={styles.iconBtn} onClick={() => handleEditMcp(m)} title="编辑">
                      ✏️
                    </button>
                    <button className={`${styles.iconBtn} ${styles.danger}`} onClick={() => handleDeleteMcp(m.id)} title="删除">
                      🗑️
                    </button>
                  </div>
                </li>
              ))}
            </ul>

            {!mcpForm ? (
               <button className={styles.addBtn} onClick={handleAddMcp}>
                 + 添加全局 MCP
               </button>
            ) : (
              <div className={styles.mcpForm}>
                <div className={styles.formHeader}>
                   <h4>{editingMcpId ? "编辑 MCP" : "新建 MCP"}</h4>
                   <button className={styles.cancelBtn} onClick={() => { setMcpForm(null); setEditingMcpId(null); }}>取消</button>
                </div>
                
                <div className={styles.field}>
                  <label>名称</label>
                  <input
                    type="text"
                    value={mcpForm.name}
                    onChange={(e) => setMcpForm({ ...mcpForm, name: e.target.value })}
                  />
                </div>

                <div className={styles.field}>
                  <label>类型</label>
                  <div className={styles.typeToggle}>
                    <button
                      className={`${styles.typeBtn} ${mcpForm.type === 'http' ? styles.active : ""}`}
                      onClick={() => setMcpForm({ ...mcpForm, type: "http", config: { url: "" } })}
                    >
                      HTTP
                    </button>
                    <button
                      className={`${styles.typeBtn} ${mcpForm.type === 'stdio' ? styles.active : ""}`}
                      onClick={() => setMcpForm({ ...mcpForm, type: "stdio", config: { command: "", args: [] } })}
                    >
                      Stdio
                    </button>
                  </div>
                </div>

                {mcpForm.type === 'http' ? (
                  <div className={styles.field}>
                    <label>URL</label>
                    <input
                      type="text"
                      value={mcpForm.config.url || ""}
                      onChange={(e) => setMcpForm({ ...mcpForm, config: { ...mcpForm.config, url: e.target.value } })}
                      placeholder="http://localhost:3000/sse"
                    />
                  </div>
                ) : (
                  <>
                    <div className={styles.field}>
                      <label>Command</label>
                      <input
                        type="text"
                        value={mcpForm.config.command || ""}
                        onChange={(e) => setMcpForm({ ...mcpForm, config: { ...mcpForm.config, command: e.target.value } })}
                        placeholder="e.g. npx"
                      />
                    </div>
                    <div className={styles.field}>
                      <label>Arguments (one per line or space-separated)</label>
                      <textarea
                        rows={3}
                        value={Array.isArray(mcpForm.config.args) ? mcpForm.config.args.join("\n") : ""}
                        onChange={(e) => {
                          const lines = e.target.value.split("\n").map(l => l.trim()).filter(Boolean);
                          setMcpForm({ ...mcpForm, config: { ...mcpForm.config, args: lines } });
                        }}
                        placeholder="e.g.&#10;-y&#10;tavily-mcp"
                        style={{ 
                          width: "100%", 
                          padding: "8px", 
                          borderRadius: "6px", 
                          border: "1px solid var(--border-subtle)", 
                          background: "var(--bg-base)", 
                          color: "var(--text-base)", 
                          fontFamily: "monospace" 
                        }}
                      />
                      <div style={{ fontSize: "0.85em", color: "var(--text-muted)", marginTop: "4px" }}>
                        每行一个参数，或用空格分隔
                      </div>
                    </div>
                    <div className={styles.field}>
                        <label>Environment Variables (JSON)</label>
                        <textarea
                            rows={3}
                            value={envJsonStr}
                            onChange={(e) => {
                                const value = e.target.value;
                                setEnvJsonStr(value);
                                // Try to parse and show error if invalid
                                try {
                                    JSON.parse(value);
                                    setEnvJsonError("");
                                } catch (err) {
                                    setEnvJsonError("Invalid JSON");
                                }
                            }}
                            placeholder='{"KEY": "VALUE"}'
                            style={{ 
                                width: "100%", 
                                padding: "8px", 
                                borderRadius: "6px", 
                                border: `1px solid ${envJsonError ? "var(--danger)" : "var(--border-subtle)"}`, 
                                background: "var(--bg-base)", 
                                color: "var(--text-base)", 
                                fontFamily: "monospace" 
                            }}
                        />
                        {envJsonError && <div style={{ color: "var(--danger)", fontSize: "0.85em", marginTop: "4px" }}>{envJsonError}</div>}
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

        {activeTab === "catalog" && config && (
          <div className={styles.section}>
            <h3>MCP 目录</h3>
            <p className={styles.hint} style={{marginBottom: "20px"}}>
              配置用户可选择的 MCP 服务器列表。用户可以在个人设置中一键启用这些预配置的服务器。
            </p>

            <div className={styles.catalogList}>
              {config.mcp_catalog.map((entry, idx) => (
                <div key={idx} className={styles.catalogItem}>
                  <div className={styles.catalogHeader}>
                    <strong>{entry.name}</strong>
                    <button
                      className={styles.deleteBtn}
                      onClick={() => {
                        const updated = config.mcp_catalog.filter((_, i) => i !== idx);
                        setConfig({ ...config, mcp_catalog: updated });
                      }}
                    >
                      删除
                    </button>
                  </div>
                  <div className={styles.catalogDesc}>{entry.description}</div>
                  <div className={styles.catalogCmd}>
                    <code>{entry.command} {entry.args.join(" ")}</code>
                  </div>
                </div>
              ))}
            </div>

            <div className={styles.catalogForm}>
              <h4>添加新的 MCP 目录项</h4>
              <div className={styles.field}>
                <label>名称</label>
                <input
                  type="text"
                  placeholder="例如: filesystem"
                  id="catalog-name"
                />
              </div>
              <div className={styles.field}>
                <label>描述</label>
                <input
                  type="text"
                  placeholder="例如: 文件系统访问工具"
                  id="catalog-description"
                />
              </div>
              <div className={styles.field}>
                <label>命令</label>
                <input
                  type="text"
                  placeholder="例如: npx"
                  id="catalog-command"
                />
              </div>
              <div className={styles.field}>
                <label>参数 (每行一个)</label>
                <textarea
                  rows={3}
                  placeholder="-y&#10;@modelcontextprotocol/server-filesystem"
                  id="catalog-args"
                  style={{ fontFamily: "monospace", fontSize: "13px" }}
                />
              </div>
              <button
                className={styles.addBtn}
                onClick={() => {
                  const nameInput = document.getElementById("catalog-name") as HTMLInputElement;
                  const descInput = document.getElementById("catalog-description") as HTMLInputElement;
                  const cmdInput = document.getElementById("catalog-command") as HTMLInputElement;
                  const argsInput = document.getElementById("catalog-args") as HTMLTextAreaElement;
                  
                  if (!nameInput.value || !descInput.value || !cmdInput.value) {
                    alert("请填写所有必填字段");
                    return;
                  }
                  
                  const newEntry = {
                    name: nameInput.value,
                    description: descInput.value,
                    command: cmdInput.value,
                    args: argsInput.value.split("\n").filter(Boolean),
                  };
                  
                  setConfig({
                    ...config,
                    mcp_catalog: [...config.mcp_catalog, newEntry],
                  });
                  
                  nameInput.value = "";
                  descInput.value = "";
                  cmdInput.value = "";
                  argsInput.value = "";
                }}
              >
                添加到目录
              </button>
            </div>

            <div className={styles.actions} style={{ marginTop: "30px" }}>
              <button className={styles.saveBtn} onClick={handleSave} disabled={saving}>
                {saving ? "保存中..." : "保存目录配置"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
