import { useState, useEffect, useCallback } from "react";
import { api } from "../api/client";
import type { Skill, AppSkill, CreateSkillRequest } from "../api/types";
import { CodeEditor } from "./CodeEditor";
import styles from "./SkillsPanel.module.css";

// ── User Skills (per-user) ────────────────────────────────────────────────────

interface UserSkillsPanelProps {
  token: string;
}

export function UserSkillsPanel({ token }: UserSkillsPanelProps) {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Skill | null>(null);
  const [creating, setCreating] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    api.listSkills(token)
      .then(setSkills)
      .catch(() => setError("加载失败"))
      .finally(() => setLoading(false));
  }, [token]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async (id: string) => {
    if (!confirm("确认删除此 Skill？")) return;
    await api.deleteSkill(token, id).catch(() => setError("删除失败"));
    load();
  };

  if (loading) return <p className={styles.muted}>加载中…</p>;

  return (
    <div className={styles.panel}>
      {error && <p className={styles.error}>{error}</p>}
      <div className={styles.header}>
        <h3 className={styles.title}>我的 Skills</h3>
        <button className={styles.addBtn} onClick={() => setCreating(true)}>+ 新建</button>
      </div>
      {skills.length === 0 && <p className={styles.muted}>暂无 Skill</p>}
      <ul className={styles.list}>
        {skills.map((s) => (
          <li key={s.id} className={styles.item}>
            <div className={styles.itemInfo}>
              <span className={styles.itemName}>{s.name}</span>
              {s.description && <span className={styles.itemDesc}>{s.description}</span>}
            </div>
            <div className={styles.itemActions}>
              <button className={styles.editBtn} onClick={() => setEditing(s)}>编辑</button>
              <button className={styles.deleteBtn} onClick={() => handleDelete(s.id)}>删除</button>
            </div>
          </li>
        ))}
      </ul>
      {(creating || editing) && (
        <SkillForm
          token={token}
          initial={editing ?? undefined}
          onSave={async (req) => {
            if (editing) {
              await api.updateSkill(token, editing.id, req);
            } else {
              await api.createSkill(token, req);
            }
            setEditing(null);
            setCreating(false);
            load();
          }}
          onCancel={() => { setEditing(null); setCreating(false); }}
        />
      )}
    </div>
  );
}

// ── App Skills (admin-managed) ────────────────────────────────────────────────

interface AppSkillsPanelProps {
  token: string;
}

export function AppSkillsPanel({ token }: AppSkillsPanelProps) {
  const [skills, setSkills] = useState<AppSkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<AppSkill | null>(null);
  const [creating, setCreating] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    api.listAdminSkills(token)
      .then(setSkills)
      .catch(() => setError("加载失败"))
      .finally(() => setLoading(false));
  }, [token]);

  useEffect(() => { load(); }, [load]);

  const handleDelete = async (id: string) => {
    if (!confirm("确认删除此默认 Skill？")) return;
    await api.deleteAdminSkill(token, id).catch(() => setError("删除失败"));
    load();
  };

  if (loading) return <p className={styles.muted}>加载中…</p>;

  return (
    <div className={styles.panel}>
      {error && <p className={styles.error}>{error}</p>}
      <div className={styles.header}>
        <h3 className={styles.title}>默认 Skills</h3>
        <button className={styles.addBtn} onClick={() => setCreating(true)}>+ 新建</button>
      </div>
      <p className={styles.hint}>默认 Skill 对所有用户生效，会注入 agent 系统提示。</p>
      {skills.length === 0 && <p className={styles.muted}>暂无默认 Skill</p>}
      <ul className={styles.list}>
        {skills.map((s) => (
          <li key={s.id} className={styles.item}>
            <div className={styles.itemInfo}>
              <span className={styles.itemName}>{s.name}</span>
              {s.description && <span className={styles.itemDesc}>{s.description}</span>}
            </div>
            <div className={styles.itemActions}>
              <button className={styles.editBtn} onClick={() => setEditing(s)}>编辑</button>
              <button className={styles.deleteBtn} onClick={() => handleDelete(s.id)}>删除</button>
            </div>
          </li>
        ))}
      </ul>
      {(creating || editing) && (
        <SkillForm
          token={token}
          initial={editing ?? undefined}
          onSave={async (req) => {
            if (editing) {
              await api.updateAdminSkill(token, editing.id, req);
            } else {
              await api.createAdminSkill(token, req);
            }
            setEditing(null);
            setCreating(false);
            load();
          }}
          onCancel={() => { setEditing(null); setCreating(false); }}
        />
      )}
    </div>
  );
}

// ── Shared form ───────────────────────────────────────────────────────────────

interface SkillFormProps {
  token: string;
  initial?: { name: string; description?: string | null; content: string };
  onSave: (req: CreateSkillRequest) => Promise<void>;
  onCancel: () => void;
}

function SkillForm({ initial, onSave, onCancel }: SkillFormProps) {
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [content, setContent] = useState(initial?.content ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!name.trim() || !content.trim()) {
      setError("名称和内容不能为空");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onSave({ name: name.trim(), description: description.trim() || null, content: content.trim() });
    } catch (e) {
      setError((e as Error).message ?? "保存失败");
      setSaving(false);
    }
  };

  return (
    <div className={styles.formOverlay}>
      <div className={styles.form}>
        <h4 className={styles.formTitle}>{initial ? "编辑 Skill" : "新建 Skill"}</h4>
        {error && <p className={styles.error}>{error}</p>}
        <label className={styles.label}>
          名称
          <input className={styles.input} value={name} onChange={(e) => setName(e.target.value)} placeholder="Skill 名称" />
        </label>
        <label className={styles.label}>
          描述（可选）
          <input className={styles.input} value={description ?? ""} onChange={(e) => setDescription(e.target.value)} placeholder="简短说明" />
        </label>
        <label className={styles.label}>
          内容（注入 system prompt 的文本）
          <CodeEditor
            value={content}
            onChange={setContent}
            language="markdown"
            height={220}
          />
        </label>
        <div className={styles.formActions}>
          <button className={styles.saveBtn} onClick={handleSubmit} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
          <button className={styles.cancelBtn} onClick={onCancel}>取消</button>
        </div>
      </div>
    </div>
  );
}
