import { useMemo, useState, useEffect } from "react";
import { marked } from "marked";
import hljs from "highlight.js";
import { api } from "../api/client";
import type {
  AdminConfig,
  AppSkill,
  CreateSkillRequest,
  McpCatalogEntry,
  McpServerConfig,
  Skill,
  UserSettings,
} from "../api/types";
import styles from "./UserSettingsModal.module.css";

interface Props {
  token: string;
  isAdmin: boolean;
  onClose: () => void;
}

type Tab = "skills" | "personal" | "admin";

type SkillDraft = CreateSkillRequest;

marked.setOptions({
  gfm: true,
  breaks: true,
});

function renderMarkdown(md: string): string {
  return marked.parse(md || "", {
    async: false,
    renderer: new marked.Renderer(),
  }) as string;
}

export function UserSettingsModal({ token, isAdmin, onClose }: Props) {
  const [activeTab, setActiveTab] = useState<Tab>("skills");
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [skills, setSkills] = useState<Skill[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [newSkill, setNewSkill] = useState<SkillDraft>({ name: "", description: "", content: "" });

  const [apiKeyInput, setApiKeyInput] = useState("");
  const [promptInput, setPromptInput] = useState("");

  const [adminConfig, setAdminConfig] = useState<AdminConfig | null>(null);
  const [adminSkills, setAdminSkills] = useState<AppSkill[]>([]);
  const [editingAdminSkillId, setEditingAdminSkillId] = useState<string | null>(null);
  const [adminSkillDraft, setAdminSkillDraft] = useState<SkillDraft>({ name: "", description: "", content: "" });

  useEffect(() => {
    api
      .getSettings(token)
      .then((s) => {
        setSettings(s);
        setApiKeyInput(s.api_key ?? "");
        setPromptInput(s.system_prompt ?? "");
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [token]);

  useEffect(() => {
    setSkillsLoading(true);
    api
      .listSkills(token)
      .then(setSkills)
      .catch((e) => setError(e.message))
      .finally(() => setSkillsLoading(false));
  }, [token]);

  useEffect(() => {
    if (!isAdmin || activeTab !== "admin") return;
    Promise.all([api.getAdminConfig(token), api.listAdminSkills(token)])
      .then(([cfg, appSkills]) => {
        setAdminConfig(cfg);
        setAdminSkills(appSkills);
      })
      .catch((e) => setError(e.message));
  }, [activeTab, isAdmin, token]);

  useEffect(() => {
    document.querySelectorAll("pre code").forEach((el) => hljs.highlightElement(el as HTMLElement));
  }, [newSkill.content, adminSkillDraft.content]);

  const personalSave = async () => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      if (settings.mode === "default") {
        await api.updateSettings(token, { mode: "default" });
      } else {
        if (!apiKeyInput.trim() || !promptInput.trim()) {
          throw new Error("自定义模式必须同时配置 API Key 和 System Prompt");
        }
        await api.updateSettings(token, {
          mode: "custom",
          api_key: apiKeyInput,
          system_prompt: promptInput,
        });
      }
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const createSkill = async () => {
    const created = await api.createSkill(token, newSkill);
    setSkills((s) => [...s, created].sort((a, b) => a.name.localeCompare(b.name)));
    setNewSkill({ name: "", description: "", content: "" });
  };

  const saveAdminConfig = async () => {
    if (!adminConfig) return;
    setSaving(true);
    try {
      await api.updateAdminConfig(token, adminConfig);
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const saveAdminSkill = async () => {
    if (!adminSkillDraft.name.trim() || !adminSkillDraft.content.trim()) return;
    if (editingAdminSkillId) {
      const updated = await api.updateAdminSkill(token, editingAdminSkillId, adminSkillDraft);
      setAdminSkills((s) => s.map((x) => (x.id === updated.id ? updated : x)).sort((a, b) => a.name.localeCompare(b.name)));
    } else {
      const created = await api.createAdminSkill(token, adminSkillDraft);
      setAdminSkills((s) => [...s, created].sort((a, b) => a.name.localeCompare(b.name)));
    }
    setEditingAdminSkillId(null);
    setAdminSkillDraft({ name: "", description: "", content: "" });
  };

  const deleteAdminSkill = async (id: string) => {
    await api.deleteAdminSkill(token, id);
    setAdminSkills((s) => s.filter((x) => x.id !== id));
    if (editingAdminSkillId === id) {
      setEditingAdminSkillId(null);
      setAdminSkillDraft({ name: "", description: "", content: "" });
    }
  };

  const newSkillPreview = useMemo(() => renderMarkdown(newSkill.content), [newSkill.content]);
  const adminSkillPreview = useMemo(() => renderMarkdown(adminSkillDraft.content), [adminSkillDraft.content]);

  if (loading || !settings) return null;

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2>用户设置</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            ✕
          </button>
        </div>

        <div className={styles.tabs}>
          <button className={activeTab === "skills" ? styles.tabActive : ""} onClick={() => setActiveTab("skills")}>Skills</button>
          <button className={activeTab === "personal" ? styles.tabActive : ""} onClick={() => setActiveTab("personal")}>个人配置</button>
          {isAdmin && <button className={activeTab === "admin" ? styles.tabActive : ""} onClick={() => setActiveTab("admin")}>管理员</button>}
        </div>

        <div className={styles.content}>
          {error && <div className={styles.error}>{error}</div>}

          {activeTab === "skills" && (
            <div className={styles.section}>
              <h3>我的 Skills</h3>
              <div className={styles.field}><label>名称</label><input value={newSkill.name} onChange={(e) => setNewSkill((s) => ({ ...s, name: e.target.value }))} /></div>
              <div className={styles.field}><label>描述</label><input value={newSkill.description ?? ""} onChange={(e) => setNewSkill((s) => ({ ...s, description: e.target.value }))} /></div>
              <div className={styles.markdownGrid}>
                <div className={styles.field}><label>Markdown</label><textarea value={newSkill.content} onChange={(e) => setNewSkill((s) => ({ ...s, content: e.target.value }))} /></div>
                <div className={styles.preview}><label>预览（代码高亮）</label><div dangerouslySetInnerHTML={{ __html: newSkillPreview }} /></div>
              </div>
              <button className={styles.saveBtn} onClick={createSkill} disabled={!newSkill.name || !newSkill.content}>添加 Skill</button>
              <hr className={styles.divider} />
              {skillsLoading ? <div>加载中...</div> : skills.map((s) => <div key={s.id} className={styles.skillItem}><b>{s.name}</b><span>{s.description}</span></div>)}
            </div>
          )}

          {activeTab === "personal" && (
            <div className={styles.section}>
              <h3>个人配置（全有或全无）</h3>
              <label className={styles.radio}><input type="radio" checked={settings.mode === "custom"} onChange={() => setSettings({ ...settings, mode: "custom" })} /> A. 配置 API Key + System Prompt</label>
              <label className={styles.radio}><input type="radio" checked={settings.mode === "default"} onChange={() => setSettings({ ...settings, mode: "default" })} /> B. 不配置（系统默认配置不可见）</label>

              {settings.mode === "custom" && (
                <>
                  <div className={styles.field}><label>API Key</label><input type="password" value={apiKeyInput} onChange={(e) => setApiKeyInput(e.target.value)} /></div>
                  <div className={styles.field}><label>System Prompt</label><textarea value={promptInput} onChange={(e) => setPromptInput(e.target.value)} /></div>
                </>
              )}
            </div>
          )}

          {activeTab === "admin" && isAdmin && adminConfig && (
            <div className={styles.section}>
              <h3>全局配置</h3>
              <div className={styles.twoCol}>
                <div className={styles.field}><label>public_path</label><input value={adminConfig.public_path} onChange={(e) => setAdminConfig({ ...adminConfig, public_path: e.target.value })} /></div>
                <div className={styles.field}><label>artifacts_path</label><input value={adminConfig.artifacts_path} onChange={(e) => setAdminConfig({ ...adminConfig, artifacts_path: e.target.value })} /></div>
              </div>
              <div className={styles.twoCol}>
                <div className={styles.field}><label>server.port</label><input type="number" value={adminConfig.server.port} onChange={(e) => setAdminConfig({ ...adminConfig, server: { ...adminConfig.server, port: Number(e.target.value) } })} /></div>
                <div className={styles.field}><label>server.subagent_prompt</label><input value={adminConfig.server.subagent_prompt ?? ""} onChange={(e) => setAdminConfig({ ...adminConfig, server: { ...adminConfig.server, subagent_prompt: e.target.value || null } })} /></div>
              </div>
              <div className={styles.field}><label>server.system_prompt</label><textarea value={adminConfig.server.system_prompt ?? ""} onChange={(e) => setAdminConfig({ ...adminConfig, server: { ...adminConfig.server, system_prompt: e.target.value || null } })} /></div>

              <ModelEditor title="frontier_model" model={adminConfig.frontier_model} onChange={(m) => setAdminConfig({ ...adminConfig, frontier_model: m })} />
              <ModelEditor title="cheap_model" model={adminConfig.cheap_model} onChange={(m) => setAdminConfig({ ...adminConfig, cheap_model: m })} />
              <ModelEditor title="embedding" model={adminConfig.embedding} onChange={(m) => setAdminConfig({ ...adminConfig, embedding: m })} />

              <McpListEditor
                title="MCP Servers"
                items={adminConfig.mcp}
                onChange={(mcp) => setAdminConfig({ ...adminConfig, mcp })}
              />
              <McpCatalogEditor
                items={adminConfig.mcp_catalog}
                onChange={(mcp_catalog) => setAdminConfig({ ...adminConfig, mcp_catalog })}
              />

              <h3>默认 Skills（数据库）</h3>
              <div className={styles.skillTable}>
                {adminSkills.map((s) => (
                  <div key={s.id} className={styles.skillItemRow}>
                    <div><b>{s.name}</b><p>{s.description}</p></div>
                    <div className={styles.rowBtns}>
                      <button onClick={() => { setEditingAdminSkillId(s.id); setAdminSkillDraft({ name: s.name, description: s.description ?? "", content: s.content }); }}>编辑</button>
                      <button onClick={() => deleteAdminSkill(s.id)} className={styles.deleteBtn}>删除</button>
                    </div>
                  </div>
                ))}
              </div>

              <div className={styles.field}><label>Skill 名称</label><input value={adminSkillDraft.name} onChange={(e) => setAdminSkillDraft((x) => ({ ...x, name: e.target.value }))} /></div>
              <div className={styles.field}><label>Skill 描述</label><input value={adminSkillDraft.description ?? ""} onChange={(e) => setAdminSkillDraft((x) => ({ ...x, description: e.target.value }))} /></div>
              <div className={styles.markdownGrid}>
                <div className={styles.field}><label>Skill Markdown</label><textarea value={adminSkillDraft.content} onChange={(e) => setAdminSkillDraft((x) => ({ ...x, content: e.target.value }))} /></div>
                <div className={styles.preview}><label>预览（代码高亮）</label><div dangerouslySetInnerHTML={{ __html: adminSkillPreview }} /></div>
              </div>
              <button className={styles.saveBtn} onClick={saveAdminSkill}>{editingAdminSkillId ? "更新 Skill" : "新增 Skill"}</button>
            </div>
          )}
        </div>

        <div className={styles.footer}>
          <button className={styles.cancelBtn} onClick={onClose}>取消</button>
          <button className={styles.saveBtn} onClick={activeTab === "admin" ? saveAdminConfig : personalSave} disabled={saving}>{saving ? "保存中..." : "保存"}</button>
        </div>
      </div>
    </div>
  );
}

function ModelEditor({ title, model, onChange }: { title: string; model: AdminConfig["frontier_model"]; onChange: (m: AdminConfig["frontier_model"]) => void }) {
  return (
    <div className={styles.modelCard}>
      <h4>{title}</h4>
      <div className={styles.twoCol}>
        <div className={styles.field}><label>name</label><input value={model.name} onChange={(e) => onChange({ ...model, name: e.target.value })} /></div>
        <div className={styles.field}><label>api_base</label><input value={model.api_base} onChange={(e) => onChange({ ...model, api_base: e.target.value })} /></div>
      </div>
      <div className={styles.field}><label>api_key</label><input value={model.api_key} onChange={(e) => onChange({ ...model, api_key: e.target.value })} /></div>
    </div>
  );
}

function McpListEditor({ title, items, onChange }: { title: string; items: McpServerConfig[]; onChange: (v: McpServerConfig[]) => void }) {
  const update = (idx: number, next: McpServerConfig) => onChange(items.map((x, i) => (i === idx ? next : x)));
  return (
    <div className={styles.modelCard}>
      <h4>{title}</h4>
      {items.map((m, i) => (
        <div key={i} className={styles.skillItemRow}>
          {"url" in m ? (
            <>
              <input value={m.name} onChange={(e) => update(i, { ...m, name: e.target.value })} />
              <input value={m.url} onChange={(e) => update(i, { ...m, url: e.target.value })} />
            </>
          ) : (
            <>
              <input value={m.name} onChange={(e) => update(i, { ...m, name: e.target.value })} />
              <input value={m.command} onChange={(e) => update(i, { ...m, command: e.target.value })} />
            </>
          )}
          <button onClick={() => onChange(items.filter((_, idx) => idx !== i))} className={styles.deleteBtn}>删</button>
        </div>
      ))}
      <button onClick={() => onChange([...items, { name: "new", url: "http://" }])}>+ HTTP MCP</button>
      <button onClick={() => onChange([...items, { name: "new", command: "", args: [], env: {} }])}>+ Stdio MCP</button>
    </div>
  );
}

function McpCatalogEditor({ items, onChange }: { items: McpCatalogEntry[]; onChange: (v: McpCatalogEntry[]) => void }) {
  const update = (idx: number, next: McpCatalogEntry) => onChange(items.map((x, i) => (i === idx ? next : x)));
  return (
    <div className={styles.modelCard}>
      <h4>MCP Catalog</h4>
      {items.map((m, i) => (
        <div key={i} className={styles.skillItemRow}>
          <input value={m.name} onChange={(e) => update(i, { ...m, name: e.target.value })} />
          <input value={m.command} onChange={(e) => update(i, { ...m, command: e.target.value })} />
          <button onClick={() => onChange(items.filter((_, idx) => idx !== i))} className={styles.deleteBtn}>删</button>
        </div>
      ))}
      <button onClick={() => onChange([...items, { name: "new", description: "", command: "", args: [] }])}>+ Catalog</button>
    </div>
  );
}
