import { useState, useEffect, useMemo } from "react";
import hljs from "highlight.js";
import { api } from "../api/client";
import { useAuth } from "../store/auth.shared";
import {
  listGlobalMcps,
  createGlobalMcp,
  updateGlobalMcp,
  deleteGlobalMcp,
} from "../api/admin";
import type { AdminConfig, GlobalMcp, ModelConfig, Provider } from "../api/types";
import styles from "./AdminConfig.module.css";
import "highlight.js/styles/github.css";

const PROVIDER_LABELS: Record<Provider, string> = {
  deepseek: "DeepSeek",
  openai:   "OpenAI",
  anthropic: "Anthropic",
  gemini:   "Gemini",
  kimi:     "Kimi",
  glm:      "GLM",
  minimax:  "MiniMax",
  grok:     "Grok",
};

const PROVIDER_DEFAULTS: Record<Provider, { api_base: string }> = {
  deepseek:  { api_base: "https://api.deepseek.com" },
  openai:    { api_base: "https://api.openai.com/v1" },
  anthropic: { api_base: "https://api.anthropic.com" },
  gemini:    { api_base: "https://generativelanguage.googleapis.com/v1beta" },
  kimi:      { api_base: "https://api.moonshot.cn/v1" },
  glm:       { api_base: "https://open.bigmodel.cn/api/paas/v4" },
  minimax:   { api_base: "https://api.minimaxi.com/anthropic" },
  grok:      { api_base: "https://api.x.ai/v1" },
};

function ModelConfigBlock({
  label,
  value,
  onChange,
}: {
  label: string;
  value: ModelConfig;
  onChange: (v: ModelConfig) => void;
}) {
  const provider: Provider = value.provider ?? "deepseek";

  return (
    <div className={styles.group}>
      <h4>{label}</h4>
      <div className={styles.field}>
        <label>Provider</label>
        <div className={styles.typeToggle}>
          {(Object.keys(PROVIDER_LABELS) as Provider[]).map((p) => (
            <button
              key={p}
              className={`${styles.typeBtn} ${provider === p ? styles.active : ""}`}
              onClick={() => onChange({ ...value, provider: p })}
            >
              {PROVIDER_LABELS[p]}
            </button>
          ))}
        </div>
      </div>
      <div className={styles.fieldRow}>
        <div className={styles.field}>
          <label>Model Name</label>
          <input
            type="text"
            value={value.name}
            onChange={(e) => onChange({ ...value, name: e.target.value })}
          />
        </div>
        <div className={styles.field}>
          <label>API Base</label>
          <input
            type="text"
            value={value.api_base}
            onChange={(e) => onChange({ ...value, api_base: e.target.value })}
            placeholder={PROVIDER_DEFAULTS[provider].api_base}
          />
        </div>
        <div className={styles.field}>
          <label>API Key</label>
          <input
            type="password"
            value={value.api_key}
            onChange={(e) => onChange({ ...value, api_key: e.target.value })}
          />
        </div>
      </div>
    </div>
  );
}

type Tab = "general" | "mcp" | "catalog";

