import { useEffect, useState } from "react";
import { api } from "../api/client";
import type { Model, UpsertModelRequest } from "../api/types";
import { ProviderSelector } from "./ProviderSelector";
import styles from "./AdminModelsPanel.module.css";


const EMPTY_FORM: UpsertModelRequest = {
  label: "",
  provider: "deepseek",
  model_name: "",
  api_base: "",
  api_key: "",
};

interface Props {
  token: string;
}

export function AdminModelsPanel({ token }: Props) {
  const [models, setModels] = useState<Model[]>([]);
  const [editing, setEditing] = useState<string | null>(null); // id or "new"
  const [form, setForm] = useState<UpsertModelRequest>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function load() {
    try {
      const data = await api.adminListModels(token);
      setModels(data);
    } catch {
      setError("加载失败");
    }
  }

  useEffect(() => { load(); }, [token]);

  function startNew() {
    setForm(EMPTY_FORM);
    setEditing("new");
    setError("");
  }

  function startEdit(m: Model) {
    setForm({
      label: m.label,
      provider: m.provider,
      model_name: m.model_name,
      api_base: m.api_base,
      api_key: "",
    });
    setEditing(m.id);
    setError("");
  }

  function cancel() {
    setEditing(null);
    setError("");
  }

  async function save() {
    if (!form.label.trim() || !form.model_name.trim()) {
      setError("名称和模型名不能为空");
      return;
    }
    setSaving(true);
    setError("");
    try {
      if (editing === "new") {
        const created = await api.adminCreateModel(token, form);
        setModels((prev) => [...prev, created]);
      } else if (editing) {
        const updated = await api.adminUpdateModel(token, editing, form);
        setModels((prev) => prev.map((m) => (m.id === editing ? updated : m)));
      }
      setEditing(null);
    } catch (e: any) {
      setError(e.message ?? "保存失败");
    } finally {
      setSaving(false);
    }
  }

  async function remove(id: string) {
    if (!confirm("删除此模型？")) return;
    try {
      await api.adminDeleteModel(token, id);
      setModels((prev) => prev.filter((m) => m.id !== id));
    } catch (e: any) {
      setError(e.message ?? "删除失败");
    }
  }

  async function setDefault(id: string) {
    try {
      await api.adminSetDefaultModel(token, id);
      setModels((prev) =>
        prev.map((m) => ({ ...m, is_default: m.id === id }))
      );
    } catch (e: any) {
      setError(e.message ?? "设置失败");
    }
  }

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3>全局模型</h3>
        <button className={styles.addBtn} onClick={startNew}>+ 添加</button>
      </div>

      {error && <p className={styles.error}>{error}</p>}

      {editing && (
        <div className={styles.form}>
          <div className={styles.row}>
            <label>显示名称</label>
            <input
              value={form.label}
              onChange={(e) => setForm({ ...form, label: e.target.value })}
              placeholder="e.g. Claude Sonnet"
            />
          </div>
          <div className={styles.row}>
            <label>Provider</label>
            <ProviderSelector value={form.provider} onChange={(p) => setForm({ ...form, provider: p })} />
          </div>
          <div className={styles.row}>
            <label>Model Name</label>
            <input
              value={form.model_name}
              onChange={(e) => setForm({ ...form, model_name: e.target.value })}
              placeholder="e.g. claude-sonnet-4-5"
            />
          </div>
          <div className={styles.row}>
            <label>API Base</label>
            <input
              value={form.api_base}
              onChange={(e) => setForm({ ...form, api_base: e.target.value })}
              placeholder="留空使用默认"
            />
          </div>
          <div className={styles.row}>
            <label>API Key</label>
            <input
              type="password"
              value={form.api_key}
              onChange={(e) => setForm({ ...form, api_key: e.target.value })}
              placeholder={editing !== "new" ? "不修改则留空" : ""}
            />
          </div>
          <div className={styles.actions}>
            <button className={styles.saveBtn} onClick={save} disabled={saving}>
              {saving ? "保存中…" : "保存"}
            </button>
            <button className={styles.cancelBtn} onClick={cancel}>取消</button>
          </div>
        </div>
      )}

      <div className={styles.list}>
        {models.length === 0 && !editing && (
          <p className={styles.empty}>暂无全局模型，点击"添加"创建</p>
        )}
        {models.map((m) => (
          <div key={m.id} className={`${styles.item} ${m.is_default ? styles.itemDefault : ""}`}>
            <div className={styles.itemInfo}>
              <span className={styles.itemLabel}>
                {m.label}
                {m.is_default && <span className={styles.defaultBadge}>默认</span>}
              </span>
              <span className={styles.itemMeta}>{m.provider} · {m.model_name}</span>
            </div>
            <div className={styles.itemActions}>
              {!m.is_default && (
                <button onClick={() => setDefault(m.id)}>设为默认</button>
              )}
              <button onClick={() => startEdit(m)}>编辑</button>
              <button className={styles.deleteBtn} onClick={() => remove(m.id)}>删除</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