export function AdminConfig() {
  const { token } = useAuth();
  const [config, setConfig] = useState<AdminConfig | null>(null);
  const [mcps, setMcps] = useState<GlobalMcp[]>([]);
  const [tab, setTab] = useState<Tab>("general");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const [editingMcpId, setEditingMcpId] = useState<string | null>(null);
  const [mcpForm, setMcpForm] = useState<{ name: string; type: "http" | "stdio"; config: any } | null>(null);
  const [envJsonStr, setEnvJsonStr] = useState("{}");
  const [envJsonError, setEnvJsonError] = useState("");

  useEffect(() => {
    if (!token) return;
    Promise.all([api.getAdminConfig(token), listGlobalMcps(token)])
      .then(([cfg, mcpList]) => { setConfig(cfg); setMcps(mcpList); })
      .catch((e) => setError(e instanceof Error ? e.message : "加载配置失败"));
  }, [token]);

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

  const highlightedSystemPrompt = useMemo(() => {
    if (!config?.server.system_prompt) return "";
    try { return hljs.highlight(config.server.system_prompt, { language: "markdown" }).value; }
    catch { return config.server.system_prompt; }
  }, [config?.server.system_prompt]);

  const highlightedSubagentPrompt = useMemo(() => {
    if (!config?.server.subagent_prompt) return "";
    try { return hljs.highlight(config.server.subagent_prompt, { language: "markdown" }).value; }
    catch { return config.server.subagent_prompt; }
  }, [config?.server.subagent_prompt]);

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
          <div className={styles.twoCol}>
            <div className={styles.group}>
              <h4>路径</h4>
              <div className={styles.field}>
                <label>Frontend Path (Public)</label>
                <input type="text" value={config.public_path}
                  onChange={(e) => setConfig({ ...config, public_path: e.target.value })} />
              </div>
              <div className={styles.field}>
                <label>Artifacts Path</label>
                <input type="text" value={config.artifacts_path}
                  onChange={(e) => setConfig({ ...config, artifacts_path: e.target.value })} />
              </div>
              <div className={styles.field}>
                <label>Port</label>
                <input type="number" value={config.server.port}
                  onChange={(e) => setConfig({ ...config, server: { ...config.server, port: parseInt(e.target.value) || 3000 } })} />
              </div>
            </div>

            <ModelConfigBlock label="Cheap Model" value={config.cheap_model}
              onChange={(v) => setConfig({ ...config, cheap_model: v })} />

            <ModelConfigBlock label="Embedding Model" value={config.embedding}
              onChange={(v) => setConfig({ ...config, embedding: v })} />
          </div>

          <div className={styles.group}>
            <h4>System Prompt</h4>
            <div className={styles.editorContainer}>
              <textarea className={styles.editorTextarea}
                value={config.server.system_prompt || ""}
                onChange={(e) => setConfig({ ...config, server: { ...config.server, system_prompt: e.target.value } })}
                onScroll={(e) => {
                  const h = e.currentTarget.nextElementSibling as HTMLElement;
                  if (h) { h.scrollTop = e.currentTarget.scrollTop; h.scrollLeft = e.currentTarget.scrollLeft; }
                }}
                placeholder="输入系统提示词..." spellCheck={false} />
              <pre className={styles.editorHighlight} aria-hidden="true"
                dangerouslySetInnerHTML={{ __html: highlightedSystemPrompt + "\n" }} />
            </div>
          </div>

          <div className={styles.group}>
            <h4>Subagent System Prompt</h4>
            <div className={styles.editorContainer}>
              <textarea className={styles.editorTextarea}
                value={config.server.subagent_prompt || ""}
                onChange={(e) => setConfig({ ...config, server: { ...config.server, subagent_prompt: e.target.value } })}
                onScroll={(e) => {
                  const h = e.currentTarget.nextElementSibling as HTMLElement;
                  if (h) { h.scrollTop = e.currentTarget.scrollTop; h.scrollLeft = e.currentTarget.scrollLeft; }
                }}
                placeholder="输入子代理系统提示词..." spellCheck={false} />
              <pre className={styles.editorHighlight} aria-hidden="true"
                dangerouslySetInnerHTML={{ __html: highlightedSubagentPrompt + "\n" }} />
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
                    <textarea rows={3} value={envJsonStr}
                      onChange={(e) => {
                        setEnvJsonStr(e.target.value);
                        try { JSON.parse(e.target.value); setEnvJsonError(""); }
                        catch { setEnvJsonError("JSON 格式错误"); }
                      }}
                      placeholder={'{"KEY": "VALUE"}'}
                      style={{ borderColor: envJsonError ? "var(--danger)" : undefined }} />
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
      {tab === "catalog" && config && (
        <div className={styles.panel}>
          <p className={styles.hint}>用户可在个人设置中一键启用这些预配置的服务器。</p>
          <div className={styles.catalogList}>
            {config.mcp_catalog.map((entry, idx) => (
              <div key={idx} className={styles.catalogItem}>
                <div className={styles.catalogHeader}>
                  <strong>{entry.name}</strong>
                  <button className={styles.deleteBtn}
                    onClick={() => setConfig({ ...config, mcp_catalog: config.mcp_catalog.filter((_, i) => i !== idx) })}>
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
                <input type="text" placeholder="filesystem" id="catalog-name" />
              </div>
              <div className={styles.field}>
                <label>描述</label>
                <input type="text" placeholder="文件系统访问工具" id="catalog-description" />
              </div>
              <div className={styles.field}>
                <label>命令</label>
                <input type="text" placeholder="npx" id="catalog-command" />
              </div>
            </div>
            <div className={styles.field}>
              <label>参数（每行一个）</label>
              <textarea rows={3} placeholder="-y&#10;@modelcontextprotocol/server-filesystem" id="catalog-args" />
            </div>
            <button className={styles.addBtn} onClick={() => {
              const n = (document.getElementById("catalog-name") as HTMLInputElement);
              const d = (document.getElementById("catalog-description") as HTMLInputElement);
              const c = (document.getElementById("catalog-command") as HTMLInputElement);
              const a = (document.getElementById("catalog-args") as HTMLTextAreaElement);
              if (!n.value || !d.value || !c.value) { alert("请填写所有必填字段"); return; }
              setConfig({ ...config, mcp_catalog: [...config.mcp_catalog, { name: n.value, description: d.value, command: c.value, args: a.value.split("\n").filter(Boolean) }] });
              n.value = ""; d.value = ""; c.value = ""; a.value = "";
            }}>添加到目录</button>
          </div>

          <div className={styles.actions}>
            <button className={styles.saveBtn} onClick={handleSave} disabled={saving}>
              {saving ? "保存中..." : "保存目录配置"}
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
